//! MQTT 3.1.1 最小编解码（CONNECT / CONNACK / PUBLISH / SUBSCRIBE / 应答 / DISCONNECT）。

use std::io;

/// 控制包类型（固定头高 4 bit）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Connect = 1,
    Connack = 2,
    Publish = 3,
    Puback = 4,
    Pubrec = 5,
    Pubrel = 6,
    Pubcomp = 7,
    Subscribe = 8,
    Suback = 9,
    Unsubscribe = 10,
    Unsuback = 11,
    Pingreq = 12,
    Pingresp = 13,
    Disconnect = 14,
}

impl PacketType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Connect),
            2 => Some(Self::Connack),
            3 => Some(Self::Publish),
            4 => Some(Self::Puback),
            5 => Some(Self::Pubrec),
            6 => Some(Self::Pubrel),
            7 => Some(Self::Pubcomp),
            8 => Some(Self::Subscribe),
            9 => Some(Self::Suback),
            10 => Some(Self::Unsubscribe),
            11 => Some(Self::Unsuback),
            12 => Some(Self::Pingreq),
            13 => Some(Self::Pingresp),
            14 => Some(Self::Disconnect),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Publish {
    pub dup: bool,
    pub qos: u8,
    pub retain: bool,
    pub topic: String,
    pub packet_id: Option<u16>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // 解码完整变体；客户端/Broker 各取所需
pub enum Incoming {
    Connack { session_present: bool, return_code: u8 },
    Publish(Publish),
    Puback(u16),
    Pubrec(u16),
    Pubrel(u16),
    Pubcomp(u16),
    Suback { packet_id: u16, codes: Vec<u8> },
    Pingresp,
    Disconnect,
    /// Broker 侧收到的客户端 CONNECT（自检用）
    Connect { client_id: String, clean_session: bool, keep_alive: u16 },
    Subscribe {
        packet_id: u16,
        topics: Vec<(String, u8)>,
    },
    Unsubscribe {
        packet_id: u16,
        topics: Vec<String>,
    },
    Unsuback(u16),
    Pingreq,
}

pub fn encode_remaining_length(mut len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(4);
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
    out
}

pub fn decode_remaining_length(bytes: &[u8]) -> io::Result<(usize, usize)> {
    let mut multiplier = 1usize;
    let mut value = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if i >= 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MQTT remaining length 过长",
            ));
        }
        value += (b as usize & 0x7f) * multiplier;
        if b & 0x80 == 0 {
            return Ok((value, i + 1));
        }
        multiplier *= 128;
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "MQTT remaining length 不完整",
    ))
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.push((v >> 8) as u8);
    buf.push((v & 0xff) as u8);
}

fn put_str(buf: &mut Vec<u8>, s: &str) {
    put_u16(buf, s.len() as u16);
    buf.extend_from_slice(s.as_bytes());
}

fn take_u16(data: &[u8], off: &mut usize) -> io::Result<u16> {
    if *off + 2 > data.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "缺 u16"));
    }
    let v = u16::from_be_bytes([data[*off], data[*off + 1]]);
    *off += 2;
    Ok(v)
}

fn take_bytes(data: &[u8], off: &mut usize, n: usize) -> io::Result<Vec<u8>> {
    if *off + n > data.len() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "缺字节"));
    }
    let out = data[*off..*off + n].to_vec();
    *off += n;
    Ok(out)
}

fn take_str(data: &[u8], off: &mut usize) -> io::Result<String> {
    let n = take_u16(data, off)? as usize;
    let bytes = take_bytes(data, off, n)?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "MQTT 字符串非 UTF-8"))
}

fn wrap(fixed_header: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 4 + body.len());
    out.push(fixed_header);
    out.extend(encode_remaining_length(body.len()));
    out.extend_from_slice(body);
    out
}

