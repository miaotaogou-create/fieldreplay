//! MQTT 通道：订阅录制 / 发布重放；MQTT 3.1.1 + 可选 TLS（E9b）。

mod broker;
mod client;
mod codec;
mod stream;
mod tls;

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

pub use broker::MiniBroker;
pub use client::MqttClient;
pub use codec::{topic_matches, Publish};
pub use tls::{write_test_ca_file, TEST_CA_PEM};

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
    /// CONNECT Username（可选，E9a）
    pub username: Option<String>,
    /// CONNECT Password（可选；无 username 时忽略）
    pub password: Option<String>,
    /// E9b：是否 TLS（mqtts）
    pub tls: bool,
    /// E9b：自定义 CA PEM 路径；有则只信该文件，无则系统根
    pub ca_cert_path: Option<PathBuf>,
    /// E9b：SNI / 证书名；缺省用 broker
    pub tls_server_name: Option<String>,
}

impl MqttConfig {
    pub fn new(broker: impl Into<String>, port: u16, topic: impl Into<String>, qos: u8) -> Self {
        Self {
            broker: broker.into(),
            port,
            topic: topic.into(),
            qos: qos.min(2),
            username: None,
            password: None,
            tls: false,
            ca_cert_path: None,
            tls_server_name: None,
        }
    }

    pub fn with_credentials(
        mut self,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        self.username = username.filter(|s| !s.is_empty());
        self.password = password.filter(|s| !s.is_empty());
        if self.username.is_none() {
            self.password = None;
        }
        self
    }

    pub fn with_tls(
        mut self,
        tls: bool,
        ca_cert_path: Option<PathBuf>,
        tls_server_name: Option<String>,
    ) -> Self {
        self.tls = tls;
        self.ca_cert_path = if tls {
            ca_cert_path.filter(|p| !p.as_os_str().is_empty())
        } else {
            None
        };
        self.tls_server_name = if tls {
            tls_server_name.filter(|s| !s.is_empty())
        } else {
            None
        };
        self
    }

    /// 与 `docs/design/fcap-format.md` MQTT `config_json` 对齐。
    pub fn to_config_json(&self) -> String {
        let mut parts = vec![
            format!(r#""broker":"{}""#, escape_json_str(&self.broker)),
            format!(r#""port":{}"#, self.port),
            format!(r#""topic":"{}""#, escape_json_str(&self.topic)),
            format!(r#""qos":{}"#, self.qos),
        ];
        if let Some(u) = &self.username {
            parts.push(format!(r#""username":"{}""#, escape_json_str(u)));
        }
        if let Some(p) = &self.password {
            parts.push(format!(r#""password":"{}""#, escape_json_str(p)));
        }
        if self.tls {
            parts.push(r#""tls":true"#.to_string());
            if let Some(p) = &self.ca_cert_path {
                parts.push(format!(
                    r#""ca_cert_path":"{}""#,
                    escape_json_str(&p.to_string_lossy())
                ));
            }
            if let Some(n) = &self.tls_server_name {
                parts.push(format!(r#""tls_server_name":"{}""#, escape_json_str(n)));
            }
        }
        format!("{{{}}}", parts.join(","))
    }

    pub fn from_config_json(s: &str) -> io::Result<Self> {
        let broker = extract_json_str(s, "broker").unwrap_or_else(|| "127.0.0.1".into());
        let port = extract_json_u64(s, "port").unwrap_or(1883);
        if port > u16::MAX as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MQTT port 超出范围"));
        }
        let topic = extract_json_str(s, "topic").unwrap_or_else(|| "#".into());
        let qos = extract_json_u64(s, "qos").unwrap_or(0).min(2) as u8;
        let username = extract_json_str(s, "username").filter(|s| !s.is_empty());
        let password = extract_json_str(s, "password").filter(|s| !s.is_empty());
        let tls = extract_json_bool(s, "tls").unwrap_or(false);
        let ca_cert_path = extract_json_str(s, "ca_cert_path")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let tls_server_name = extract_json_str(s, "tls_server_name").filter(|s| !s.is_empty());
        Ok(Self {
            broker,
            port: port as u16,
            topic,
            qos,
            username: username.clone(),
            password: if username.is_some() { password } else { None },
            tls,
            ca_cert_path: if tls { ca_cert_path } else { None },
            tls_server_name: if tls { tls_server_name } else { None },
        })
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        crate::tcp::resolve_one(&format!("{}:{}", self.broker, self.port))
    }

    pub fn ca_path(&self) -> Option<&Path> {
        self.ca_cert_path.as_deref()
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

fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    let after = after.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_json_roundtrip() {
        let cfg = MqttConfig::new("192.168.1.10", 8883, "factory/line3/#", 1)
            .with_credentials(Some("field".into()), Some("s3cret".into()))
            .with_tls(
                true,
                Some(PathBuf::from("C:/certs/ca.pem")),
                Some("mqtt.factory.local".into()),
            );
        let parsed = MqttConfig::from_config_json(&cfg.to_config_json()).unwrap();
        assert_eq!(parsed.broker, "192.168.1.10");
        assert_eq!(parsed.port, 8883);
        assert_eq!(parsed.topic, "factory/line3/#");
        assert_eq!(parsed.qos, 1);
        assert_eq!(parsed.username.as_deref(), Some("field"));
        assert_eq!(parsed.password.as_deref(), Some("s3cret"));
        assert!(parsed.tls);
        assert_eq!(
            parsed.ca_cert_path.as_deref(),
            Some(Path::new("C:/certs/ca.pem"))
        );
        assert_eq!(parsed.tls_server_name.as_deref(), Some("mqtt.factory.local"));
    }

    #[test]
    fn config_json_legacy_without_credentials() {
        let parsed = MqttConfig::from_config_json(
            r##"{"broker":"127.0.0.1","port":1883,"topic":"#","qos":0}"##,
        )
        .unwrap();
        assert!(parsed.username.is_none());
        assert!(parsed.password.is_none());
        assert!(!parsed.tls);
    }

    #[test]
    fn flags_pack() {
        assert_eq!(pack_mqtt_flags(1, true), 0b101);
        assert_eq!(mqtt_qos_from_flags(0b101), 1);
        assert!(mqtt_retain_from_flags(0b101));
    }
}
