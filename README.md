# slap-your-openclaw

A Rust CLI that detects physical slaps and shakes on Apple Silicon MacBooks via the built-in accelerometer and publishes classified events over MQTT to [OpenClaw](https://github.com/hughmadden/openclaw-mqtt).

## How It Works

Apple Silicon MacBooks contain a Bosch BMI286 IMU (Inertial Measurement Unit) accessible through IOKit HID. This tool:

1. **Reads raw accelerometer data** at ~800Hz via a C shim that talks to IOKit HID
2. **Decimates to 100Hz** and removes gravity via a high-pass filter
3. **Runs 4 detection algorithms** in parallel on every sample:
   - **STA/LTA** (Short-Term Average / Long-Term Average) — detects sudden energy changes at 3 timescales
   - **CUSUM** (Cumulative Sum) — detects persistent shifts in the signal mean
   - **Kurtosis** — measures how "spiky" the signal distribution is (impulsive events have high kurtosis)
   - **Peak/MAD** (Median Absolute Deviation) — flags statistical outliers
4. **Classifies events** into 6 severity levels based on how many algorithms fire and the signal amplitude
5. **Distinguishes slaps from shakes** by tracking STA/LTA activation duration (< 100ms = slap, > 200ms = shake)
6. **Publishes events** as JSON to an MQTT topic in OpenClaw's inbound format

## Severity Levels

| Level | Name | Typical Cause |
|-------|------|---------------|
| 1 | MICRO_VIB | Typing, very light table bump |
| 2 | VIB_LEGERE | Setting down a cup, light tap |
| 3 | VIBRATION | Firm tap, moderate bump |
| 4 | MICRO_CHOC | Light slap, strong tap |
| 5 | CHOC_MOYEN | Firm slap, dropping something nearby |
| 6 | CHOC_MAJEUR | Full slap, strong impact |

## Requirements

- **Apple Silicon Mac** (M1/M2/M3/M4) — the accelerometer is not available on Intel Macs
- **Root privileges** — IOKit HID access requires `sudo`
- **MQTT broker** — any MQTT v3.1.1 broker (e.g., Mosquitto)
- **Rust toolchain** — for building from source

## Installation

```bash
git clone <this-repo>
cd slap-your-openclaw
cargo build --release
```

The binary is at `./target/release/slap-your-openclaw`.

## Usage

```bash
# Basic usage (connects to localhost:1883)
sudo ./target/release/slap-your-openclaw

# Custom MQTT broker
sudo ./target/release/slap-your-openclaw --mqtt-host broker.local --mqtt-port 8883

# Lower sensitivity (only report level 1+)
sudo ./target/release/slap-your-openclaw --min-level 1

# Custom topic and cooldown
sudo ./target/release/slap-your-openclaw --mqtt-topic my/topic --cooldown 1000
```

### CLI Options

```
--mqtt-host <HOST>     MQTT broker host [env: MQTT_HOST] [default: localhost]
--mqtt-port <PORT>     MQTT broker port [env: MQTT_PORT] [default: 1883]
--mqtt-topic <TOPIC>   MQTT publish topic [env: MQTT_TOPIC] [default: openclaw/slap/inbound]
--cooldown <MS>        Cooldown between events in milliseconds [env: SLAP_COOLDOWN] [default: 500]
--min-level <1-6>      Minimum severity level to publish [env: SLAP_MIN_LEVEL] [default: 3]
```

All options can also be set via environment variables.

## MQTT Output Format

Events are published as JSON matching the OpenClaw inbound format:

```json
{
  "senderId": "slap-detector",
  "text": "SLAP DETECTED! Level 5 (CHOC_MOYEN) - amplitude: 0.0234g",
  "correlationId": "slap-a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

The `text` field starts with `SLAP`, `SHAKE`, or `UNKNOWN` depending on the detected motion type.

## Testing It Out

1. Start an MQTT broker:
   ```bash
   # If using Mosquitto
   mosquitto
   ```

2. Subscribe to the topic in one terminal:
   ```bash
   mosquitto_sub -t "openclaw/slap/inbound" -v
   ```

3. Run the detector in another terminal:
   ```bash
   sudo ./target/release/slap-your-openclaw --min-level 1
   ```

4. Slap your laptop! You should see events in both terminals.

## Architecture

```
src/
├── main.rs            # CLI entry, tokio main loop, signal handling
├── config.rs          # CLI args + env vars (clap derive)
├── sensor/
│   ├── mod.rs         # Sensor module exports
│   ├── iokit.rs       # Rust FFI bindings to C shim
│   └── iokit.c        # C shim: IOKit HID → ring buffer
├── detector/
│   ├── mod.rs         # 4 detection algorithms + severity classifier
│   └── ring.rs        # Fixed-capacity ring buffer (RingFloat)
└── mqtt.rs            # MQTT publisher (rumqttc + tokio)
```

**Data flow:**
```
IOKit HID callback (C, ~800Hz)
  → ring buffer (shared memory)
    → Rust reader (decimated to 100Hz)
      → high-pass filter (gravity removal)
        → STA/LTA, CUSUM, Kurtosis, Peak/MAD
          → severity classifier + slap/shake discrimination
            → MQTT publish (JSON)
```

## Running Tests

```bash
cargo test           # All 19 unit tests
cargo clippy         # Lint check
cargo fmt --check    # Format check
```

## Credits

Detection algorithms ported from:
- [taigrr/spank](https://github.com/taigrr/spank) (Go implementation)
- [taigrr/apple-silicon-accelerometer](https://github.com/taigrr/apple-silicon-accelerometer) (IOKit HID access)

## License

MIT
