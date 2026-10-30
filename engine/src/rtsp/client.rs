//! 最小 RTSP 客户端：OPTIONS→DESCRIBE→SETUP(交织 TCP)→PLAY，按交织块收 RTP。

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::url::RtspUrl;

/// 已完成 PLAY 的拉流会话；`poll_rtp` 返回交织通道 0 的原始块（不解 NAL）。
pub struct RtspClient {
    stream: TcpStream,
    buf: Vec<u8>,
    cseq: u32,
    session: String,
    base_uri: String,
}

impl RtspClient {
    /// 连接并对 `url` 完成握手；`read_timeout` 用于轮询 stop。
    pub fn open(url: &RtspUrl, read_timeout: Duration) -> io::Result<Self> {
        let addr = crate::tcp::resolve_one(&format!("{}:{}", url.host, url.port))?;
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(read_timeout))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let mut client = Self {
            stream,
            buf: Vec::with_capacity(8192),
            cseq: 0,
            session: String::new(),
            base_uri: url.request_uri(),
        };

        let _ = client.request("OPTIONS", &url.request_uri(), &[])?;

        let describe = client.request(
            "DESCRIBE",
            &url.request_uri(),
            &[("Accept", "application/sdp")],
        )?;
        if let Some(base) = header_value(&describe.headers, "Content-Base")
            .or_else(|| header_value(&describe.headers, "Content-Location"))
        {
            client.base_uri = base.trim().trim_end_matches('/').to_string();
            if !client.base_uri.to_ascii_lowercase().starts_with("rtsp://") {
                client.base_uri = url.request_uri();
            }
        }
        let control = pick_video_control(&describe.body).unwrap_or_else(|| "*".into());
        let setup_uri = if control == "*" {
            url.request_uri()
        } else if control.to_ascii_lowercase().starts_with("rtsp://") {
            control
        } else {
            // 相对 control：优先拼到 Content-Base，否则拼到请求 URL
            resolve_control(&client.base_uri, &url.request_uri(), &control)
        };

        let setup = client.request(
            "SETUP",
            &setup_uri,
            &[(
                "Transport",
                "RTP/AVP/TCP;unicast;interleaved=0-1",
            )],
        )?;
        client.session = header_value(&setup.headers, "Session")
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "SETUP 响应缺 Session")
            })?;

        let play_uri = url.request_uri();
        let session = client.session.clone();
        let _ = client.request(
            "PLAY",
            &play_uri,
            &[("Session", session.as_str()), ("Range", "npt=0.000-")],
        )?;

        Ok(client)
    }

    /// 轮询一块 RTP（交织 channel 0）；跳过 RTCP（channel 1）与偶发 RTSP 应答。
    /// `Ok(None)` = 读超时无数据。
    pub fn poll_rtp(&mut self, stop: &AtomicBool) -> io::Result<Option<Vec<u8>>> {
        while !stop.load(Ordering::SeqCst) {
            match self.read_next()? {
                None => return Ok(None),
                Some(FrameOrMsg::Rtp(payload)) => return Ok(Some(payload)),
                Some(FrameOrMsg::Rtcp(_)) => continue,
                Some(FrameOrMsg::Rtsp(_)) => continue,
            }
        }
        Ok(None)
    }

    pub fn teardown(mut self) {
        if self.session.is_empty() {
            return;
        }
        let session = self.session.clone();
        let uri = self.base_uri.clone();
        let _ = self.request("TEARDOWN", &uri, &[("Session", session.as_str())]);
    }

    fn request(
        &mut self,
        method: &str,
        uri: &str,
        extra: &[(&str, &str)],
    ) -> io::Result<RtspResponse> {
        self.cseq += 1;
        let mut req = format!(
            "{method} {uri} RTSP/1.0\r\nCSeq: {}\r\nUser-Agent: FieldReplay/0.1\r\n",
            self.cseq
        );
        for (k, v) in extra {
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        if !self.session.is_empty() && !extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("Session"))
        {
            req.push_str("Session: ");
            req.push_str(&self.session);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        self.stream.write_all(req.as_bytes())?;

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("等待 {method} 响应超时"),
                ));
            }
            match self.read_next()? {
                None => continue,
                Some(FrameOrMsg::Rtsp(resp)) => {
                    if resp.status < 200 || resp.status >= 300 {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "RTSP {method} 失败: {} {}",
                                resp.status, resp.reason
                            ),
                        ));
                    }
                    return Ok(resp);
                }
                // 握手阶段偶发交错帧：丢弃
                Some(FrameOrMsg::Rtp(_)) | Some(FrameOrMsg::Rtcp(_)) => continue,
            }
        }
    }

    fn read_next(&mut self) -> io::Result<Option<FrameOrMsg>> {
        loop {
            if !self.buf.is_empty() {
                if self.buf[0] == b'$' {
                    if self.buf.len() < 4 {
                        // 继续读
                    } else {
                        let channel = self.buf[1];
                        let len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
                        if self.buf.len() >= 4 + len {
                            let payload = self.buf[4..4 + len].to_vec();
                            self.buf.drain(..4 + len);
                            return Ok(Some(if channel == 0 {
                                FrameOrMsg::Rtp(payload)
                            } else {
                                FrameOrMsg::Rtcp(payload)
                            }));
                        }
                    }
                } else {
                    // RTSP 文本响应
                    if let Some(resp) = try_parse_response(&mut self.buf)? {
                        return Ok(Some(FrameOrMsg::Rtsp(resp)));
                    }
                }
            }

            let mut tmp = [0u8; 4096];
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "RTSP 连接已关闭",
                    ));
                }
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

