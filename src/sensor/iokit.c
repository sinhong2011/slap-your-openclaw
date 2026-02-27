#include <IOKit/IOKitLib.h>
#include <IOKit/hid/IOHIDDevice.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdint.h>
#include <string.h>
#include <stdatomic.h>

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

// HID usage pages/usages for Apple SPU sensors
#define PAGE_VENDOR 0xFF00
#define USAGE_ACCEL 3

// CFNumber types
#define kCFNumberSInt32Type 3
#define kCFNumberSInt64Type 4

static uint8_t g_ring[RING_SIZE];
static int g_decimation_count = 0;
static uint8_t g_report_buf[REPORT_BUF_SIZE];

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
    (void)context; (void)result; (void)sender; (void)type; (void)reportID;

    if (reportLength != IMU_REPORT_LEN) return;

    g_decimation_count++;
    if (g_decimation_count < IMU_DECIMATION) return;
    g_decimation_count = 0;

    int32_t x, y, z;
    memcpy(&x, &report[IMU_DATA_OFFSET], 4);
    memcpy(&y, &report[IMU_DATA_OFFSET + 4], 4);
    memcpy(&z, &report[IMU_DATA_OFFSET + 8], 4);
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

// Wake up SPU sensor drivers
static int wake_spu_drivers(void) {
    CFMutableDictionaryRef matching = IOServiceMatching("AppleSPUHIDDriver");
    io_iterator_t iter;
    kern_return_t kr = IOServiceGetMatchingServices(kIOMainPortDefault, matching, &iter);
    if (kr != KERN_SUCCESS) return -1;

    io_service_t svc;
    while ((svc = IOIteratorNext(iter)) != 0) {
        set_int_property(svc, CFSTR("SensorPropertyReportingState"), 1);
        set_int_property(svc, CFSTR("SensorPropertyPowerState"), 1);
        set_int_property(svc, CFSTR("ReportInterval"), REPORT_INTERVAL_US);
        IOObjectRelease(svc);
    }
    IOObjectRelease(iter);
    return 0;
}

// Register HID device callbacks for accelerometer
static int register_hid_devices(void) {
    CFMutableDictionaryRef matching = IOServiceMatching("AppleSPUHIDDevice");
    io_iterator_t iter;
    kern_return_t kr = IOServiceGetMatchingServices(kIOMainPortDefault, matching, &iter);
    if (kr != KERN_SUCCESS) return -1;

    int found = 0;
    io_service_t svc;
    while ((svc = IOIteratorNext(iter)) != 0) {
        int64_t up = get_int_property(svc, CFSTR("PrimaryUsagePage"));
        int64_t u = get_int_property(svc, CFSTR("PrimaryUsage"));

        if (up == PAGE_VENDOR && u == USAGE_ACCEL) {
            IOHIDDeviceRef hid = IOHIDDeviceCreate(kCFAllocatorDefault, svc);
            if (hid) {
                kr = IOHIDDeviceOpen(hid, kIOHIDOptionsTypeNone);
                if (kr == kIOReturnSuccess) {
                    IOHIDDeviceRegisterInputReportCallback(
                        hid, g_report_buf, REPORT_BUF_SIZE,
                        accel_callback, NULL);
                    IOHIDDeviceScheduleWithRunLoop(
                        hid, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
                    found++;
                }
            }
        }
        IOObjectRelease(svc);
    }
    IOObjectRelease(iter);
    return found > 0 ? 0 : -1;
}

// --- Public API called from Rust ---

// Initialize sensor: wake drivers, register callbacks.
// Returns 0 on success, -1 on failure.
int iokit_sensor_init(void) {
    memset(g_ring, 0, RING_SIZE);
    if (wake_spu_drivers() != 0) return -1;
    if (register_hid_devices() != 0) return -1;
    return 0;
}

// Run the CFRunLoop (blocks forever). Call from a dedicated thread.
void iokit_sensor_run(void) {
    while (1) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, false);
    }
}

// Get pointer to the ring buffer (for Rust to read).
const uint8_t* iokit_ring_ptr(void) {
    return g_ring;
}

// Get ring buffer size.
int iokit_ring_size(void) {
    return RING_SIZE;
}
