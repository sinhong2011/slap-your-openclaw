# slap-your-openclaw Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust CLI that detects physical slaps on Apple Silicon MacBooks via the built-in accelerometer (IOKit HID) and publishes severity-classified events over MQTT to OpenClaw.

**Architecture:** Single monolithic binary. C shim reads the Bosch BMI286 IMU via IOKit HID, writes samples to a lock-free ring buffer. Rust reads samples, runs 4 vibration detection algorithms (STA/LTA, CUSUM, Kurtosis, Peak/MAD), classifies events into 6 severity levels, and publishes JSON to MQTT.

**Tech Stack:** Rust, C (IOKit FFI), clap 4 (CLI), rumqttc (MQTT), tokio (async runtime), serde/serde_json (JSON), chrono (timestamps), cc (C build)

**Design doc:** `docs/plans/2026-02-28-slap-your-openclaw-design.md`

---

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `src/main.rs`
- Create: `CLAUDE.md`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "slap-your-openclaw"
version = "0.1.0"
edition = "2021"
description = "Detect laptop slaps via Apple Silicon accelerometer, publish MQTT events to OpenClaw"
license = "MIT"

[[bin]]
name = "slap-your-openclaw"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive", "env"] }
rumqttc = "0.24"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }

[build-dependencies]
cc = "1"
```

**Step 2: Create build.rs**

```rust
fn main() {
    // Compile the C shim for IOKit HID access
    cc::Build::new()
        .file("src/sensor/iokit.c")
        .flag("-framework")
        .flag("IOKit")
        .flag("-framework")
        .flag("CoreFoundation")
        .compile("iokit_shim");

    // Link macOS frameworks
    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
}
```

**Step 3: Create minimal src/main.rs**

```rust
fn main() {
    println!("slap-your-openclaw: not yet implemented");
}
```

**Step 4: Create placeholder C file so build works**

Create `src/sensor/iokit.c`:

```c
// Placeholder - IOKit HID accelerometer access
// Will be implemented in Task 5
```

**Step 5: Create CLAUDE.md**

```markdown
# slap-your-openclaw

## What This Is

A Rust CLI that detects physical slaps/impacts on Apple Silicon MacBooks via the built-in accelerometer (Bosch BMI286 IMU accessed via IOKit HID) and publishes severity-classified events over MQTT.

## Architecture

```
src/
├── main.rs          # CLI (clap) + main loop + signal handling
├── config.rs        # CLI args + env var config (clap derive)
├── sensor/
│   ├── mod.rs       # Sensor module: start_sensor() → SensorRing
│   ├── iokit.rs     # Rust FFI bindings to C shim
│   └── iokit.c      # C shim: IOKit HID accelerometer via AppleSPUHIDDriver
├── detector/
│   ├── mod.rs       # Vibration detector: 4 algorithms + event classifier
│   └── ring.rs      # Fixed-capacity ring buffer (RingFloat)
└── mqtt.rs          # MQTT publisher using rumqttc
```

## Key Constants

- Sample rate: 100Hz (decimated from ~800Hz raw)
- IMU: Bosch BMI286, report length 22 bytes, data offset 6
- Scale: Q16 fixed-point → g-force (divide by 65536.0)
- Cooldown: 500ms between events
- 6 severity levels: MICRO_VIB → CHOC_MAJEUR

## How to Build & Run

```bash
cargo build --release
sudo ./target/release/slap-your-openclaw --mqtt-host localhost
```

## How to Test

```bash
cargo test           # Unit tests (detector, ring buffer, config)
cargo clippy         # Lint
cargo fmt --check    # Format check
```

## Conventions

- C shim handles all macOS framework calls (IOKit, CoreFoundation)
- Rust FFI in sensor/iokit.rs wraps C functions
- Detector is pure Rust, no unsafe, fully unit-testable with synthetic data
- MQTT payload uses OpenClaw inbound format: { senderId, text, correlationId }
```

**Step 6: Verify project builds**

Run: `cd slap-your-openclaw && cargo build 2>&1`
Expected: Successful compilation (warnings OK at this stage)

**Step 7: Commit**

```bash
git add Cargo.toml build.rs src/main.rs src/sensor/iokit.c CLAUDE.md docs/
git commit -m "feat: scaffold slap-your-openclaw project"
```

---

### Task 2: Ring Buffer

**Files:**
- Create: `src/detector/ring.rs`
- Create: `src/detector/mod.rs` (minimal, just re-exports ring for now)

**Step 1: Write failing tests for RingFloat**

In `src/detector/ring.rs`:

```rust
/// Fixed-capacity ring buffer for f64 values.
pub struct RingFloat {
    data: Vec<f64>,
    pos: usize,
    full: bool,
}

