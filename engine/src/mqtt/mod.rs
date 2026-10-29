//! MQTT 通道：订阅录制 / 发布重放；仅标准库实现最小 MQTT 3.1.1。

mod broker;
mod client;
mod codec;

use std::io;
use std::net::SocketAddr;

pub use broker::MiniBroker;
pub use client::MqttClient;
pub use codec::{topic_matches, Publish};

/// `.fcap` 帧 flags：低 2 bit = QoS；bit2 = retain。
pub const FLAG_MQTT_QOS_MASK: u32 = 0b11;
pub const FLAG_MQTT_RETAIN: u32 = 1 << 2;

pub fn pack_mqtt_flags(qos: u8, retain: bool) -> u32 {
    let mut f = (qos.min(2) as u32) & FLAG_MQTT_QOS_MASK;
    if retain {
        f |= FLAG_MQTT_RETAIN;
    }
    f
}

pub fn mqtt_qos_from_flags(flags: u32) -> u8 {
    (flags & FLAG_MQTT_QOS_MASK) as u8
}

pub fn mqtt_retain_from_flags(flags: u32) -> bool {
    flags & FLAG_MQTT_RETAIN != 0
}

#[derive(Debug, Clone)]
pub struct MqttConfig {
    pub broker: String,
    pub port: u16,
    pub topic: String,
    pub qos: u8,
}

impl MqttConfig {
    pub fn new(broker: impl Into<String>, port: u16, topic: impl Into<String>, qos: u8) -> Self {
        Self {
            broker: broker.into(),
            port,
            topic: topic.into(),
            qos: qos.min(2),
        }
    }

    /// 与 `docs/design/fcap-format.md` MQTT `config_json` 对齐。
    pub fn to_config_json(&self) -> String {
        format!(
            r#"{{"broker":"{}","port":{},"topic":"{}","qos":{}}}"#,
            escape_json_str(&self.broker),
            self.port,
            escape_json_str(&self.topic),
            self.qos
        )
    }

    pub fn from_config_json(s: &str) -> io::Result<Self> {
        let broker = extract_json_str(s, "broker").unwrap_or_else(|| "127.0.0.1".into());
        let port = extract_json_u64(s, "port").unwrap_or(1883);
        if port > u16::MAX as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MQTT port 超出范围"));
        }
        let topic = extract_json_str(s, "topic").unwrap_or_else(|| "#".into());
        let qos = extract_json_u64(s, "qos").unwrap_or(0).min(2) as u8;
        Ok(Self {
            broker,
            port: port as u16,
            topic,
            qos,
        })
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        crate::tcp::resolve_one(&format!("{}:{}", self.broker, self.port))
    }
}

/// 重放时可给录制 topic 加前缀（与 UI `topicPrefix` 一致：直接拼接）。
pub fn apply_topic_prefix(prefix: &str, topic: &str) -> String {
    if prefix.is_empty() {
        topic.to_string()
    } else {
        format!("{prefix}{topic}")
    }
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    let after = after.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    if !after.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                other => out.push(other),
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    let after = after.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    let end = after
        .find(|c: char| !(c.is_ascii_digit()))
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_json_roundtrip() {
        let cfg = MqttConfig::new("192.168.1.10", 1883, "factory/line3/#", 1);
        let parsed = MqttConfig::from_config_json(&cfg.to_config_json()).unwrap();
        assert_eq!(parsed.broker, "192.168.1.10");
        assert_eq!(parsed.port, 1883);
        assert_eq!(parsed.topic, "factory/line3/#");
        assert_eq!(parsed.qos, 1);
    }

    #[test]
    fn flags_pack() {
        assert_eq!(pack_mqtt_flags(1, true), 0b101);
        assert_eq!(mqtt_qos_from_flags(0b101), 1);
        assert!(mqtt_retain_from_flags(0b101));
    }
}
