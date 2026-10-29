//! 本地 WebSocket 控制面：ping / status / session.load|open|clear。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::fcap::{
    ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionReader, SessionWriter,
};
use crate::mqtt::{mqtt_qos_from_flags, mqtt_retain_from_flags};

use super::b64;
use super::json_util::{err_result, escape_str, extract_str, ok_result};
use super::ws::{
    accept_websocket, read_text_frame, write_text_frame as write_text_frame_server,
};

pub const DEFAULT_BIND: &str = "127.0.0.1:17890";
pub const API_VERSION: u32 = 1;
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
struct LoadedSession {
    source: String,
    start_wall_unix_us: u64,
    duration_us: u64,
    total_payload_bytes: u64,
    finalized: bool,
    meta_json: Option<String>,
    channels: Vec<ChannelEntry>,
    packets: Vec<PacketRecord>,
}

struct ControlState {
    session: Option<LoadedSession>,
}

impl ControlState {
    fn new() -> Self {
        Self { session: None }
    }
}

pub fn serve(bind: SocketAddr) -> io::Result<()> {
    let listener = TcpListener::bind(bind)?;
    eprintln!("FieldReplay 控制面监听 ws://{bind} （API v{API_VERSION}）");
    let state = Arc::new(Mutex::new(ControlState::new()));
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let st = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, st) {
                        eprintln!("控制面连接结束: {e}");
                    }
                });
            }
            Err(e) => eprintln!("接受连接失败: {e}"),
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<ControlState>>) -> io::Result<()> {
    accept_websocket(&mut stream)?;
    // 不主动推送 hello，避免与客户端握手读缓冲区粘包丢帧；由客户端发 ping。
    loop {
        let Some(msg) = read_text_frame(&mut stream)? else {
            return Ok(());
        };
        let reply = dispatch(&msg, &state);
        write_text_frame_server(&mut stream, &reply)?;
    }
}

