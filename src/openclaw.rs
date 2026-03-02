use std::process::Stdio;
use std::time::Instant;

use tokio::process::Command;
use tokio::sync::mpsc;

use crate::config::StandaloneArgs;
use crate::detector;

/// OpenClaw direct publisher handle.
pub struct Publisher {
    tx: mpsc::UnboundedSender<detector::Event>,
}

impl Publisher {
    /// Create publisher and spawn background worker that invokes `openclaw agent`.
    pub fn connect(config: &StandaloneArgs) -> Result<Self, String> {
        let (tx, mut rx) = mpsc::unbounded_channel::<detector::Event>();

        let settings = Settings::from_config(config);
        eprintln!(
            "openclaw: routing to agent={} session_id={}",
            settings.agent, settings.session_id
        );
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Coalesce burst events while worker is busy so we keep latency bounded.
                let mut latest = event;
                let mut dropped = 0usize;
                while let Ok(next) = rx.try_recv() {
                    latest = next;
                    dropped += 1;
                }
                if dropped > 0 {
                    eprintln!("openclaw: coalesced {dropped} queued event(s)");
                }

                if let Err(e) = send_event(&settings, &latest).await {
                    eprintln!("openclaw publish error: {e}");
                }
            }
        });

        Ok(Self { tx })
    }

    /// Publish a slap event to OpenClaw agent.
    pub fn publish(&self, event: &detector::Event) -> Result<(), String> {
        self.tx
            .send(event.clone())
            .map_err(|e| format!("OpenClaw channel send error: {e}"))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Settings {
    openclaw_bin: String,
    agent: String,
    session_id: String,
    thinking: String,
    timeout_sec: u64,
    deliver: bool,
    reply_channel: Option<String>,
    reply_to: Option<String>,
    run_as_user: Option<String>,
}

impl Settings {
    fn from_config(config: &StandaloneArgs) -> Self {
        Self {
            openclaw_bin: config.openclaw_bin.clone(),
            agent: config.openclaw_agent.clone(),
            session_id: config.openclaw_session_id.clone(),
            thinking: config.openclaw_thinking.clone(),
            timeout_sec: config.openclaw_timeout_sec,
            deliver: config.openclaw_deliver,
            reply_channel: config.openclaw_reply_channel.clone(),
            reply_to: config.openclaw_reply_to.clone(),
            run_as_user: config
                .openclaw_run_as
                .clone()
                .or_else(|| std::env::var("SUDO_USER").ok()),
        }
    }
}

async fn send_event(settings: &Settings, event: &detector::Event) -> Result<(), String> {
    let started_at = Instant::now();
    let correlation_id = format!("slap-{}", uuid::Uuid::new_v4());
    // Keep this structured: prompt can parse and decide final wording.
    let msg = format!(
        "{}_EVENT level={} severity={} amplitude={:.5}g correlationId={}",
        event.kind.as_str(),
        event.severity.level(),
        event.severity.as_str(),
        event.amplitude,
        correlation_id
    );

    let mut cmd = if let Some(user) = &settings.run_as_user {
        let mut c = Command::new("sudo");
        c.args(["-u", user, &settings.openclaw_bin]);
        c
    } else {
        Command::new(&settings.openclaw_bin)
    };

    cmd.arg("agent")
        .arg("--agent")
        .arg(&settings.agent)
        .arg("--session-id")
        .arg(&settings.session_id)
        .arg("--thinking")
        .arg(&settings.thinking)
        .arg("--timeout")
        .arg(settings.timeout_sec.to_string())
        .arg("--message")
        .arg(msg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if settings.deliver {
        cmd.arg("--deliver");
        if let Some(ch) = &settings.reply_channel {
            cmd.arg("--reply-channel").arg(ch);
        }
        if let Some(target) = &settings.reply_to {
            cmd.arg("--reply-to").arg(target);
        }
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to start openclaw command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "openclaw exited with status {} (stderr: {}; stdout: {})",
            output.status, stderr, stdout
        ));
    }

    eprintln!(
        "openclaw: delivered in {}ms",
        started_at.elapsed().as_millis()
    );

    Ok(())
}
