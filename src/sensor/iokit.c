#include <IOKit/IOKitLib.h>
#include <IOKit/hid/IOHIDDevice.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>

// Ring buffer shared between C callback and Rust reader.
// Layout: [write_idx: u32][total: u64][padding: u32][samples: N * 12 bytes]
#define RING_CAP 8000
#define RING_ENTRY 12
#define RING_HEADER 16
#define RING_SIZE (RING_HEADER + RING_CAP * RING_ENTRY)

// IMU constants (Bosch BMI286 via AppleSPUHIDDevice)
#define IMU_REPORT_LEN 22
#define IMU_DATA_OFFSET 6
#define IMU_DECIMATION 8
#define REPORT_BUF_SIZE 4096
#define REPORT_INTERVAL_US 1000
#define IMU_RAW_L1_MAX 1048576LL      // 16g in Q16
#define IMU_LOCK_L1_MIN 32768LL       // 0.5g in Q16
#define IMU_LOCK_L1_MAX 262144LL      // 4g in Q16
#define IMU_LOCK_HITS 6
#define IMU_DEVICE_LOCK_HITS 3

// HID usage pages
#define PAGE_VENDOR 0xFF00
#define USAGE_ACCEL_SPANK 3
#define USAGE_ACCEL_FALLBACK 255

// CFNumber types
#define kCFNumberSInt32Type 3
#define kCFNumberSInt64Type 4

static uint8_t g_ring[RING_SIZE];
static int g_decimation_count = 0;
static int g_callback_count = 0;
static int g_selected_device_idx = -1;
static int g_candidate_device_idx = -1;
static int g_candidate_device_hits = 0;
static int g_selected_report_id = -1;
static int g_candidate_report_id = -1;
static int g_candidate_report_hits = 0;

// Keep report buffers and device refs alive (one per device)
#define MAX_DEVICES 8
static uint8_t g_report_bufs[MAX_DEVICES][REPORT_BUF_SIZE];
static IOHIDDeviceRef g_devices[MAX_DEVICES];
static int64_t g_device_usage[MAX_DEVICES];
static int g_device_count = 0;

// Forward declaration.
static void accel_callback(void *context, IOReturn result, void *sender,
                           IOHIDReportType type, uint32_t reportID,
                           uint8_t *report, CFIndex reportLength);

static void register_accel_callback_for_idx(int idx) {
    IOHIDDeviceRegisterInputReportCallback(
        g_devices[idx], g_report_bufs[idx], REPORT_BUF_SIZE,
        accel_callback, (void*)(intptr_t)idx);
    fprintf(stderr, "iokit: registered accel callback on idx=%d usage=%lld\n",
            idx, g_device_usage[idx]);
}

static bool is_accel_usage(int64_t usage) {
    return usage == USAGE_ACCEL_SPANK || usage == USAGE_ACCEL_FALLBACK;
}

// Write a sample into the ring buffer
static void ring_write_sample(int32_t x, int32_t y, int32_t z) {
    uint32_t idx;
    memcpy(&idx, &g_ring[0], 4);

    size_t off = RING_HEADER + (size_t)idx * RING_ENTRY;
    memcpy(&g_ring[off], &x, 4);
    memcpy(&g_ring[off + 4], &y, 4);
    memcpy(&g_ring[off + 8], &z, 4);

    uint32_t next_idx = (idx + 1) % RING_CAP;
    memcpy(&g_ring[0], &next_idx, 4);

    uint64_t total;
    memcpy(&total, &g_ring[4], 8);
    total++;
    memcpy(&g_ring[4], &total, 8);
}