fn dispatch(msg: &str, state: &Arc<Mutex<ControlState>>) -> String {
    let id = extract_str(msg, "id").unwrap_or_else(|| "0".into());
    let cmd = match extract_str(msg, "cmd") {
        Some(c) => c,
        None => return err_result(&id, "缺少 cmd"),
    };
    match cmd.as_str() {
        "ping" => ok_result(
            &id,
            &format!(
                "{{\"pong\":true,\"version\":\"{ENGINE_VERSION}\",\"api\":{API_VERSION}}}"
            ),
        ),
        "status" => match state.lock() {
            Ok(g) => ok_result(&id, &status_json(&g)),
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "session.clear" => match state.lock() {
            Ok(mut g) => {
                g.session = None;
                ok_result(&id, "{\"cleared\":true}")
            }
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "session.open" => {
            let path = match extract_str(msg, "path") {
                Some(p) if !p.is_empty() => p,
                _ => return err_result(&id, "session.open 需要 path"),
            };
            match load_from_path(&path) {
                Ok(loaded) => store_and_reply(&id, state, loaded),
                Err(e) => err_result(&id, &e.to_string()),
            }
        }
        "session.load" => {
            let b64s = match extract_str(msg, "b64") {
                Some(p) if !p.is_empty() => p,
                _ => return err_result(&id, "session.load 需要 b64"),
            };
            match load_from_b64(&b64s) {
                Ok(loaded) => store_and_reply(&id, state, loaded),
                Err(e) => err_result(&id, &e),
            }
        }
        other => err_result(&id, &format!("未知 cmd: {other}")),
    }
}

fn store_and_reply(
    id: &str,
    state: &Arc<Mutex<ControlState>>,
    loaded: LoadedSession,
) -> String {
    let result = session_to_json(&loaded);
    match state.lock() {
        Ok(mut g) => {
            g.session = Some(loaded);
            ok_result(id, &result)
        }
        Err(_) => err_result(id, "状态锁损坏"),
    }
}

fn status_json(g: &ControlState) -> String {
    match &g.session {
        None => "{\"state\":\"idle\",\"sessionLoaded\":false,\"packetCount\":0}".into(),
        Some(s) => format!(
            "{{\"state\":\"idle\",\"sessionLoaded\":true,\"packetCount\":{},\"source\":\"{}\"}}",
            s.packets.len(),
            escape_str(&s.source)
        ),
    }
}

fn load_from_path(path: &str) -> io::Result<LoadedSession> {
    let mut reader = SessionReader::open(path)?;
    let packets = reader.read_all_packets()?;
    Ok(LoadedSession {
        source: path.to_string(),
        start_wall_unix_us: reader.header().start_wall_unix_us,
        duration_us: reader.header().duration_us,
        total_payload_bytes: reader.header().total_payload_bytes,
        finalized: reader.header().is_finalized(),
        meta_json: reader.meta_json().map(|s| s.to_string()),
        channels: reader.channels().to_vec(),
        packets,
    })
}

fn load_from_b64(b64s: &str) -> Result<LoadedSession, String> {
    let bytes = b64::decode(b64s)?;
    let mut reader = SessionReader::open_bytes(bytes).map_err(|e| e.to_string())?;
    let packets = reader.read_all_packets().map_err(|e| e.to_string())?;
    Ok(LoadedSession {
        source: "upload".into(),
        start_wall_unix_us: reader.header().start_wall_unix_us,
        duration_us: reader.header().duration_us,
        total_payload_bytes: reader.header().total_payload_bytes,
        finalized: reader.header().is_finalized(),
        meta_json: reader.meta_json().map(|s| s.to_string()),
        channels: reader.channels().to_vec(),
        packets,
    })
}

fn session_to_json(s: &LoadedSession) -> String {
    let mut chs = String::from("[");
    for (i, c) in s.channels.iter().enumerate() {
        if i > 0 {
            chs.push(',');
        }
        let config = if c.config_json.trim().is_empty() {
            "{}"
        } else {
            c.config_json.as_str()
        };
        chs.push_str(&format!(
            "{{\"index\":{},\"type\":\"{}\",\"name\":\"{}\",\"config\":{}}}",
            c.channel_index,
            channel_type_str(c.channel_type),
            escape_str(&c.name),
            config
        ));
    }
    chs.push(']');

    let mut pkts = String::from("[");
    for (i, p) in s.packets.iter().enumerate() {
        if i > 0 {
            pkts.push(',');
        }
        let ch_ty = s
            .channels
            .iter()
            .find(|c| c.channel_index == p.channel_index)
            .map(|c| c.channel_type);
        let topic = if ch_ty == Some(ChannelType::Mqtt) {
            String::from_utf8_lossy(&p.aux).into_owned()
        } else {
            String::new()
        };
        let qos = mqtt_qos_from_flags(p.flags);
        let retain = mqtt_retain_from_flags(p.flags);
        pkts.push_str(&format!(
            "{{\"i\":{i},\"us\":{},\"ch\":{},\"kind\":{},\"dir\":{},\"flags\":{},\"topic\":\"{}\",\"qos\":{qos},\"retain\":{},\"auxB64\":\"{}\",\"payloadB64\":\"{}\"}}",
            p.relative_us,
            p.channel_index,
            packet_kind_u8(p.kind),
            p.direction,
            p.flags,
            escape_str(&topic),
            if retain { "true" } else { "false" },
            b64::encode(&p.aux),
            b64::encode(&p.payload),
        ));
    }
    pkts.push(']');

    let meta = s
        .meta_json
        .as_deref()
        .map(|m| m.to_string())
        .unwrap_or_else(|| "null".into());
    let meta_field = if meta == "null" || (meta.starts_with('{') && meta.ends_with('}')) {
        meta
    } else {
        format!("\"{}\"", escape_str(&meta))
    };

    format!(
        "{{\"source\":\"{}\",\"startWallUnixUs\":{},\"durationUs\":{},\"totalPayloadBytes\":{},\"finalized\":{},\"meta\":{meta_field},\"channels\":{chs},\"packets\":{pkts}}}",
        escape_str(&s.source),
        s.start_wall_unix_us,
        s.duration_us,
        s.total_payload_bytes,
        if s.finalized { "true" } else { "false" },
    )
}

fn channel_type_str(t: ChannelType) -> &'static str {
    match t {
        ChannelType::Udp => "udp",
        ChannelType::Serial => "serial",
        ChannelType::Tcp => "tcp",
        ChannelType::Mqtt => "mqtt",
        ChannelType::Video => "video_stream",
    }
}

fn packet_kind_u8(k: PacketKind) -> u8 {
    match k {
        PacketKind::Data => 0,
        PacketKind::TcpConnect => 1,
        PacketKind::TcpDisconnect => 2,
        PacketKind::Marker => 3,
    }
}

/// 自检：临时目录写一小包 → serve 环回 ping + session.open。
pub fn self_check_control() -> io::Result<()> {
    let dir = std::env::temp_dir().join("fieldreplay_e5a_ctrl");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("sample.fcap");
    {
        let channels = [ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "udp-demo".into(),
            config_json: r#"{"bind":"127.0.0.1:9"}"#.into(),
        }];
        let mut w = SessionWriter::create(&path, 1_700_000_000_000_000, &channels, None)?;
        w.append_packet(&PacketRecord {
            relative_us: 1000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:1".to_vec(),
            payload: b"hello-e5a".to_vec(),
        })?;
        w.finalize()?;
    }

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let state = Arc::new(Mutex::new(ControlState::new()));
    let st = Arc::clone(&state);
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let _ = handle_client(stream, st);
        }
    });

    thread::sleep(Duration::from_millis(50));
    let mut client = TcpStream::connect(addr)?;
    client.set_read_timeout(Some(Duration::from_secs(3)))?;
    client.set_write_timeout(Some(Duration::from_secs(3)))?;
    ws_client_handshake(&mut client)?;

    write_text_frame_client(&mut client, r#"{"id":"1","cmd":"ping"}"#)?;
    let pong = read_text_frame(&mut client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 ping 响应")
    })?;
    if !pong.contains("\"pong\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ping 失败: {pong}"),
        ));
    }

    let open = format!(
        r#"{{"id":"2","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(&path)
    );
    write_text_frame_client(&mut client, &open)?;
    let loaded = read_text_frame(&mut client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 open 响应")
    })?;
    let expect_b64 = b64::encode(b"hello-e5a");
    if !loaded.contains("\"ok\":true") || !loaded.contains(&expect_b64) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session.open 失败: {loaded}"),
        ));
    }

    eprintln!("  控制面 WebSocket ping + session.open：通过");
    Ok(())
}

fn escape_path_for_json(path: &Path) -> String {
    escape_str(&path.to_string_lossy())
}

fn ws_client_handshake(stream: &mut TcpStream) -> io::Result<()> {
    let key = b64::encode(b"fieldreplay-e5a-key!");
    let req = format!(
        "GET / HTTP/1.1\r\n\
Host: 127.0.0.1\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: {key}\r\n\
Sec-WebSocket-Version: 13\r\n\
\r\n"
    );
    stream.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 256];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "握手响应中断",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let resp = String::from_utf8_lossy(&buf);
    if !resp.starts_with("HTTP/1.1 101") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("握手失败: {resp}"),
        ));
    }
    Ok(())
}

fn write_text_frame_client(stream: &mut TcpStream, text: &str) -> io::Result<()> {
    let payload = text.as_bytes();
    let mut out = Vec::with_capacity(14 + payload.len());
    out.push(0x81);
    let mask = [0x11u8, 0x22, 0x33, 0x44];
    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        out.push(b ^ mask[i % 4]);
    }
    stream.write_all(&out)
}