impl RingFloat {
    pub fn new(cap: usize) -> Self {
        Self {
            data: vec![0.0; cap],
            pos: 0,
            full: false,
        }
    }

    pub fn push(&mut self, v: f64) {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn slice(&self) -> Vec<f64> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_push_and_len() {
        let mut r = RingFloat::new(5);
        assert_eq!(r.len(), 0);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn test_ring_slice_not_full() {
        let mut r = RingFloat::new(5);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.slice(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_ring_wraps_around() {
        let mut r = RingFloat::new(3);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.len(), 3);
        r.push(4.0);
        assert_eq!(r.len(), 3);
        // Should return in insertion order: 2.0, 3.0, 4.0
        assert_eq!(r.slice(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_ring_full_cycle() {
        let mut r = RingFloat::new(3);
        for i in 0..10 {
            r.push(i as f64);
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.slice(), vec![7.0, 8.0, 9.0]);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test ring -- --nocapture 2>&1`
Expected: FAIL with "not yet implemented"

**Step 3: Implement RingFloat**

Replace the `todo!()` implementations:

```rust
pub fn push(&mut self, v: f64) {
    self.data[self.pos] = v;
    self.pos += 1;
    if self.pos >= self.data.len() {
        self.pos = 0;
        self.full = true;
    }
}

pub fn len(&self) -> usize {
    if self.full {
        self.data.len()
    } else {
        self.pos
    }
}

pub fn slice(&self) -> Vec<f64> {
    let n = self.len();
    let mut out = Vec::with_capacity(n);
    if self.full {
        out.extend_from_slice(&self.data[self.pos..]);
        out.extend_from_slice(&self.data[..self.pos]);
    } else {
        out.extend_from_slice(&self.data[..self.pos]);
    }
    out
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test ring -- --nocapture 2>&1`
Expected: All 4 tests PASS

**Step 5: Create detector/mod.rs**

```rust
pub mod ring;
```

Wire up in `src/main.rs`:

```rust
mod detector;

fn main() {
    println!("slap-your-openclaw: not yet implemented");
}
```

**Step 6: Commit**

```bash
git add src/detector/
git commit -m "feat: add RingFloat ring buffer with tests"
```

---

### Task 3: Config Module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

**Step 1: Write config.rs with clap derive**

```rust
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "slap-your-openclaw")]
#[command(about = "Detect laptop slaps via Apple Silicon accelerometer, publish MQTT events")]
#[command(version)]
pub struct Config {
    /// MQTT broker host
    #[arg(long, env = "MQTT_HOST", default_value = "localhost")]
    pub mqtt_host: String,

    /// MQTT broker port
    #[arg(long, env = "MQTT_PORT", default_value_t = 1883)]
    pub mqtt_port: u16,

    /// MQTT publish topic
    #[arg(long, env = "MQTT_TOPIC", default_value = "openclaw/slap/inbound")]
    pub mqtt_topic: String,

    /// Cooldown between events in milliseconds
    #[arg(long, env = "SLAP_COOLDOWN", default_value_t = 500)]
    pub cooldown_ms: u64,

    /// Minimum severity level to publish (1-6)
    #[arg(long, env = "SLAP_MIN_LEVEL", default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=6))]
    pub min_level: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::try_parse_from(["slap-your-openclaw"]).unwrap();
        assert_eq!(config.mqtt_host, "localhost");
        assert_eq!(config.mqtt_port, 1883);
        assert_eq!(config.mqtt_topic, "openclaw/slap/inbound");
        assert_eq!(config.cooldown_ms, 500);
        assert_eq!(config.min_level, 3);
    }

    #[test]
    fn test_custom_config() {
        let config = Config::try_parse_from([
            "slap-your-openclaw",
            "--mqtt-host", "broker.local",
            "--mqtt-port", "8883",
            "--mqtt-topic", "custom/topic",
            "--cooldown", "1000",
            "--min-level", "5",
        ]).unwrap();
        assert_eq!(config.mqtt_host, "broker.local");
        assert_eq!(config.mqtt_port, 8883);
        assert_eq!(config.mqtt_topic, "custom/topic");
        assert_eq!(config.cooldown_ms, 1000);
        assert_eq!(config.min_level, 5);
    }

    #[test]
    fn test_invalid_min_level() {
        let result = Config::try_parse_from([
            "slap-your-openclaw",
            "--min-level", "7",
        ]);
        assert!(result.is_err());
    }
}
```

**Step 2: Wire into main.rs**

```rust
mod config;
mod detector;

use clap::Parser;
use config::Config;

fn main() {
    let config = Config::parse();
    println!("slap-your-openclaw: config={config:?}");
}
```

**Step 3: Run tests**

Run: `cargo test config -- --nocapture 2>&1`
Expected: All 3 tests PASS

**Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add CLI config with clap derive + env var support"
```

---

### Task 4: Vibration Detector

This is the largest task — port all 4 detection algorithms from the Go reference.

**Files:**
- Modify: `src/detector/mod.rs` (full detector implementation)
- The ring buffer from Task 2 is used here

**Step 1: Write detector struct and tests for high-pass filter**

In `src/detector/mod.rs`:

```rust
pub mod ring;

use ring::RingFloat;

/// Severity levels for detected events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    MicroVib = 1,
    VibLegere = 2,
    Vibration = 3,
    MicroChoc = 4,
    ChocMoyen = 5,
    ChocMajeur = 6,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MicroVib => "MICRO_VIB",
            Self::VibLegere => "VIB_LEGERE",
            Self::Vibration => "VIBRATION",
            Self::MicroChoc => "MICRO_CHOC",
            Self::ChocMoyen => "CHOC_MOYEN",
            Self::ChocMajeur => "CHOC_MAJEUR",
        }
    }

    pub fn level(&self) -> u8 {
        *self as u8
    }
}

/// A detected vibration/impact event.
#[derive(Debug, Clone)]
pub struct Event {
    pub severity: Severity,
    pub amplitude: f64,
    pub sources: Vec<String>,
}

/// Sample rate in Hz (after decimation).
const FS: usize = 100;

/// High-pass filter alpha for gravity removal.
const HP_ALPHA: f64 = 0.95;

/// Vibration detector using STA/LTA, CUSUM, Kurtosis, and Peak/MAD.
pub struct Detector {
    sample_count: usize,

    // High-pass filter state
    hp_ready: bool,
    hp_prev_raw: [f64; 3],
    hp_prev_out: [f64; 3],

    // STA/LTA (3 timescales)
    sta: [f64; 3],
    lta: [f64; 3],
    sta_n: [usize; 3],
    lta_n: [usize; 3],
    sta_lta_on: [f64; 3],
    sta_lta_off: [f64; 3],
    sta_lta_active: [bool; 3],

    // CUSUM
    cusum_pos: f64,
    cusum_neg: f64,
    cusum_mu: f64,
    cusum_k: f64,
    cusum_h: f64,

    // Kurtosis
    kurt_buf: RingFloat,
    kurt_dec: usize,

    // Peak / MAD
    peak_buf: RingFloat,

    // Event tracking
    events: Vec<Event>,
    last_evt_t: f64,
}

impl Detector {
    pub fn new() -> Self {
        Self {
            sample_count: 0,
            hp_ready: false,
            hp_prev_raw: [0.0; 3],
            hp_prev_out: [0.0; 3],
            sta: [0.0; 3],
            lta: [1e-10, 1e-10, 1e-10],
            sta_n: [3, 15, 50],
            lta_n: [100, 500, 2000],
            sta_lta_on: [3.0, 2.5, 2.0],
            sta_lta_off: [1.5, 1.3, 1.2],
            sta_lta_active: [false; 3],
            cusum_pos: 0.0,
            cusum_neg: 0.0,
            cusum_mu: 0.0,
            cusum_k: 0.0005,
            cusum_h: 0.01,
            kurt_buf: RingFloat::new(100),
            kurt_dec: 0,
            peak_buf: RingFloat::new(200),
            events: Vec::new(),
            last_evt_t: 0.0,
        }
    }

    /// Process one accelerometer sample (in g). Returns dynamic magnitude.
    pub fn process(&mut self, ax: f64, ay: f64, az: f64, t_now: f64) -> f64 {
        self.sample_count += 1;

        // High-pass filter: first sample just initializes
        if !self.hp_ready {
            self.hp_prev_raw = [ax, ay, az];
            self.hp_ready = true;
            return 0.0;
        }

        let a = HP_ALPHA;
        let hx = a * (self.hp_prev_out[0] + ax - self.hp_prev_raw[0]);
        let hy = a * (self.hp_prev_out[1] + ay - self.hp_prev_raw[1]);
        let hz = a * (self.hp_prev_out[2] + az - self.hp_prev_raw[2]);
        self.hp_prev_raw = [ax, ay, az];
        self.hp_prev_out = [hx, hy, hz];
        let mag = (hx * hx + hy * hy + hz * hz).sqrt();

        // Run detection algorithms
        let mut detections: Vec<&str> = Vec::new();

        // STA/LTA
        let e = mag * mag;
        for i in 0..3 {
            self.sta[i] += (e - self.sta[i]) / self.sta_n[i] as f64;
            self.lta[i] += (e - self.lta[i]) / self.lta_n[i] as f64;
            let ratio = self.sta[i] / (self.lta[i] + 1e-30);
            let was = self.sta_lta_active[i];
            if ratio > self.sta_lta_on[i] && !was {
                self.sta_lta_active[i] = true;
                detections.push("STA/LTA");
            } else if ratio < self.sta_lta_off[i] {
                self.sta_lta_active[i] = false;
            }
        }

        // CUSUM
        self.cusum_mu += 0.0001 * (mag - self.cusum_mu);
        self.cusum_pos = f64::max(0.0, self.cusum_pos + mag - self.cusum_mu - self.cusum_k);
        self.cusum_neg = f64::max(0.0, self.cusum_neg - mag + self.cusum_mu - self.cusum_k);
        if self.cusum_pos > self.cusum_h {
            detections.push("CUSUM");
            self.cusum_pos = 0.0;
        }
        if self.cusum_neg > self.cusum_h {
            if !detections.contains(&"CUSUM") {
                detections.push("CUSUM");
            }
            self.cusum_neg = 0.0;
        }

        // Kurtosis
        self.kurt_buf.push(mag);
        self.kurt_dec += 1;
        if self.kurt_dec >= 10 && self.kurt_buf.len() >= 50 {
            self.kurt_dec = 0;
            let buf = self.kurt_buf.slice();
            let n = buf.len() as f64;
            let mu: f64 = buf.iter().sum::<f64>() / n;
            let mut m2 = 0.0_f64;
            let mut m4 = 0.0_f64;
            for &v in &buf {
                let diff = v - mu;
                let d2 = diff * diff;
                m2 += d2;
                m4 += d2 * d2;
            }
            m2 /= n;
            m4 /= n;
            let kurtosis = m4 / (m2 * m2 + 1e-30);
            if kurtosis > 6.0 {
                detections.push("KURTOSIS");
            }
        }

        // Peak / MAD
        self.peak_buf.push(mag);
        if self.peak_buf.len() >= 50 && self.sample_count % 10 == 0 {
            let buf = self.peak_buf.slice();
            let mut sorted = buf.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = sorted.len();
            let median = sorted[n / 2];

            let mut devs: Vec<f64> = sorted.iter().map(|v| (v - median).abs()).collect();
            devs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mad = devs[n / 2];
            let sigma = 1.4826 * mad + 1e-30;

            let dev = (mag - median).abs() / sigma;
            if dev > 2.0 {
                detections.push("PEAK");
            }
        }

        // Classify if any detections and enough time since last event
        if !detections.is_empty() && (t_now - self.last_evt_t) > 0.01 {
            self.last_evt_t = t_now;
            self.classify(&detections, mag);
        }

        mag
    }

    fn classify(&mut self, sources: &[&str], amp: f64) {
        // Deduplicate sources
        let mut unique: Vec<String> = Vec::new();
        for &s in sources {
            let s = s.to_string();
            if !unique.contains(&s) {
                unique.push(s);
            }
        }
        let ns = unique.len();

        let severity = if ns >= 4 && amp > 0.05 {
            Severity::ChocMajeur
        } else if ns >= 3 && amp > 0.02 {
            Severity::ChocMoyen
        } else if unique.contains(&"PEAK".to_string()) && amp > 0.005 {
            Severity::MicroChoc
        } else if (unique.contains(&"STA/LTA".to_string()) || unique.contains(&"CUSUM".to_string())) && amp > 0.003 {
            Severity::Vibration
        } else if amp > 0.001 {
            Severity::VibLegere
        } else {
            Severity::MicroVib
        };

        let event = Event {
            severity,
            amplitude: amp,
            sources: unique,
        };

        self.events.push(event);
        if self.events.len() > 500 {
            self.events.drain(..self.events.len() - 500);
        }
    }

    /// Take the latest event if any new events exist since last call.
    pub fn take_latest_event(&mut self) -> Option<Event> {
        self.events.pop()
    }

    /// Drain all events since last call.
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed a constant gravity signal (simulating a stationary laptop).
    fn feed_gravity(det: &mut Detector, n: usize) {
        for i in 0..n {
            let t = i as f64 / FS as f64;
            det.process(0.0, 0.0, -1.0, t);
        }
    }

    #[test]
    fn test_no_events_on_stationary() {
        let mut det = Detector::new();
        // Feed 5 seconds of perfectly still accelerometer data
        feed_gravity(&mut det, FS * 5);
        let events = det.drain_events();
        // Should have very few or no significant events
        let significant: Vec<_> = events.iter()
            .filter(|e| e.severity >= Severity::Vibration)
            .collect();
        assert!(significant.is_empty(), "No significant events expected on stationary data, got {}", significant.len());
    }

    #[test]
    fn test_spike_triggers_event() {
        let mut det = Detector::new();
        // Warm up with 2 seconds of gravity
        feed_gravity(&mut det, FS * 2);
        det.drain_events(); // clear any warmup events

        // Inject a sharp spike (simulating a slap)
        let t_base = 2.0;
        det.process(0.5, 0.3, -0.8, t_base);
        det.process(0.8, 0.5, -0.5, t_base + 0.01);
        det.process(1.0, 0.7, -0.3, t_base + 0.02);
        det.process(0.3, 0.2, -0.9, t_base + 0.03);

        // Resume normal gravity
        for i in 0..50 {
            let t = t_base + 0.04 + i as f64 / FS as f64;
            det.process(0.0, 0.0, -1.0, t);
        }

        let events = det.drain_events();
        assert!(!events.is_empty(), "Should detect at least one event from spike");
    }

    #[test]
    fn test_major_impact_classification() {
        let mut det = Detector::new();
        // Warm up
        feed_gravity(&mut det, FS * 3);
        det.drain_events();

        // Inject a very large spike (major impact)
        let t_base = 3.0;
        for i in 0..10 {
            let t = t_base + i as f64 / FS as f64;
            let intensity = 2.0; // 2g spike
            det.process(intensity, intensity * 0.8, -0.2, t);
        }

        // Return to normal
        for i in 0..100 {
            let t = t_base + 0.1 + i as f64 / FS as f64;
            det.process(0.0, 0.0, -1.0, t);
        }

        let events = det.drain_events();
        let max_severity = events.iter()
            .map(|e| e.severity)
            .max();
        assert!(max_severity.is_some(), "Should detect events from major impact");
        // A 2g spike should trigger multiple detectors → high severity
        assert!(
            max_severity.unwrap() >= Severity::MicroChoc,
            "Major impact should be at least MICRO_CHOC, got {:?}",
            max_severity.unwrap()
        );
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::ChocMajeur > Severity::ChocMoyen);
        assert!(Severity::ChocMoyen > Severity::MicroChoc);
        assert!(Severity::MicroChoc > Severity::Vibration);
        assert!(Severity::Vibration > Severity::VibLegere);
        assert!(Severity::VibLegere > Severity::MicroVib);
    }

    #[test]
    fn test_severity_level_numbers() {
        assert_eq!(Severity::MicroVib.level(), 1);
        assert_eq!(Severity::VibLegere.level(), 2);
        assert_eq!(Severity::Vibration.level(), 3);
        assert_eq!(Severity::MicroChoc.level(), 4);
        assert_eq!(Severity::ChocMoyen.level(), 5);
        assert_eq!(Severity::ChocMajeur.level(), 6);
    }

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::ChocMajeur.as_str(), "CHOC_MAJEUR");
        assert_eq!(Severity::MicroVib.as_str(), "MICRO_VIB");
    }
}
```

**Step 2: Run tests to verify they pass**

Run: `cargo test detector -- --nocapture 2>&1`
Expected: All 6 tests PASS

**Step 3: Commit**

```bash
git add src/detector/mod.rs
git commit -m "feat: add vibration detector with STA/LTA, CUSUM, Kurtosis, Peak/MAD"
```

---

### Task 5: C Shim for IOKit HID Accelerometer

**Files:**
- Modify: `src/sensor/iokit.c` (replace placeholder)
- Create: `src/sensor/iokit.rs`
- Create: `src/sensor/mod.rs`

**Step 1: Write the C shim**

Replace `src/sensor/iokit.c` with:

```c
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
    CFTypeRef ref = IORegistryEntryCreateCFProperty(service, key, kCFAllocatorDefault, 0);
    if (ref) {
        CFNumberGetValue(ref, kCFNumberSInt64Type, &val);
        CFRelease(ref);
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
```

**Step 2: Write Rust FFI bindings (src/sensor/iokit.rs)**

```rust
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
```

**Step 3: Write sensor/mod.rs**

```rust
pub mod iokit;

pub use iokit::{start_sensor, Sample, SensorRing};
```

**Step 4: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Successful compilation (may have unused warnings)

**Step 5: Commit**

```bash
git add src/sensor/ build.rs
git commit -m "feat: add IOKit HID accelerometer sensor via C shim"
```

---

### Task 6: MQTT Publisher

**Files:**
- Create: `src/mqtt.rs`

**Step 1: Write MQTT publisher**

```rust
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::detector::{self, Severity};

/// MQTT event payload matching OpenClaw inbound format.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlapPayload {
    pub sender_id: String,
    pub text: String,
    pub correlation_id: String,
}

impl SlapPayload {
    pub fn from_event(event: &detector::Event) -> Self {
        let text = format!(
            "SLAP DETECTED! Level {} ({}) - amplitude: {:.4}g",
            event.severity.level(),
            event.severity.as_str(),
            event.amplitude,
        );
        Self {
            sender_id: "slap-detector".into(),
            text,
            correlation_id: format!("slap-{}", uuid::Uuid::new_v4()),
        }
    }
}

/// MQTT publisher handle.
pub struct Publisher {
    tx: mpsc::UnboundedSender<String>,
}

impl Publisher {
    /// Connect to MQTT broker and return a Publisher handle.
    /// Spawns a background tokio task for the MQTT event loop.
    pub fn connect(
        host: &str,
        port: u16,
        topic: String,
    ) -> Result<Self, String> {
        let mut opts = MqttOptions::new("slap-your-openclaw", host, port);
        opts.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, mut eventloop) = AsyncClient::new(opts, 64);
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        let topic_clone = topic.clone();

        // Spawn MQTT event loop
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        eprintln!("mqtt: connected to broker");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("mqtt: connection error: {e}, reconnecting...");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        });

        // Spawn publish worker
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                if let Err(e) = client
                    .publish(&topic_clone, QoS::AtLeastOnce, false, payload.as_bytes())
                    .await
                {
                    eprintln!("mqtt: publish error: {e}");
                }
            }
        });

        Ok(Self { tx })
    }

    /// Publish a slap event.
    pub fn publish(&self, event: &detector::Event) -> Result<(), String> {
        let payload = SlapPayload::from_event(event);
        let json = serde_json::to_string(&payload)
            .map_err(|e| format!("JSON serialize error: {e}"))?;
        self.tx
            .send(json)
            .map_err(|e| format!("MQTT channel send error: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector;

    #[test]
    fn test_slap_payload_format() {
        let event = detector::Event {
            severity: Severity::ChocMoyen,
            amplitude: 0.0234,
            sources: vec!["STA/LTA".into(), "CUSUM".into(), "PEAK".into()],
        };
        let payload = SlapPayload::from_event(&event);
        assert_eq!(payload.sender_id, "slap-detector");
        assert!(payload.text.contains("Level 5"));
        assert!(payload.text.contains("CHOC_MOYEN"));
        assert!(payload.text.contains("0.0234g"));
        assert!(payload.correlation_id.starts_with("slap-"));
    }

    #[test]
    fn test_slap_payload_json() {
        let event = detector::Event {
            severity: Severity::MicroChoc,
            amplitude: 0.008,
            sources: vec!["PEAK".into()],
        };
        let payload = SlapPayload::from_event(&event);
        let json = serde_json::to_string(&payload).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["senderId"], "slap-detector");
        assert!(parsed["text"].as_str().unwrap().contains("Level 4"));
        assert!(parsed["correlationId"].as_str().unwrap().starts_with("slap-"));
    }
}
```

**Step 2: Run tests**

Run: `cargo test mqtt -- --nocapture 2>&1`
Expected: 2 tests PASS

**Step 3: Commit**

```bash
git add src/mqtt.rs
git commit -m "feat: add MQTT publisher with OpenClaw inbound format"
```

---

### Task 7: Main Loop

**Files:**
- Modify: `src/main.rs`

**Step 1: Wire everything together**

```rust
mod config;
mod detector;
mod mqtt;
mod sensor;

use std::time::{Duration, Instant};

use clap::Parser;
use config::Config;
use detector::Detector;

#[tokio::main]
async fn main() {
    let config = Config::parse();

    // Check root privileges
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("error: slap-your-openclaw requires root privileges for accelerometer access");
        eprintln!("run with: sudo slap-your-openclaw");
        std::process::exit(1);
    }

    eprintln!(
        "slap-your-openclaw: starting (mqtt={}:{}, topic={}, min_level={}, cooldown={}ms)",
        config.mqtt_host, config.mqtt_port, config.mqtt_topic, config.min_level, config.cooldown_ms
    );

    // Start accelerometer sensor
    let mut ring = match sensor::start_sensor() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Wait for sensor warmup
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect MQTT
    let publisher = match mqtt::Publisher::connect(
        &config.mqtt_host,
        config.mqtt_port,
        config.mqtt_topic.clone(),
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("slap-your-openclaw: listening for slaps... (ctrl+c to quit)");

    // Main detection loop
    let mut det = Detector::new();
    let mut last_publish = Instant::now();
    let cooldown = Duration::from_millis(config.cooldown_ms);

    let mut interval = tokio::time::interval(Duration::from_millis(10));
    loop {
        interval.tick().await;

        if !ring.is_running() {
            eprintln!("error: sensor thread stopped unexpectedly");
            std::process::exit(1);
        }

        let samples = ring.read_new();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        let n_samples = samples.len();
        for (idx, s) in samples.iter().enumerate() {
            let t = now_secs - (n_samples - idx - 1) as f64 / 100.0;
            det.process(s.x, s.y, s.z, t);
        }

        for event in det.drain_events() {
            if event.severity.level() < config.min_level {
                continue;
            }

            if last_publish.elapsed() < cooldown {
                continue;
            }

            eprintln!(
                "slap #{} [{}  amp={:.5}g] sources={:?}",
                event.severity.level(),
                event.severity.as_str(),
                event.amplitude,
                event.sources,
            );

            if let Err(e) = publisher.publish(&event) {
                eprintln!("mqtt publish error: {e}");
            }
            last_publish = Instant::now();
        }
    }
}
```

**Step 2: Add libc dependency to Cargo.toml**

Add to `[dependencies]` in `Cargo.toml`:

```toml
libc = "0.2"
```

**Step 3: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Successful compilation

**Step 4: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass (ring buffer, config, detector, mqtt)

**Step 5: Commit**

```bash
git add src/main.rs Cargo.toml
git commit -m "feat: wire main loop - sensor → detector → MQTT publisher"
```

---

### Task 8: Clippy + Format + Final Polish

**Files:**
- Various, fixing any clippy/fmt warnings

**Step 1: Run clippy**

Run: `cargo clippy -- -W clippy::all 2>&1`
Expected: Fix any warnings

**Step 2: Run fmt**

Run: `cargo fmt 2>&1`

**Step 3: Run all tests one final time**

Run: `cargo test 2>&1`
Expected: All pass

**Step 4: Commit**

```bash
git add -A
git commit -m "chore: clippy + fmt cleanup"
```

---

### Task 9: Manual Integration Test

**Step 1: Build release binary**

Run: `cargo build --release 2>&1`

**Step 2: Test with local MQTT broker**

If you have mosquitto running:

```bash
# Terminal 1: Subscribe to see events
mosquitto_sub -t "openclaw/slap/inbound" -v

# Terminal 2: Run the detector
sudo ./target/release/slap-your-openclaw --mqtt-host localhost --min-level 1

# Terminal 3: Slap your laptop! Check Terminal 1 for JSON events.
```

**Step 3: Verify JSON format**

Expected output on subscriber:

```
openclaw/slap/inbound {"senderId":"slap-detector","text":"SLAP DETECTED! Level 5 (CHOC_MOYEN) - amplitude: 0.0234g","correlationId":"slap-a1b2c3d4-..."}
```

**Step 4: Final commit if any fixes needed**

```bash
git add -A
git commit -m "fix: integration test fixes"
```