// HID input report callback
static void accel_callback(void *context, IOReturn result, void *sender,
                           IOHIDReportType type, uint32_t reportID,
                           uint8_t *report, CFIndex reportLength) {
    (void)result; (void)sender; (void)type;

    int device_idx = (int)(intptr_t)context;

    g_callback_count++;

    // Log first 10 reports
    if (g_callback_count <= 10) {
        fprintf(stderr, "iokit: report #%d: dev=%d id=%u len=%ld bytes=[",
                g_callback_count, device_idx, reportID, (long)reportLength);
        int show = reportLength < 32 ? (int)reportLength : 32;
        for (int i = 0; i < show; i++) {
            fprintf(stderr, "%s%02x", i ? " " : "", report[i]);
        }
        fprintf(stderr, "]\n");
    } else if (g_callback_count == 11) {
        fprintf(stderr, "iokit: (suppressing further report logs)\n");
    }

    // Accept only BMI286-length reports.
    if (reportLength != IMU_REPORT_LEN) return;

    // Parse raw XYZ (Q16 fixed-point) and validate range with 64-bit math.
    int32_t x, y, z;
    memcpy(&x, &report[IMU_DATA_OFFSET], 4);
    memcpy(&y, &report[IMU_DATA_OFFSET + 4], 4);
    memcpy(&z, &report[IMU_DATA_OFFSET + 8], 4);

    int64_t ax = x >= 0 ? (int64_t)x : -(int64_t)x;
    int64_t ay = y >= 0 ? (int64_t)y : -(int64_t)y;
    int64_t az = z >= 0 ? (int64_t)z : -(int64_t)z;
    int64_t l1 = ax + ay + az;
    if (l1 <= 0 || l1 > IMU_RAW_L1_MAX) return;

    // Lock to the active accel device first (supports model differences where
    // accel may be Usage=3 or Usage=255).
    if (g_selected_device_idx < 0) {
        if (l1 < IMU_LOCK_L1_MIN || l1 > IMU_LOCK_L1_MAX) return;

        if (device_idx == g_candidate_device_idx) {
            g_candidate_device_hits++;
        } else {
            g_candidate_device_idx = device_idx;
            g_candidate_device_hits = 1;
        }

        if (g_candidate_device_hits >= IMU_DEVICE_LOCK_HITS) {
            g_selected_device_idx = g_candidate_device_idx;
            fprintf(stderr, "iokit: locked accelerometer device idx=%d usage=%lld\n",
                    g_selected_device_idx, g_device_usage[g_selected_device_idx]);
        }
        return;
    }

    if (device_idx != g_selected_device_idx) return;

    // Lock to a stable report ID on the selected device. This avoids mixing
    // different report formats from the same HID device.
    if (g_selected_report_id < 0) {
        if (l1 < IMU_LOCK_L1_MIN || l1 > IMU_LOCK_L1_MAX) return;

        if ((int)reportID == g_candidate_report_id) {
            g_candidate_report_hits++;
        } else {
            g_candidate_report_id = (int)reportID;
            g_candidate_report_hits = 1;
        }

        if (g_candidate_report_hits >= IMU_LOCK_HITS) {
            g_selected_report_id = g_candidate_report_id;
            fprintf(stderr, "iokit: locked accelerometer reportID=%d\n", g_selected_report_id);
        }
        return;
    }

    if ((int)reportID != g_selected_report_id) return;

    g_decimation_count++;
    if (g_decimation_count < IMU_DECIMATION) return;
    g_decimation_count = 0;

    ring_write_sample(x, y, z);
}

// Helper: set integer property on IOService
static void set_int_property(io_service_t service, CFStringRef key, int32_t value) {
    CFNumberRef num = CFNumberCreate(NULL, kCFNumberSInt32Type, &value);
    if (num) {
        IORegistryEntrySetCFProperty(service, key, num);
        CFRelease(num);
    }
}

// Helper: read integer property from IOService
static int64_t get_int_property(io_service_t service, CFStringRef key) {
    int64_t val = 0;
    CFTypeRef prop = IORegistryEntryCreateCFProperty(service, key, kCFAllocatorDefault, 0);
    if (prop) {
        CFNumberGetValue(prop, kCFNumberSInt64Type, &val);
        CFRelease(prop);
    }
    return val;
}

// Wake up SPU sensor drivers — same as spank: mainPort=0
static int wake_spu_drivers(void) {
    CFMutableDictionaryRef matching = IOServiceMatching("AppleSPUHIDDriver");
    io_iterator_t iter;
    // Use 0 for mainPort, same as spank (Go passes 0)
    kern_return_t kr = IOServiceGetMatchingServices(0, matching, &iter);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "iokit: AppleSPUHIDDriver matching failed (kr=%d)\n", kr);
        return -1;
    }

    int count = 0;
    io_service_t svc;
    while ((svc = IOIteratorNext(iter)) != 0) {
        set_int_property(svc, CFSTR("SensorPropertyReportingState"), 1);
        set_int_property(svc, CFSTR("SensorPropertyPowerState"), 1);
        set_int_property(svc, CFSTR("ReportInterval"), REPORT_INTERVAL_US);
        IOObjectRelease(svc);
        count++;
    }
    IOObjectRelease(iter);
    fprintf(stderr, "iokit: woke %d SPU drivers\n", count);
    return 0;
}

