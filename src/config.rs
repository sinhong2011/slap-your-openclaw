use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "slap-your-openclaw")]
#[command(about = "Detect laptop slaps via Apple Silicon accelerometer, publish MQTT events")]
#[command(version)]
pub struct Config {
    /// MQTT broker host
    #[arg(long, env = "MQTT_HOST", default_value = "localhost")]
    pub mqtt_host: String,

    /// MQTT broker port
    #[arg(long, env = "MQTT_PORT", default_value_t = 1883)]
    pub mqtt_port: u16,

    /// MQTT publish topic
    #[arg(long, env = "MQTT_TOPIC", default_value = "openclaw/slap/inbound")]
    pub mqtt_topic: String,

    /// Cooldown between events in milliseconds
    #[arg(long = "cooldown", env = "SLAP_COOLDOWN", default_value_t = 500)]
    pub cooldown_ms: u64,

    /// Minimum severity level to publish (1-6)
    #[arg(long, env = "SLAP_MIN_LEVEL", default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=6))]
    pub min_level: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::try_parse_from(["slap-your-openclaw"]).unwrap();
        assert_eq!(config.mqtt_host, "localhost");
        assert_eq!(config.mqtt_port, 1883);
        assert_eq!(config.mqtt_topic, "openclaw/slap/inbound");
        assert_eq!(config.cooldown_ms, 500);
        assert_eq!(config.min_level, 3);
    }

    #[test]
    fn test_custom_config() {
        let config = Config::try_parse_from([
            "slap-your-openclaw",
            "--mqtt-host", "broker.local",
            "--mqtt-port", "8883",
            "--mqtt-topic", "custom/topic",
            "--cooldown", "1000",
            "--min-level", "5",
        ]).unwrap();
        assert_eq!(config.mqtt_host, "broker.local");
        assert_eq!(config.mqtt_port, 8883);
        assert_eq!(config.mqtt_topic, "custom/topic");
        assert_eq!(config.cooldown_ms, 1000);
        assert_eq!(config.min_level, 5);
    }

    #[test]
    fn test_invalid_min_level() {
        let result = Config::try_parse_from([
            "slap-your-openclaw",
            "--min-level", "7",
        ]);
        assert!(result.is_err());
    }
}
