use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(name = "slap-your-openclaw")]
#[command(
    about = "Detect laptop slaps via Apple Silicon accelerometer and trigger OpenClaw agent events"
)]
#[command(version)]
pub struct Cli {
    /// Cooldown between events in milliseconds
    #[arg(long = "cooldown", env = "SLAP_COOLDOWN", default_value_t = 500)]
    pub cooldown_ms: u64,

    /// Minimum severity level to publish (1-6)
    #[arg(long, env = "SLAP_MIN_LEVEL", default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=6))]
    pub min_level: u8,

    /// Minimum SLAP amplitude (g) to publish
    #[arg(long, env = "SLAP_MIN_SLAP_AMP", default_value_t = 0.010)]
    pub min_slap_amp: f64,

    /// Minimum SHAKE amplitude (g) to publish
    #[arg(long, env = "SLAP_MIN_SHAKE_AMP", default_value_t = 0.030)]
    pub min_shake_amp: f64,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Run as MCP server over stdio (for AI agent integration)
    Mcp,

    /// Run in standalone mode (default if no subcommand)
    Standalone(StandaloneArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct StandaloneArgs {
    /// Legacy compatibility flag (no-op)
    #[arg(long, env = "OPENCLAW_DIRECT", default_value_t = false)]
    pub openclaw: bool,

    /// OpenClaw agent id for direct event delivery
    #[arg(long, env = "OPENCLAW_AGENT", default_value = "main")]
    pub openclaw_agent: String,

    /// OpenClaw thinking level (off|minimal|low|medium|high)
    #[arg(long, env = "OPENCLAW_THINKING", default_value = "off", value_parser = ["off", "minimal", "low", "medium", "high"])]
    pub openclaw_thinking: String,

    /// OpenClaw session id for slap/shake events
    #[arg(long, env = "OPENCLAW_SESSION_ID", default_value = "slap-detector")]
    pub openclaw_session_id: String,

    /// OpenClaw command timeout in seconds
    #[arg(long = "openclaw-timeout", env = "OPENCLAW_TIMEOUT", default_value_t = 20)]
    pub openclaw_timeout_sec: u64,

    /// Deliver agent reply back to channel
    #[arg(long, env = "OPENCLAW_DELIVER", default_value_t = false)]
    pub openclaw_deliver: bool,

    /// Reply channel override (e.g. discord)
    #[arg(long, env = "OPENCLAW_REPLY_CHANNEL")]
    pub openclaw_reply_channel: Option<String>,

    /// Reply target override (e.g. channel:123456789)
    #[arg(long, env = "OPENCLAW_REPLY_TO")]
    pub openclaw_reply_to: Option<String>,

    /// Force running openclaw as this user (defaults to SUDO_USER)
    #[arg(long, env = "OPENCLAW_RUN_AS")]
    pub openclaw_run_as: Option<String>,

    /// OpenClaw CLI binary path/name
    #[arg(long, env = "OPENCLAW_BIN", default_value = "openclaw")]
    pub openclaw_bin: String,

    /// Local mode: print events to stdout instead of invoking OpenClaw
    #[arg(long, default_value_t = false)]
    pub local: bool,
}

impl Default for StandaloneArgs {
    fn default() -> Self {
        Self {
            openclaw: false,
            openclaw_agent: "main".into(),
            openclaw_thinking: "off".into(),
            openclaw_session_id: "slap-detector".into(),
            openclaw_timeout_sec: 20,
            openclaw_deliver: false,
            openclaw_reply_channel: None,
            openclaw_reply_to: None,
            openclaw_run_as: None,
            openclaw_bin: "openclaw".into(),
            local: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_no_subcommand() {
        let cli = Cli::try_parse_from(["slap-your-openclaw"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.cooldown_ms, 500);
        assert_eq!(cli.min_level, 4);
        assert!((cli.min_slap_amp - 0.010).abs() < f64::EPSILON);
        assert!((cli.min_shake_amp - 0.030).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mcp_subcommand() {
        let cli =
            Cli::try_parse_from(["slap-your-openclaw", "--min-level", "3", "mcp"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Mcp)));
        assert_eq!(cli.min_level, 3);
    }

    #[test]
    fn test_standalone_subcommand() {
        let cli = Cli::try_parse_from([
            "slap-your-openclaw",
            "standalone",
            "--openclaw-agent",
            "ops",
            "--openclaw-thinking",
            "minimal",
            "--openclaw-session-id",
            "slap-prod",
            "--openclaw-timeout",
            "8",
            "--openclaw-deliver",
            "--openclaw-reply-channel",
            "discord",
            "--openclaw-reply-to",
            "channel:123",
            "--openclaw-run-as",
            "niskan516",
            "--openclaw-bin",
            "/usr/local/bin/openclaw",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Standalone(args)) => {
                assert_eq!(args.openclaw_agent, "ops");
                assert_eq!(args.openclaw_thinking, "minimal");
                assert_eq!(args.openclaw_session_id, "slap-prod");
                assert_eq!(args.openclaw_timeout_sec, 8);
                assert!(args.openclaw_deliver);
                assert_eq!(args.openclaw_reply_channel.as_deref(), Some("discord"));
                assert_eq!(args.openclaw_reply_to.as_deref(), Some("channel:123"));
                assert_eq!(args.openclaw_run_as.as_deref(), Some("niskan516"));
                assert_eq!(args.openclaw_bin, "/usr/local/bin/openclaw");
            }
            _ => panic!("Expected Standalone command"),
        }
    }

    #[test]
    fn test_detector_args_with_standalone() {
        let cli = Cli::try_parse_from([
            "slap-your-openclaw",
            "--cooldown",
            "1000",
            "--min-level",
            "5",
            "--min-slap-amp",
            "0.02",
            "--min-shake-amp",
            "0.05",
            "standalone",
            "--local",
        ])
        .unwrap();
        assert_eq!(cli.cooldown_ms, 1000);
        assert_eq!(cli.min_level, 5);
        assert!((cli.min_slap_amp - 0.02).abs() < 1e-9);
        assert!((cli.min_shake_amp - 0.05).abs() < 1e-9);
        match cli.command {
            Some(Command::Standalone(args)) => assert!(args.local),
            _ => panic!("Expected Standalone command"),
        }
    }

    #[test]
    fn test_invalid_min_level() {
        let result = Cli::try_parse_from(["slap-your-openclaw", "--min-level", "7"]);
        assert!(result.is_err());
    }
}
