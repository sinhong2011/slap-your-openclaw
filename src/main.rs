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
                "{} #{} [{}  amp={:.5}g] sources={:?}",
                event.kind.as_str(),
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
