# slap-your-openclaw

## What This Is

A Rust CLI that detects physical slaps/impacts on Apple Silicon MacBooks via the built-in accelerometer (Bosch BMI286 IMU accessed via IOKit HID). Runs in two modes: standalone (invokes OpenClaw agent CLI) or MCP server (exposes tools over stdio for AI agent integration).

## Architecture

```
slap-your-openclaw           # standalone mode (default, backwards-compatible)
slap-your-openclaw mcp       # MCP server mode (stdio)
```

Both modes share the same sensor thread + detection loop:

```
Sensor Thread (OS)           Tokio Runtime
─────────────────           ─────────────────────────────
IOKit HID → ring buf   →   Detection Loop Task
                                ↓ updates
                            Arc<SharedState>
                                ↓ broadcast
                        ┌───────┴──────────┐
                   Standalone          MCP Server
                   (openclaw CLI)      (rmcp stdio)
```

```
src/
├── main.rs          # CLI (clap) + mode dispatch
├── config.rs        # Cli struct + Command subcommands + StandaloneArgs
├── shared.rs        # SharedState, DetectorConfig, run_detection_loop()
├── openclaw.rs      # OpenClaw agent CLI publisher (standalone mode)
├── sensor/
│   ├── mod.rs       # Sensor module: start_sensor() → SensorRing
│   ├── iokit.rs     # Rust FFI bindings to C shim
│   └── iokit.c      # C shim: IOKit HID accelerometer via AppleSPUHIDDriver
├── detector/
│   ├── mod.rs       # Vibration detector: 4 algorithms + event classifier
│   └── ring.rs      # Fixed-capacity ring buffer (RingFloat)
└── mcp/
    ├── mod.rs       # MCP module declaration
    └── server.rs    # SlapServer: 5 MCP tools via rmcp
skill/
└── SKILL.md         # Agent skill for response personality
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `slap_status` | Detector phase, samples processed, sensor health, uptime |
| `slap_get_events` | Recent event history (filterable by limit, min_level) |
| `slap_wait_for_event` | Block until event occurs or timeout |
| `slap_get_config` | Current runtime configuration |
| `slap_set_config` | Update config at runtime (cooldown, thresholds) |

## Key Constants

- Sample rate: 100Hz (decimated from ~800Hz raw)
- IMU: Bosch BMI286, report length 22 bytes, data offset 6
- Scale: Q16 fixed-point → g-force (divide by 65536.0)
- Cooldown: 500ms between events
- 6 severity levels: MICRO_VIB → CHOC_MAJEUR

## How to Build & Run

```bash
cargo build --release

# Standalone mode (default)
sudo ./target/release/slap-your-openclaw
sudo ./target/release/slap-your-openclaw standalone --local    # stdout JSON output
sudo ./target/release/slap-your-openclaw --min-level 3         # more sensitive

# MCP server mode
sudo ./target/release/slap-your-openclaw mcp
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
- MCP server uses rmcp crate with derive macros (same pattern as miniflux/mcp)
- Shared detector args (cooldown, min_level, amplitudes) on top-level Cli; mode-specific args on subcommand