pub fn encode_connect(client_id: &str, keep_alive_sec: u16, clean_session: bool) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(&mut body, "MQTT");
    body.push(4); // protocol level 3.1.1
    let mut flags = 0u8;
    if clean_session {
        flags |= 0x02;
    }
    body.push(flags);
    put_u16(&mut body, keep_alive_sec);
    put_str(&mut body, client_id);
    wrap(0x10, &body)
}

pub fn encode_connack(session_present: bool, return_code: u8) -> Vec<u8> {
    wrap(0x20, &[u8::from(session_present), return_code])
}

pub fn encode_publish(msg: &Publish) -> io::Result<Vec<u8>> {
    if msg.qos > 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "MQTT QoS 须为 0..2"));
    }
    if msg.qos > 0 && msg.packet_id.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "QoS>0 的 PUBLISH 需要 packet_id",
        ));
    }
    let mut body = Vec::new();
    put_str(&mut body, &msg.topic);
    if let Some(id) = msg.packet_id {
        put_u16(&mut body, id);
    }
    body.extend_from_slice(&msg.payload);
    let mut hdr = 0x30u8;
    if msg.dup {
        hdr |= 0x08;
    }
    hdr |= (msg.qos & 0x03) << 1;
    if msg.retain {
        hdr |= 0x01;
    }
    Ok(wrap(hdr, &body))
}

pub fn encode_subscribe(packet_id: u16, topic: &str, qos: u8) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    put_str(&mut body, topic);
    body.push(qos.min(2));
    wrap(0x82, &body) // SUBSCRIBE flags must be 0010
}

pub fn encode_suback(packet_id: u16, codes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    body.extend_from_slice(codes);
    wrap(0x90, &body)
}

pub fn encode_puback(packet_id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    wrap(0x40, &body)
}

pub fn encode_pubrec(packet_id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    wrap(0x50, &body)
}

pub fn encode_pubrel(packet_id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    wrap(0x62, &body) // flags 0010
}

pub fn encode_pubcomp(packet_id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    put_u16(&mut body, packet_id);
    wrap(0x70, &body)
}

pub fn encode_pingreq() -> Vec<u8> {
    vec![0xc0, 0x00]
}

pub fn encode_pingresp() -> Vec<u8> {
    vec![0xd0, 0x00]
}

pub fn encode_disconnect() -> Vec<u8> {
    vec![0xe0, 0x00]
}

/// 解析完整 MQTT 控制包（含固定头）。`Ok((consumed, packet))`。
pub fn decode_packet(buf: &[u8]) -> io::Result<Option<(usize, Incoming)>> {
    if buf.is_empty() {
        return Ok(None);
    }
    let fixed = buf[0];
    let (rem_len, rem_bytes) = match decode_remaining_length(&buf[1..]) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    };
    let header_len = 1 + rem_bytes;
    let total = header_len + rem_len;
    if buf.len() < total {
        return Ok(None);
    }
    let body = &buf[header_len..total];
    let packet_type = PacketType::from_u8(fixed >> 4).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("未知 MQTT 包类型 {}", fixed >> 4),
        )
    })?;
    let flags = fixed & 0x0f;
    let pkt = decode_body(packet_type, flags, body)?;
    Ok(Some((total, pkt)))
}

