//! TCP 通道：角色可配（server / client）；仅标准库。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 录制时工具扮演的角色；重放时引擎取反（演设备）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpRole {
    Server,
    Client,
}

impl TcpRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "server" | "listen" | "s" => Some(Self::Server),
            "client" | "connect" | "c" => Some(Self::Client),
            _ => None,
        }
    }

    /// 重放侧相对录制配置取反。
    pub fn invert(self) -> Self {
        match self {
            Self::Server => Self::Client,
            Self::Client => Self::Server,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TcpConfig {
    pub role: TcpRole,
    pub host: String,
    pub port: u16,
}

impl TcpConfig {
    pub fn new(role: TcpRole, host: impl Into<String>, port: u16) -> Self {
        Self {
            role,
            host: host.into(),
            port,
        }
    }

    /// 与 `docs/design/fcap-format.md` TCP `config_json` 对齐。
    pub fn to_config_json(&self) -> String {
        format!(
            r#"{{"role":"{}","host":"{}","port":{}}}"#,
            self.role.as_str(),
            escape_json_str(&self.host),
            self.port
        )
    }

    /// 从通道表 JSON 解析（只认本引擎写出的键；宽松抠字段）。
    pub fn from_config_json(s: &str) -> io::Result<Self> {
        let role = extract_json_str(s, "role")
            .and_then(|r| TcpRole::parse(&r))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TCP config 缺 role"))?;
        let host = extract_json_str(s, "host").unwrap_or_else(|| "0.0.0.0".into());
        let port = extract_json_u64(s, "port")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TCP config 缺 port"))?;
        if port > u16::MAX as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "TCP port 超出范围"));
        }
        Ok(Self {
            role,
            host,
            port: port as u16,
        })
    }

    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        resolve_one(&format!("{}:{}", self.host, self.port))
    }
}

pub fn resolve_one(addr: &str) -> io::Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("无法解析地址: {addr}")))
}

pub fn peer_aux(peer: SocketAddr) -> Vec<u8> {
    peer.to_string().into_bytes()
}

/// 绑定并 listen；返回 listener（已可 accept）。
pub fn listen(bind: SocketAddr) -> io::Result<TcpListener> {
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// 轮询 accept，直到连上、`stop`，或调用方自行超时。
/// `Ok(None)` = 本轮未连上（被 stop 打断）。
pub fn accept_until(
    listener: &TcpListener,
    stop: &AtomicBool,
    poll: Duration,
) -> io::Result<Option<(TcpStream, SocketAddr)>> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                prepare_stream(&stream, poll)?;
                return Ok(Some((stream, peer)));
            }
            Err(e) if is_timeout(&e) => {
                std::thread::sleep(poll);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

pub fn connect(addr: SocketAddr, read_timeout: Duration) -> io::Result<TcpStream> {
    let stream = TcpStream::connect(addr)?;
    prepare_stream(&stream, read_timeout)?;
    Ok(stream)
}

fn prepare_stream(stream: &TcpStream, read_timeout: Duration) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_nodelay(true)?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Data(usize),
    Timeout,
    Eof,
}

/// 读一段；超时 / EOF 与错误分开。
pub fn read_some(stream: &TcpStream, buf: &mut [u8]) -> io::Result<ReadOutcome> {
    match (&*stream).read(buf) {
        Ok(0) => Ok(ReadOutcome::Eof),
        Ok(n) => Ok(ReadOutcome::Data(n)),
        Err(e) if is_timeout(&e) => Ok(ReadOutcome::Timeout),
        Err(e) => Err(e),
    }
}

pub fn write_all(stream: &TcpStream, payload: &[u8]) -> io::Result<()> {
    (&*stream).write_all(payload)?;
    Ok(())
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
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

/// ponytail: 只抠本引擎写出的扁平 JSON；上限是不认嵌套/空白变体。升级路径：typed 配置或轻量解析器。
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
    fn role_invert_and_json_roundtrip() {
        assert_eq!(TcpRole::Server.invert(), TcpRole::Client);
        assert_eq!(TcpRole::Client.invert(), TcpRole::Server);
        let cfg = TcpConfig::new(TcpRole::Server, "0.0.0.0", 9000);
        let parsed = TcpConfig::from_config_json(&cfg.to_config_json()).unwrap();
        assert_eq!(parsed.role, TcpRole::Server);
        assert_eq!(parsed.host, "0.0.0.0");
        assert_eq!(parsed.port, 9000);
    }
}
