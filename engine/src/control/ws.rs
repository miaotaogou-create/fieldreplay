//! 最小 WebSocket 服务端（文本帧，RFC6455），供本地控制面使用。

use std::io::{self, Read, Write};
use std::net::TcpStream;

use super::b64;
use super::sha1;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub fn accept_websocket(stream: &mut TcpStream) -> io::Result<()> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 256];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "握手前连接关闭",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "握手头过大",
            ));
        }
    }

    let req = String::from_utf8_lossy(&buf);
    let mut key = None;
    let mut upgrade_ws = false;
    let mut connection_upgrade = false;
    for line in req.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("sec-websocket-key:") {
            key = Some(line.split_once(':').map(|(_, v)| v.trim().to_string()).unwrap());
        } else if lower.starts_with("upgrade:") && lower.contains("websocket") {
            upgrade_ws = true;
        } else if lower.starts_with("connection:") && lower.contains("upgrade") {
            connection_upgrade = true;
        }
    }

    if !upgrade_ws || !connection_upgrade {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "非 WebSocket 升级请求",
        ));
    }
    let key = key.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "缺少 Sec-WebSocket-Key")
    })?;

    let accept = b64::encode(&sha1::sha1(format!("{key}{WS_GUID}").as_bytes()));
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: {accept}\r\n\
\r\n"
    );
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

/// 读一条完整文本消息；对端 Close 返回 `None`。
pub fn read_text_frame(stream: &mut TcpStream) -> io::Result<Option<String>> {
    let mut header = [0u8; 2];
    if let Err(e) = stream.read_exact(&mut header) {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e);
    }
    let opcode = header[0] & 0x0f;
    let masked = (header[1] & 0x80) != 0;
    let mut len = (header[1] & 0x7f) as u64;
    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        len = u64::from_be_bytes(ext);
    }
    if len > 64 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "帧过大",
        ));
    }
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        stream.read_exact(&mut payload)?;
    }
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    match opcode {
        0x1 => String::from_utf8(payload)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        0x8 => Ok(None),
        0x9 => {
            // ping → pong
            write_frame(stream, 0xA, &payload)?;
            read_text_frame(stream)
        }
        0xA => read_text_frame(stream),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("不支持的 WS opcode: {opcode}"),
        )),
    }
}

pub fn write_text_frame(stream: &mut TcpStream, text: &str) -> io::Result<()> {
    write_frame(stream, 0x1, text.as_bytes())
}

fn write_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(14 + payload.len());
    out.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    stream.write_all(&out)
}
