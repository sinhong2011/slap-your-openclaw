mod config;
mod detector;
mod mcp;
mod openclaw;
mod sensor;
mod shared;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use config::{Cli, Command, StandaloneArgs};
use shared::{DetectorConfig, SharedState};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Check root privileges
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("error: slap-your-openclaw requires root privileges for accelerometer access");
        eprintln!("run with: sudo slap-your-openclaw");
        std::process::exit(1);
    }

    // Resolve standalone args (for startup logging when in standalone mode)
    let standalone_args = match &cli.command {
        Some(Command::Standalone(args)) => Some(args.clone()),
        None => Some(StandaloneArgs::default()),
        Some(Command::Mcp) => None,
    };

    if let Some(ref args) = standalone_args {
        eprintln!(
            "slap-your-openclaw: starting standalone (local={}, min_level={}, cooldown={}ms, min_slap_amp={:.4}g, min_shake_amp={:.4}g)",
            args.local,
            cli.min_level,
            cli.cooldown_ms,
            cli.min_slap_amp,
            cli.min_shake_amp
        );
    } else {
        eprintln!(
            "slap-your-openclaw: starting MCP server (min_level={}, cooldown={}ms)",
            cli.min_level,
            cli.cooldown_ms
        );
    }

    // Start accelerometer sensor
    let ring = match sensor::start_sensor() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Wait for sensor warmup
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create shared state
    let state = Arc::new(SharedState::new(DetectorConfig {
        cooldown_ms: cli.cooldown_ms,
        min_level: cli.min_level,
        min_slap_amp: cli.min_slap_amp,
        min_shake_amp: cli.min_shake_amp,
    }));

    // Spawn detection loop
    let detection_state = state.clone();
    tokio::spawn(async move {
        shared::run_detection_loop(ring, &detection_state).await;
    });

    // Dispatch to mode
    match cli.command {
        Some(Command::Mcp) => {
            if let Err(e) = mcp::server::run(state).await {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            let args = standalone_args.unwrap();
            run_standalone(args, state).await;
        }
    }
}

async fn run_standalone(args: StandaloneArgs, state: Arc<SharedState>) {
    enum OutputMode {
        Local,
        OpenClaw(openclaw::Publisher),
    }

    let mode = if args.local {
        OutputMode::Local
    } else {
        match openclaw::Publisher::connect(&args) {
            Ok(p) => OutputMode::OpenClaw(p),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    };

    let mode_str = match &mode {
        OutputMode::Local => "local mode (stdout)",
        OutputMode::OpenClaw(_) => "openclaw-agent",
    };
    eprintln!("slap-your-openclaw: listening for slaps... (ctrl+c to quit) [{mode_str}]");

    // Subscribe to events from the shared detection loop
    let mut rx = state.event_tx.subscribe();

    loop {
        match rx.recv().await {
            Ok(ts_event) => {
                let event = &ts_event.event;
                match &mode {
                    OutputMode::OpenClaw(publisher) => {
                        if let Err(e) = publisher.publish(event) {
                            eprintln!("openclaw publish error: {e}");
                        }
                    }
                    OutputMode::Local => {
                        println!(
                            "{{\"senderId\":\"slap\",\"text\":\"{} #{} {}\",\"correlationId\":\"\"}}",
                            event.kind.as_str(),
                            event.severity.level(),
                            event.severity.as_str()
                        );
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("warning: dropped {n} events (consumer too slow)");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                eprintln!("error: detection loop stopped");
                std::process::exit(1);
            }
        }
    }
}
