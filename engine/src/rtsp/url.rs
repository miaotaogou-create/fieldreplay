//! `rtsp://` URL 解析（仅 std；无鉴权 / 无 rtspu）。

use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspUrl {
    pub host: String,
    pub port: u16,
    /// 以 `/` 开头的路径（至少 `/`）
    pub path: String,
    pub raw: String,
}

impl RtspUrl {
    pub fn parse(s: &str) -> io::Result<Self> {
        let raw = s.trim().to_string();
        let lower = raw.to_ascii_lowercase();
        if !lower.starts_with("rtsp://") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RTSP URL 须以 rtsp:// 开头",
            ));
        }
        let rest = &raw["rtsp://".len()..];
        if rest.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "RTSP URL 缺主机"));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, "/".to_string()),
        };
        if authority.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "RTSP URL 缺主机"));
        }
        // ponytail: 不解析 user:pass@；上限是无鉴权相机。升级：拆 userinfo。
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = if let Some(h) = authority.strip_prefix('[') {
            // IPv6：[addr]:port
            let end = h
                .find(']')
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "RTSP IPv6 缺 ]"))?;
            let host = h[..end].to_string();
            let after = &h[end + 1..];
            let port = if let Some(p) = after.strip_prefix(':') {
                p.parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "RTSP 端口无效")
                })?
            } else {
                554u16
            };
            (host, port)
        } else if let Some((h, p)) = authority.rsplit_once(':') {
            if h.contains(':') {
                // 裸 IPv6 无括号：不当端口
                (authority.to_string(), 554u16)
            } else {
                let port: u16 = p.parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "RTSP 端口无效")
                })?;
                (h.to_string(), port)
            }
        } else {
            (authority.to_string(), 554u16)
        };
        let path = if path.is_empty() {
            "/".into()
        } else {
            path
        };
        Ok(Self {
            host,
            port,
            path,
            raw,
        })
    }

    pub fn request_uri(&self) -> String {
        format!("rtsp://{}:{}{}", self.host, self.port, self.path)
    }

    pub fn join_control(&self, control: &str) -> String {
        let c = control.trim();
        if c.is_empty() || c == "*" {
            return self.request_uri();
        }
        if c.to_ascii_lowercase().starts_with("rtsp://") {
            return c.to_string();
        }
        let base = self.request_uri();
        if c.starts_with('/') {
            format!("rtsp://{}:{}{}", self.host, self.port, c)
        } else if base.ends_with('/') {
            format!("{base}{c}")
        } else {
            format!("{base}/{c}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_port_and_path() {
        let u = RtspUrl::parse("rtsp://192.168.1.10/live/ch0").unwrap();
        assert_eq!(u.host, "192.168.1.10");
        assert_eq!(u.port, 554);
        assert_eq!(u.path, "/live/ch0");
        assert_eq!(u.request_uri(), "rtsp://192.168.1.10:554/live/ch0");
    }

    #[test]
    fn join_relative_control() {
        let u = RtspUrl::parse("rtsp://127.0.0.1:8554/cam").unwrap();
        assert_eq!(
            u.join_control("track1"),
            "rtsp://127.0.0.1:8554/cam/track1"
        );
        assert_eq!(
            u.join_control("/abs"),
            "rtsp://127.0.0.1:8554/abs"
        );
    }
}
