# slap-your-openclaw Design

## Overview

A Rust CLI that detects physical slaps on an Apple Silicon MacBook using the built-in accelerometer (Bosch BMI286 IMU via IOKit HID), classifies impact severity into 6 levels, and publishes events via MQTT to OpenClaw for Discord notification.

## Architecture

```
MacBook Accelerometer (BMI286 IMU)
    │
    ▼
IOKit HID (C shim, requires sudo)
    │
    ▼
Ring Buffer (lock-free, C writes → Rust reads)
    │
    ▼
Vibration Detector (4 algorithms: STA/LTA, CUSUM, Kurtosis, Peak/MAD)
    │
    ▼
Event Classifier (6 severity levels)
    │
    ▼
MQTT Publisher → openclaw/slap/inbound
    │
    ▼
OpenClaw (openclaw-mqtt plugin) → Discord notification
```

Single monolithic binary. No daemon/client split.

## Project Structure

```
slap-your-openclaw/
├── Cargo.toml
├── build.rs                    # Links IOKit.framework, CoreFoundation.framework
├── CLAUDE.md
├── docs/plans/
│   └── 2026-02-28-slap-your-openclaw-design.md
└── src/
    ├── main.rs                 # CLI (clap), startup, main loop
    ├── config.rs               # CLI args + env var config
    ├── sensor/
    │   ├── mod.rs              # Sensor module exports
    │   ├── iokit.rs            # Rust FFI bindings to C shim
    │   └── iokit.c             # C shim: IOKit HID accelerometer access
    ├── detector/
    │   ├── mod.rs              # Vibration detector (4 algorithms + classifier)
    │   └── ring.rs             # Ring buffer for sample history
    └── mqtt.rs                 # MQTT publisher
```

## Sensor Layer

### C Shim (`sensor/iokit.c`)

Handles all macOS framework calls:

1. `dlopen` IOKit.framework + CoreFoundation.framework
2. Find `AppleSPUHIDDriver` services, wake sensors:
   - Set `SensorPropertyReportingState` = 1
   - Set `SensorPropertyPowerState` = 1
   - Set `ReportInterval` = 10000 (microseconds, ~100Hz)
3. Find `AppleSPUHIDDevice` with vendor usage page + accelerometer usage
4. Create HID device, open it, register input report callback
5. Parse BMI286 reports: extract 3x int32 XYZ at data offset, write to ring buffer
6. Run `CFRunLoop` to receive callbacks

### Rust FFI (`sensor/iokit.rs`)

- `extern "C"` declarations for C shim functions
- `start_sensor() -> Result<Arc<SensorRing>>` — spawns OS thread, returns shared ring
- `SensorRing` — lock-free SPSC ring buffer using atomics (C writes, Rust reads)
- Samples scaled from Q16 int32 to f64 g-force (divide by 65536.0)

### Constants (from reference)

- IMU report length: 64 bytes
- IMU data offset: 4 bytes into report
- Decimation factor: 4 (400Hz raw → 100Hz effective)
- Report buffer size: 1024 bytes

## Detector Layer

Port of `taigrr/apple-silicon-accelerometer` detector, operating at 100Hz.

### High-Pass Filter

IIR filter (alpha=0.95) removes gravity component from raw accelerometer data:
```
hp_out[i] = alpha * (hp_prev_out[i] + raw[i] - hp_prev_raw[i])
```
Then compute dynamic magnitude: `mag = sqrt(hx² + hy² + hz²)`

### Detection Algorithms

All 4 run on every sample:

1. **STA/LTA** — Short-Term Average / Long-Term Average at 3 timescales
   - Windows: (3/100), (15/500), (50/2000) samples
   - On thresholds: 3.0, 2.5, 2.0
   - Off thresholds: 1.5, 1.3, 1.2
   - Triggers when ratio exceeds on-threshold

2. **CUSUM** — Cumulative Sum drift detection
   - K = 0.0005, H = 0.01
   - Triggers when cumulative positive or negative drift exceeds H

3. **Kurtosis** — Statistical impulsiveness over 100-sample window
   - Triggers when kurtosis > 6.0 (normal distribution = 3.0)

