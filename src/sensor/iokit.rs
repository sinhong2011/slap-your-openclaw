use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const RING_CAP: usize = 8000;
const RING_ENTRY: usize = 12;
const RING_HEADER: usize = 16;
const ACCEL_SCALE: f64 = 65536.0;

extern "C" {
    fn iokit_sensor_init() -> i32;
    fn iokit_sensor_run();
    fn iokit_ring_ptr() -> *const u8;
}

/// A 3-axis accelerometer sample in g-force.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Handle to read accelerometer samples from the C ring buffer.
pub struct SensorRing {
    ring_ptr: *const u8,
    last_total: u64,
    running: Arc<AtomicBool>,
}

// SAFETY: The ring buffer pointer is valid for the lifetime of the process
// and is only written by the C callback thread. Reads use memcpy-style
// access with atomic total counter for synchronization.
unsafe impl Send for SensorRing {}
unsafe impl Sync for SensorRing {}

impl SensorRing {
    /// Read new samples since last call. Returns samples scaled to g-force.
    pub fn read_new(&mut self) -> Vec<Sample> {
        let ring = self.ring_ptr;
        if ring.is_null() {
            return Vec::new();
        }

        unsafe {
            // Read total count
            let total = u64::from_le_bytes(
                std::slice::from_raw_parts(ring.add(4), 8)
                    .try_into().unwrap()
            );

            let n_new = (total as i64 - self.last_total as i64).max(0) as usize;
            if n_new == 0 {
                return Vec::new();
            }
            let n_new = n_new.min(RING_CAP);

            let idx = u32::from_le_bytes(
                std::slice::from_raw_parts(ring, 4)
                    .try_into().unwrap()
            ) as usize;

            let start = (idx as isize - n_new as isize).rem_euclid(RING_CAP as isize) as usize;
            let mut samples = Vec::with_capacity(n_new);

            for i in 0..n_new {
                let pos = (start + i) % RING_CAP;
                let off = RING_HEADER + pos * RING_ENTRY;
                let x = i32::from_le_bytes(
                    std::slice::from_raw_parts(ring.add(off), 4).try_into().unwrap()
                );
                let y = i32::from_le_bytes(
                    std::slice::from_raw_parts(ring.add(off + 4), 4).try_into().unwrap()
                );
                let z = i32::from_le_bytes(
                    std::slice::from_raw_parts(ring.add(off + 8), 4).try_into().unwrap()
                );
                samples.push(Sample {
                    x: x as f64 / ACCEL_SCALE,
                    y: y as f64 / ACCEL_SCALE,
                    z: z as f64 / ACCEL_SCALE,
                });
            }

            self.last_total = total;
            samples
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Start the sensor. Spawns a dedicated OS thread for the CFRunLoop.
/// Returns a SensorRing for reading samples.
pub fn start_sensor() -> Result<SensorRing, String> {
    unsafe {
        let ret = iokit_sensor_init();
        if ret != 0 {
            return Err("Failed to initialize IOKit HID sensor. Is this Apple Silicon? Running as root?".into());
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name("iokit-sensor".into())
        .spawn(move || {
            unsafe { iokit_sensor_run(); }
            running_clone.store(false, Ordering::Relaxed);
        })
        .map_err(|e| format!("Failed to spawn sensor thread: {e}"))?;

    let ring_ptr = unsafe { iokit_ring_ptr() };

    Ok(SensorRing {
        ring_ptr,
        last_total: 0,
        running,
    })
}
