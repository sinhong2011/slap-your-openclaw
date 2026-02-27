use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::detector;

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
    pub fn connect(host: &str, port: u16, topic: String) -> Result<Self, String> {
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
        let json =
            serde_json::to_string(&payload).map_err(|e| format!("JSON serialize error: {e}"))?;
        self.tx
            .send(json)
            .map_err(|e| format!("MQTT channel send error: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::{self, Severity};

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
        assert!(parsed["correlationId"]
            .as_str()
            .unwrap()
            .starts_with("slap-"));
    }
}