4. **Peak/MAD** — Median Absolute Deviation over 200-sample window
   - Triggers when deviation from median > 2.0 sigma (MAD-based)

### Event Classification

Based on number of triggered detectors + amplitude:

| Level | Severity     | Condition                     |
|-------|-------------|-------------------------------|
| 6     | CHOC_MAJEUR | 4+ detectors AND amp > 0.05g |
| 5     | CHOC_MOYEN  | 3+ detectors AND amp > 0.02g |
| 4     | MICRO_CHOC  | PEAK detector AND amp > 0.005g |
| 3     | VIBRATION   | STA/LTA or CUSUM AND amp > 0.003g |
| 2     | VIB_LEGERE  | any detector AND amp > 0.001g |
| 1     | MICRO_VIB   | any detector AND amp > 0.001g |

### Cooldown

500ms minimum between events to prevent rapid-fire triggers.

## MQTT Layer

### Client

- Crate: `rumqttc` (async MQTT v5)
- Auto-reconnect on broker disconnect
- QoS 1 (at-least-once delivery)

### Topic & Payload

Topic: `openclaw/slap/inbound` (configurable via CLI)

```json
{
  "senderId": "slap-detector",
  "text": "SLAP DETECTED! Level 5 (CHOC_MOYEN) - amplitude: 0.0234g",
  "correlationId": "slap-1709089500123"
}
```

The payload uses OpenClaw's expected inbound JSON format:
- `senderId`: Fixed as `"slap-detector"` so OpenClaw tracks conversation per device
- `text`: Human-readable slap description for the agent to process
- `correlationId`: Unique per event for reply matching

### OpenClaw Agent Prompt

The OpenClaw agent should have a system prompt or skill that interprets slap events and sends Discord notifications with appropriate reactions based on severity:

| Level | Discord Message |
|-------|----------------|
| 1-2   | Light touch detected — barely felt that |
| 3     | Vibration detected — something's shaking |
| 4     | Micro-slap! Hey, watch it! |
| 5     | Solid slap! Ouch, that hurt! |
| 6     | MAJOR HIT! Someone just smacked me hard! |

## CLI Configuration

```
slap-your-openclaw [OPTIONS]

Options:
  --mqtt-host <HOST>    MQTT broker host [env: MQTT_HOST] [default: localhost]
  --mqtt-port <PORT>    MQTT broker port [env: MQTT_PORT] [default: 1883]
  --mqtt-topic <TOPIC>  MQTT publish topic [env: MQTT_TOPIC] [default: openclaw/slap/inbound]
  --cooldown <MS>       Cooldown between events in ms [env: SLAP_COOLDOWN] [default: 500]
  --min-level <LEVEL>   Minimum severity level to publish (1-6) [env: SLAP_MIN_LEVEL] [default: 3]
```

`--min-level` filters noise: default 3 ignores micro-vibrations, only reports actual impacts.

## Main Loop

```
1. Parse CLI args → Config
2. Check euid == 0 (exit with message if not sudo)
3. Start sensor thread (C shim → IOKit HID → ring buffer)
4. Wait 100ms for sensor warmup
5. Connect MQTT client (rumqttc async)
6. Create detector instance
7. Tick every 10ms:
   a. Read new samples from ring buffer
   b. Feed each sample to detector.process()
   c. On new event: check cooldown, check min-level
   d. If passes: format JSON, MQTT publish
8. SIGINT/SIGTERM → clean shutdown
```

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4.x | CLI args + env vars |
| `rumqttc` | 0.24+ | Async MQTT client |
| `serde` + `serde_json` | 1.x | JSON serialization |
| `chrono` | 0.4 | UTC timestamps |
| `tokio` | 1.x | Async runtime (MQTT) |
| `cc` | 1.x | Build-time C compilation |

## Constraints

- macOS Apple Silicon only (M1 Pro or later) — uses AppleSPUHIDDriver
- Requires sudo for IOKit HID accelerometer access
- No audio playback — MQTT events only
- Single sensor instance per process
