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
