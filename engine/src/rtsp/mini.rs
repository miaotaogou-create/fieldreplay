//! 自检用迷你 RTSP 服务端：答 DESCRIBE/SETUP/PLAY 后交织吐块（非生产级）。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::client::encode_interleaved;

/// 本机迷你 RTSP：单连接；PLAY 后按序发送预设载荷（通道 0）。
pub struct MiniRtspServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MiniRtspServer {
    pub fn start(payloads: Vec<Vec<u8>>) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);

        let join = thread::spawn(move || {
            if let Err(e) = run_once(listener, payloads, &stop_bg) {
                eprintln!("迷你 RTSP 退出: {e}");
            }
        });

        Ok(Self {
            addr,
            stop,
            join: Some(join),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn url(&self, path: &str) -> String {
        let p = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("rtsp://{}:{}{p}", self.addr.ip(), self.addr.port())
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 碰一下端口，解除可能卡在 accept 的阻塞
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for MiniRtspServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run_once(
    listener: TcpListener,
    payloads: Vec<Vec<u8>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut stream = None;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((s, _)) => {
                stream = Some(s);
                break;
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
    let Some(mut stream) = stream else {
        return Ok(());
    };
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut buf = Vec::new();
    let mut session = String::from("1");
    let mut playing = false;
    let mut sent = 0usize;

    while !stop.load(Ordering::SeqCst) {
        // PLAY 后优先吐块
        if playing && sent < payloads.len() {
            let frame = encode_interleaved(0, &payloads[sent]);
            stream.write_all(&frame)?;
            sent += 1;
            thread::sleep(Duration::from_millis(30));
            continue;
        }

        match read_request(&mut stream, &mut buf) {
            Ok(None) => continue,
            Ok(Some(req)) => {
                handle_request(&mut stream, &req, &mut session, &mut playing)?;
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

struct RtspRequest {
    method: String,
    uri: String,
    cseq: String,
}

fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<Option<RtspRequest>> {
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

fn handle_request(
    stream: &mut TcpStream,
    req: &RtspRequest,
    session: &mut String,
    playing: &mut bool,
) -> io::Result<()> {
    match req.method.as_str() {
        "OPTIONS" => {
            write_response(
                stream,
                200,
                &req.cseq,
                &[("Public", "OPTIONS, DESCRIBE, SETUP, PLAY, TEARDOWN")],
                b"",
            )?;
        }
        "DESCRIBE" => {
            let sdp = b"v=0\r\n\
o=- 0 0 IN IP4 127.0.0.1\r\n\
s=FieldReplay Mini\r\n\
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
        }
        "PLAY" => {
            *playing = true;
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
        }
        "TEARDOWN" => {
            *playing = false;
            write_response(
                stream,
                200,
                &req.cseq,
                &[("Session", session.as_str())],
                b"",
            )?;
        }
        _ => {
            write_response(stream, 501, &req.cseq, &[], b"")?;
        }
    }
    Ok(())
}

fn trim_track(uri: &str) -> String {
    // Content-Base 用会话 URI（去掉 /track1）
    if let Some(i) = uri.rfind("/track") {
        uri[..i].to_string()
    } else {
        uri.trim_end_matches('/').to_string()
    }
}

fn write_response(
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
