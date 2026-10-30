//! RTSP 信令最小子集（OPTIONS/DESCRIBE/SETUP/PLAY/TEARDOWN）；仅交织 TCP。

use std::io::{self, Read, Write};
use std::net::TcpStream;

pub struct RtspRequest {
    pub method: String,
    pub uri: String,
    pub cseq: String,
}

/// 从缓冲读一条完整 RTSP 请求；数据不足返回 `Ok(None)`。
pub fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<Option<RtspRequest>> {
    let mut tmp = [0u8; 2048];
    match stream.read(&mut tmp) {
        Ok(0) => {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "客户端断开",
            ));
        }
        Ok(n) => {
            buf.extend_from_slice(&tmp[..n]);
        }
        Err(e)
            if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    }

    // 跳过客户端偶发的交织噪声
    while buf.first() == Some(&b'$') {
        if buf.len() < 4 {
            return Ok(None);
        }
        let len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        if buf.len() < 4 + len {
            return Ok(None);
        }
        buf.drain(..4 + len);
    }

    let end = match buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => i,
        None => return Ok(None),
    };
    let header = std::str::from_utf8(&buf[..end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "请求非 UTF-8"))?;
    let mut lines = header.split("\r\n");
    let start = lines.next().unwrap_or("");
    let mut sp = start.split_whitespace();
    let method = sp.next().unwrap_or("").to_string();
    let uri = sp.next().unwrap_or("").to_string();
    let mut cseq = String::from("0");
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("CSeq") {
                cseq = v.trim().to_string();
            }
        }
    }
    buf.drain(..end + 4);
    Ok(Some(RtspRequest { method, uri, cseq }))
}

/// 处理一条信令。返回 `true` 表示已 PLAY，可开始交织吐块。
pub fn handle_request(
    stream: &mut TcpStream,
    req: &RtspRequest,
    session: &mut String,
) -> io::Result<bool> {
    match req.method.as_str() {
        "OPTIONS" => {
            write_response(
                stream,
                200,
                &req.cseq,
                &[("Public", "OPTIONS, DESCRIBE, SETUP, PLAY, TEARDOWN")],
                b"",
            )?;
            Ok(false)
        }
        "DESCRIBE" => {
            let sdp = b"v=0\r\n\
o=- 0 0 IN IP4 127.0.0.1\r\n\
s=FieldReplay\r\n\
t=0 0\r\n\
m=video 0 RTP/AVP 96\r\n\
a=rtpmap:96 H264/90000\r\n\
a=control:track1\r\n";
            let content_base = format!("{}/", trim_track(&req.uri));
            write_response(
                stream,
                200,
                &req.cseq,
                &[
                    ("Content-Type", "application/sdp"),
                    ("Content-Base", content_base.as_str()),
                ],
                sdp,
            )?;
            Ok(false)
        }
        "SETUP" => {
            *session = "1".into();
            write_response(
                stream,
                200,
                &req.cseq,
                &[
                    ("Transport", "RTP/AVP/TCP;unicast;interleaved=0-1"),
                    ("Session", session.as_str()),
                ],
                b"",
            )?;
            Ok(false)
        }
        "PLAY" => {
            write_response(
                stream,
                200,
                &req.cseq,
                &[
                    ("Session", session.as_str()),
                    ("Range", "npt=0.000-"),
                ],
                b"",
            )?;
            Ok(true)
        }
        "TEARDOWN" => {
            write_response(
                stream,
                200,
                &req.cseq,
                &[("Session", session.as_str())],
                b"",
            )?;
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "TEARDOWN",
            ))
        }
        _ => {
            write_response(stream, 501, &req.cseq, &[], b"")?;
            Ok(false)
        }
    }
}

pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    cseq: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        501 => "Not Implemented",
        _ => "Error",
    };
    let mut out = format!("RTSP/1.0 {status} {reason}\r\nCSeq: {cseq}\r\n");
    for (k, v) in headers {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(out.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    Ok(())
}

fn trim_track(uri: &str) -> String {
    if let Some(i) = uri.rfind("/track") {
        uri[..i].to_string()
    } else {
        uri.trim_end_matches('/').to_string()
    }
}