// Register HID device callbacks — matches spank's approach exactly:
// IOServiceMatching → IOHIDDeviceCreate → Open → RegisterInputReport → ScheduleWithRunLoop
// MUST be called from the same thread that runs CFRunLoop.
static int register_hid_devices(void) {
    CFMutableDictionaryRef matching = IOServiceMatching("AppleSPUHIDDevice");
    io_iterator_t iter;
    // Use 0 for mainPort, same as spank
    kern_return_t kr = IOServiceGetMatchingServices(0, matching, &iter);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "iokit: AppleSPUHIDDevice matching failed (kr=%d)\n", kr);
        return -1;
    }

    int callbacks = 0;
    int total = 0;
    io_service_t svc;
    while ((svc = IOIteratorNext(iter)) != 0) {
        total++;
        int64_t up = get_int_property(svc, CFSTR("PrimaryUsagePage"));
        int64_t u = get_int_property(svc, CFSTR("PrimaryUsage"));
        fprintf(stderr, "iokit: device %d: UsagePage=0x%llx Usage=%lld\n", total, up, u);

        // Filter: open all devices on UsagePage 0xff00 (vendor sensors: accel, gyro, ALS)
        // and filter by data format in the callback
        if (up != PAGE_VENDOR) {
            fprintf(stderr, "iokit: skipping device %d (UsagePage=0x%llx not 0xff00)\n", total, up);
            IOObjectRelease(svc);
            continue;
        }

        // Always open the device to wake it, but only register callback for accel
        {
            IOHIDDeviceRef hid = IOHIDDeviceCreate(kCFAllocatorDefault, svc);
            if (hid) {
                kr = IOHIDDeviceOpen(hid, kIOHIDOptionsTypeNone);
                if (kr == kIOReturnSuccess) {
                    if (g_device_count >= MAX_DEVICES) {
                        fprintf(stderr, "iokit: device limit reached (%d), skipping device %d\n",
                                MAX_DEVICES, total);
                        CFRelease(hid);
                        IOObjectRelease(svc);
                        continue;
                    }

                    int idx = g_device_count;
                    g_devices[idx] = hid; // keep reference alive
                    g_device_count++;

                    // Schedule FIRST, then register callback
                    IOHIDDeviceScheduleWithRunLoop(
                        hid, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);

                    g_device_usage[idx] = u;

                    if (is_accel_usage(u)) {
                        register_accel_callback_for_idx(idx);
                        callbacks++;
                        fprintf(stderr, "iokit: opened accel-candidate device %d (usage=%lld)\n", total, u);
                    } else {
                        fprintf(stderr, "iokit: opened device %d (usage=%lld) with no callback\n", total, u);
                    }
                } else {
                    fprintf(stderr, "iokit: IOHIDDeviceOpen failed (kr=0x%x)\n", kr);
                    CFRelease(hid);
                }
            } else {
                fprintf(stderr, "iokit: IOHIDDeviceCreate failed for device %d\n", total);
            }
        }
        IOObjectRelease(svc);
    }
    IOObjectRelease(iter);
    fprintf(stderr, "iokit: found %d devices total, opened %d (callbacks=%d)\n",
            total, g_device_count, callbacks);
    return callbacks > 0 ? 0 : -1;
}

// --- Public API called from Rust ---

int iokit_sensor_init(void) {
    memset(g_ring, 0, RING_SIZE);
    g_decimation_count = 0;
    g_callback_count = 0;
    g_selected_device_idx = -1;
    g_candidate_device_idx = -1;
    g_candidate_device_hits = 0;
    g_selected_report_id = -1;
    g_candidate_report_id = -1;
    g_candidate_report_hits = 0;
    g_device_count = 0;
    memset(g_devices, 0, sizeof(g_devices));
    memset(g_device_usage, 0, sizeof(g_device_usage));
    return 0;
}

// Wake drivers, register devices, and run CFRunLoop — ALL on one thread.
// Matches spank's Go code exactly: everything on the locked OS thread.
void iokit_sensor_run(void) {
    if (wake_spu_drivers() != 0) {
        fprintf(stderr, "iokit: failed to wake SPU drivers\n");
        return;
    }
    if (register_hid_devices() != 0) {
        fprintf(stderr, "iokit: failed to register HID devices\n");
        return;
    }
    fprintf(stderr, "iokit: running CFRunLoop with %d devices\n", g_device_count);
    while (1) {
        int32_t result = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, false);
        // Log run loop status periodically
        if (g_callback_count == 0) {
            static int loop_count = 0;
            loop_count++;
            if (loop_count <= 3) {
                fprintf(stderr, "iokit: runloop iteration %d (result=%d, callbacks=%d)\n",
                        loop_count, result, g_callback_count);
            }
        }
    }
}

const uint8_t* iokit_ring_ptr(void) {
    return g_ring;
}

int iokit_ring_size(void) {
    return RING_SIZE;
}