enum FrameOrMsg {
    Rtp(Vec<u8>),
    Rtcp(Vec<u8>),
    Rtsp(RtspResponse),
}

struct RtspResponse {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn try_parse_response(buf: &mut Vec<u8>) -> io::Result<Option<RtspResponse>> {
    let header_end = match find_double_crlf(buf) {
        Some(i) => i,
        None => return Ok(None),
    };
    let header_bytes = &buf[..header_end];
    let header_str = std::str::from_utf8(header_bytes).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "RTSP 响应非 UTF-8")
    })?;
    let mut lines = header_str.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let _proto = parts.next();
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "RTSP 状态行无效"))?;
    let reason = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if key.eq_ignore_ascii_case("Content-Length") {
                content_length = val.parse().unwrap_or(0);
            }
            headers.push((key, val));
        }
    }
    let body_start = header_end + 4;
    if buf.len() < body_start + content_length {
        return Ok(None);
    }
    let body = buf[body_start..body_start + content_length].to_vec();
    buf.drain(..body_start + content_length);
    Ok(Some(RtspResponse {
        status,
        reason,
        headers,
        body,
    }))
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn pick_video_control(sdp: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(sdp).ok()?;
    let mut in_video = false;
    let mut session_control: Option<String> = None;
    let mut media_control: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("m=") {
            in_video = line.to_ascii_lowercase().starts_with("m=video");
            continue;
        }
        if let Some(rest) = line.strip_prefix("a=control:") {
            let c = rest.trim().to_string();
            if in_video {
                media_control = Some(c);
            } else if session_control.is_none() {
                session_control = Some(c);
            }
        }
    }
    media_control.or(session_control)
}

fn resolve_control(content_base: &str, request_uri: &str, control: &str) -> String {
    let base = if content_base.to_ascii_lowercase().starts_with("rtsp://") {
        content_base
    } else {
        request_uri
    };
    if control.starts_with('/') {
        if let Some(idx) = base.find("://") {
            let after = &base[idx + 3..];
            if let Some(slash) = after.find('/') {
                return format!("{}{}", &base[..idx + 3 + slash], control);
            }
            return format!("{base}{control}");
        }
    }
    if base.ends_with('/') {
        format!("{base}{control}")
    } else {
        format!("{base}/{control}")
    }
}

/// 编码交织帧（自检 / 迷你服务端用）。
pub fn encode_interleaved(channel: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.push(b'$');
    out.push(channel);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}
