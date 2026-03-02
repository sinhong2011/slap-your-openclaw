use std::sync::Arc;

use rmcp::{model::*, tool, ServerHandler, ServiceExt};

use crate::shared::SharedState;

#[derive(Clone)]
pub struct SlapServer {
    state: Arc<SharedState>,
}

#[tool(tool_box)]
impl SlapServer {
    pub fn new(state: Arc<SharedState>) -> Self {
        Self { state }
    }

    #[tool(
        name = "slap_status",
        description = "Get current detector status: phase (Warmup/Arming/Ready), samples processed, sensor health, and uptime"
    )]
    async fn slap_status(&self) -> Result<CallToolResult, rmcp::Error> {
        let phase = *self.state.phase.read().await;
        let samples = *self.state.samples_processed.read().await;
        let sensor_running = *self.state.sensor_running.read().await;
        let uptime_secs = self.state.started_at.elapsed().as_secs();

        let result = serde_json::json!({
            "phase": phase,
            "samples_processed": samples,
            "sensor_running": sensor_running,
            "uptime_secs": uptime_secs,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap(),
        )]))
    }

    #[tool(
        name = "slap_get_events",
        description = "Get recent slap/shake events. Returns a JSON array of events with severity, kind, amplitude, sources, and timestamp."
    )]
    async fn slap_get_events(
        &self,
        #[tool(param)] limit: Option<u32>,
        #[tool(param)] min_level: Option<u8>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let recent = self.state.recent_events.read().await;
        let min = min_level.unwrap_or(1);
        let lim = limit.unwrap_or(20) as usize;

        let filtered: Vec<_> = recent
            .iter()
            .filter(|e| e.event.severity.level() >= min)
            .rev()
            .take(lim)
            .collect();

        let json = serde_json::to_string_pretty(&filtered).unwrap();
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "slap_wait_for_event",
        description = "Block until a slap/shake event occurs or timeout. Creates a fresh broadcast subscriber each call. Returns the event JSON or {\"status\": \"timeout\"}."
    )]
    async fn slap_wait_for_event(
        &self,
        #[tool(param)] timeout_secs: Option<u64>,
        #[tool(param)] min_level: Option<u8>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(30).min(120));
        let min = min_level.unwrap_or(4);
        let mut rx = self.state.event_tx.subscribe();

        let result = tokio::time::timeout(timeout, async {
            loop {
                match rx.recv().await {
                    Ok(ts_event) if ts_event.event.severity.level() >= min => {
                        return Some(ts_event);
                    }
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
        })
        .await;

        match result {
            Ok(Some(ts_event)) => {
                let json = serde_json::to_string_pretty(&ts_event).unwrap();
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Ok(None) => Ok(CallToolResult::success(vec![Content::text(
                "{\"status\": \"channel_closed\"}",
            )])),
            Err(_) => Ok(CallToolResult::success(vec![Content::text(
                "{\"status\": \"timeout\"}",
            )])),
        }
    }

    #[tool(
        name = "slap_get_config",
        description = "Get current detector configuration: cooldown_ms, min_level, min_slap_amp, min_shake_amp"
    )]
    async fn slap_get_config(&self) -> Result<CallToolResult, rmcp::Error> {
        let config = self.state.config.read().await;
        let result = serde_json::json!({
            "cooldown_ms": config.cooldown_ms,
            "min_level": config.min_level,
            "min_slap_amp": config.min_slap_amp,
            "min_shake_amp": config.min_shake_amp,
        });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap(),
        )]))
    }

    #[tool(
        name = "slap_set_config",
        description = "Update detector configuration at runtime. All parameters are optional; only provided values are changed."
    )]
    async fn slap_set_config(
        &self,
        #[tool(param)] min_level: Option<u8>,
        #[tool(param)] cooldown_ms: Option<u64>,
        #[tool(param)] min_slap_amp: Option<f64>,
        #[tool(param)] min_shake_amp: Option<f64>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut config = self.state.config.write().await;
        let mut changes = Vec::new();

        if let Some(v) = min_level {
            if !(1..=6).contains(&v) {
                return Err(rmcp::Error::invalid_params(
                    "min_level must be 1-6",
                    None,
                ));
            }
            config.min_level = v;
            changes.push(format!("min_level={v}"));
        }
        if let Some(v) = cooldown_ms {
            config.cooldown_ms = v;
            changes.push(format!("cooldown_ms={v}"));
        }
        if let Some(v) = min_slap_amp {
            config.min_slap_amp = v;
            changes.push(format!("min_slap_amp={v}"));
        }
        if let Some(v) = min_shake_amp {
            config.min_shake_amp = v;
            changes.push(format!("min_shake_amp={v}"));
        }

        if changes.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "No changes specified",
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Updated: {}",
                changes.join(", ")
            ))]))
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for SlapServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Slap detector MCP server. Monitors Apple Silicon MacBook accelerometer for physical slaps and shakes. Use slap_wait_for_event to reactively wait for events, slap_get_events to review history, and slap_status to check detector readiness.".into()
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run(state: Arc<SharedState>) -> Result<(), Box<dyn std::error::Error>> {
    let server = SlapServer::new(state);
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
