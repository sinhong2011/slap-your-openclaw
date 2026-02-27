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