fn decode_body(ty: PacketType, flags: u8, body: &[u8]) -> io::Result<Incoming> {
    let mut off = 0usize;
    match ty {
        PacketType::Connect => {
            let proto = take_str(body, &mut off)?;
            if proto != "MQTT" && proto != "MQIsdp" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("不支持的 MQTT 协议名: {proto}"),
                ));
            }
            let _level = take_bytes(body, &mut off, 1)?[0];
            let connect_flags = take_bytes(body, &mut off, 1)?[0];
            let keep_alive = take_u16(body, &mut off)?;
            let client_id = take_str(body, &mut off)?;
            Ok(Incoming::Connect {
                client_id,
                clean_session: connect_flags & 0x02 != 0,
                keep_alive,
            })
        }
        PacketType::Connack => {
            if body.len() < 2 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "CONNACK 过短"));
            }
            Ok(Incoming::Connack {
                session_present: body[0] & 0x01 != 0,
                return_code: body[1],
            })
        }
        PacketType::Publish => {
            let dup = flags & 0x08 != 0;
            let qos = (flags >> 1) & 0x03;
            let retain = flags & 0x01 != 0;
            let topic = take_str(body, &mut off)?;
            let packet_id = if qos > 0 {
                Some(take_u16(body, &mut off)?)
            } else {
                None
            };
            let payload = body[off..].to_vec();
            Ok(Incoming::Publish(Publish {
                dup,
                qos,
                retain,
                topic,
                packet_id,
                payload,
            }))
        }
        PacketType::Puback => Ok(Incoming::Puback(take_u16(body, &mut off)?)),
        PacketType::Pubrec => Ok(Incoming::Pubrec(take_u16(body, &mut off)?)),
        PacketType::Pubrel => Ok(Incoming::Pubrel(take_u16(body, &mut off)?)),
        PacketType::Pubcomp => Ok(Incoming::Pubcomp(take_u16(body, &mut off)?)),
        PacketType::Subscribe => {
            let packet_id = take_u16(body, &mut off)?;
            let mut topics = Vec::new();
            while off < body.len() {
                let topic = take_str(body, &mut off)?;
                if off >= body.len() {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "SUBSCRIBE 缺 QoS"));
                }
                let qos = body[off];
                off += 1;
                topics.push((topic, qos));
            }
            Ok(Incoming::Subscribe { packet_id, topics })
        }
        PacketType::Suback => {
            let packet_id = take_u16(body, &mut off)?;
            Ok(Incoming::Suback {
                packet_id,
                codes: body[off..].to_vec(),
            })
        }
        PacketType::Unsubscribe => {
            let packet_id = take_u16(body, &mut off)?;
            let mut topics = Vec::new();
            while off < body.len() {
                topics.push(take_str(body, &mut off)?);
            }
            Ok(Incoming::Unsubscribe { packet_id, topics })
        }
        PacketType::Unsuback => Ok(Incoming::Unsuback(take_u16(body, &mut off)?)),
        PacketType::Pingreq => Ok(Incoming::Pingreq),
        PacketType::Pingresp => Ok(Incoming::Pingresp),
        PacketType::Disconnect => Ok(Incoming::Disconnect),
    }
}

/// 主题过滤器匹配（`+` / `#`）。
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    let mut f = filter.split('/').peekable();
    let mut t = topic.split('/').peekable();
    loop {
        match (f.next(), t.next()) {
            (None, None) => return true,
            (Some("#"), _) => return f.peek().is_none(),
            (Some("+"), Some(_)) => continue,
            (Some(a), Some(b)) if a == b => continue,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_length_roundtrip() {
        for n in [0usize, 127, 128, 16383, 16384] {
            let enc = encode_remaining_length(n);
            let (v, used) = decode_remaining_length(&enc).unwrap();
            assert_eq!(v, n);
            assert_eq!(used, enc.len());
        }
    }

    #[test]
    fn publish_qos0_roundtrip() {
        let msg = Publish {
            dup: false,
            qos: 0,
            retain: true,
            topic: "factory/line3/servo".into(),
            packet_id: None,
            payload: b"hello".to_vec(),
        };
        let bytes = encode_publish(&msg).unwrap();
        let (n, pkt) = decode_packet(&bytes).unwrap().unwrap();
        assert_eq!(n, bytes.len());
        match pkt {
            Incoming::Publish(p) => {
                assert_eq!(p.topic, "factory/line3/servo");
                assert_eq!(p.payload, b"hello");
                assert_eq!(p.qos, 0);
                assert!(p.retain);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn topic_filter_wildcards() {
        assert!(topic_matches("a/+/c", "a/b/c"));
        assert!(topic_matches("a/#", "a/b/c"));
        assert!(!topic_matches("a/+/c", "a/b/c/d"));
        assert!(!topic_matches("a/b", "a/x"));
    }
}
