use std::io::{self, Write};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{broadcast, RwLock};

use crate::detector::{self, Detector, Event, EventKind};
use crate::sensor::iokit::SensorRing;

const ARMING_SAMPLES: usize = detector::SAMPLE_RATE_HZ; // 1.0s
const MAX_RECENT_EVENTS: usize = 100;

/// Detector phase for external visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DetectorPhase {
    Warmup,
    Arming,
    Ready,
}

/// Runtime-mutable detector configuration.
pub struct DetectorConfig {
    pub cooldown_ms: u64,
    pub min_level: u8,
    pub min_slap_amp: f64,
    pub min_shake_amp: f64,
}

/// Event with a unix timestamp for MCP consumers.
#[derive(Debug, Clone, Serialize)]
pub struct TimestampedEvent {
    pub event: Event,
    pub timestamp: f64,
}

/// Shared state between detection loop and output modes.
pub struct SharedState {
    pub config: RwLock<DetectorConfig>,
    pub phase: RwLock<DetectorPhase>,
    pub samples_processed: RwLock<usize>,
    pub event_tx: broadcast::Sender<TimestampedEvent>,
    pub recent_events: RwLock<Vec<TimestampedEvent>>,
    pub sensor_running: RwLock<bool>,
    pub started_at: Instant,
}

impl SharedState {
    pub fn new(config: DetectorConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            config: RwLock::new(config),
            phase: RwLock::new(DetectorPhase::Warmup),
            samples_processed: RwLock::new(0),
            event_tx,
            recent_events: RwLock::new(Vec::new()),
            sensor_running: RwLock::new(true),
            started_at: Instant::now(),
        }
    }
}

/// Run the detection loop, feeding sensor data into the detector and
/// broadcasting events through shared state. This is extracted from the
/// original main.rs loop so both standalone and MCP modes can use it.
pub async fn run_detection_loop(mut ring: SensorRing, state: &SharedState) {
    let mut det = Detector::new();
    let mut last_publish = Instant::now() - Duration::from_secs(10);
    let mut processed_samples: usize = 0;
    let mut warmup_last_tenths: Option<u64> = None;
    let mut arming_last_tenths: Option<u64> = None;
    let mut detector_ready_logged = false;
    let warmup_total_tenths =
        ((detector::WARMUP_SAMPLES * 10) as u64).div_ceil(detector::SAMPLE_RATE_HZ as u64);
    let arming_total_tenths =
        ((ARMING_SAMPLES * 10) as u64).div_ceil(detector::SAMPLE_RATE_HZ as u64);
    let bar_width = 25_u64;
    let armed_after_samples = detector::WARMUP_SAMPLES + ARMING_SAMPLES;

    let mut interval = tokio::time::interval(Duration::from_millis(10));
    loop {
        interval.tick().await;

        if !ring.is_running() {
            *state.sensor_running.write().await = false;
            eprintln!("error: sensor thread stopped unexpectedly");
            return;
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
        processed_samples += n_samples;
        *state.samples_processed.write().await = processed_samples;

        let remaining_warmup = detector::WARMUP_SAMPLES.saturating_sub(processed_samples);
        let remaining_arming = armed_after_samples.saturating_sub(processed_samples);
        let armed = processed_samples >= armed_after_samples;

        // Update phase
        {
            let new_phase = if remaining_warmup > 0 {
                DetectorPhase::Warmup
            } else if !armed {
                DetectorPhase::Arming
            } else {
                DetectorPhase::Ready
            };
            *state.phase.write().await = new_phase;
        }

        // Progress bars on stderr (same as original)
        if remaining_warmup > 0 {
            let tenths =
                ((remaining_warmup * 10) as u64).div_ceil(detector::SAMPLE_RATE_HZ as u64);
            if warmup_last_tenths != Some(tenths) {
                let done_tenths = warmup_total_tenths.saturating_sub(tenths);
                let filled = ((done_tenths * bar_width) / warmup_total_tenths.max(1)) as usize;
                let empty = bar_width as usize - filled;
                let mut stderr = io::stderr();
                let _ = write!(
                    stderr,
                    "\r\x1b[2Kwarmup: [{}{}] {:.1}s remaining",
                    "#".repeat(filled),
                    "-".repeat(empty),
                    tenths as f64 / 10.0
                );
                let _ = stderr.flush();
                warmup_last_tenths = Some(tenths);
            }
        } else if !armed {
            let arming_tenths =
                ((remaining_arming * 10) as u64).div_ceil(detector::SAMPLE_RATE_HZ as u64);
            if arming_last_tenths != Some(arming_tenths) {
                let done_tenths = arming_total_tenths.saturating_sub(arming_tenths);
                let filled = ((done_tenths * bar_width) / arming_total_tenths.max(1)) as usize;
                let empty = bar_width as usize - filled;
                let mut stderr = io::stderr();
                let _ = write!(
                    stderr,
                    "\r\x1b[2Karming: [{}{}] {:.1}s remaining",
                    "#".repeat(filled),
                    "-".repeat(empty),
                    arming_tenths as f64 / 10.0
                );
                let _ = stderr.flush();
                arming_last_tenths = Some(arming_tenths);
            }
        } else if !detector_ready_logged {
            let mut stderr = io::stderr();
            let _ = writeln!(
                stderr,
                "\r\x1b[2Kdetector: [{}] ready",
                "#".repeat(bar_width as usize)
            );
            let _ = stderr.flush();
            detector_ready_logged = true;
        }

        // Read current config for filtering
        let config = state.config.read().await;
        let cooldown = Duration::from_millis(config.cooldown_ms);
        let min_level = config.min_level;
        let min_slap_amp = config.min_slap_amp;
        let min_shake_amp = config.min_shake_amp;
        drop(config);

        for event in det.drain_events() {
            if !armed {
                continue;
            }
            if !matches!(event.kind, EventKind::Slap | EventKind::Shake) {
                continue;
            }
            if event.severity.level() < min_level {
                continue;
            }

            // Anti-typing guard
            if matches!(event.kind, EventKind::Slap) {
                let has_peak = event.sources.iter().any(|s| s == "PEAK");
                if !has_peak && event.amplitude < 0.030 {
                    continue;
                }
            }

            if matches!(event.kind, EventKind::Slap) && event.amplitude < min_slap_amp {
                continue;
            }
            if matches!(event.kind, EventKind::Shake) && event.amplitude < min_shake_amp {
                continue;
            }

            if last_publish.elapsed() < cooldown {
                continue;
            }

            eprintln!(
                ">>> {} #{} [{}  amp={:.5}g] sources={:?}",
                event.kind.as_str(),
                event.severity.level(),
                event.severity.as_str(),
                event.amplitude,
                event.sources,
            );

            let ts_event = TimestampedEvent {
                event,
                timestamp: now_secs,
            };

            // Broadcast to all subscribers (MCP, standalone, etc.)
            let _ = state.event_tx.send(ts_event.clone());

            // Append to recent events ring
            {
                let mut recent = state.recent_events.write().await;
                recent.push(ts_event);
                let len = recent.len();
                if len > MAX_RECENT_EVENTS {
                    recent.drain(..len - MAX_RECENT_EVENTS);
                }
            }

            last_publish = Instant::now();
        }
    }
}
