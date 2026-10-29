//! 本地 WebSocket 控制面：ping / status / session.* / record.* / replay.*。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionReader, SessionWriter};

use super::b64;
use super::dto::{session_to_json, status_json};
use super::hub::PushHub;
use super::json_util::{err_result, escape_str, extract_object, extract_str, ok_result};
use super::live::{handle_record_start, handle_record_stop};
use super::replay_live::{handle_replay_start, handle_replay_stop};
use super::session_file::export_session_bytes;
use super::state::{ControlState, LoadedSession};
use super::ws::{
    accept_websocket, read_text_frame, write_text_frame as write_text_frame_server,
};

pub const DEFAULT_BIND: &str = "127.0.0.1:17890";
pub const API_VERSION: u32 = 1;
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn serve(bind: SocketAddr) -> io::Result<()> {
    let listener = TcpListener::bind(bind)?;
    eprintln!("FieldReplay 控制面监听 ws://{bind} （API v{API_VERSION}）");
    let state = Arc::new(Mutex::new(ControlState::new()));
    let hub = Arc::new(PushHub::new());
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let st = Arc::clone(&state);
                let hb = Arc::clone(&hub);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, st, hb) {
                        eprintln!("控制面连接结束: {e}");
                    }
                });
            }
            Err(e) => eprintln!("接受连接失败: {e}"),
        }
    }
    Ok(())
}

fn handle_client(
    mut stream: TcpStream,
    state: Arc<Mutex<ControlState>>,
    hub: Arc<PushHub>,
) -> io::Result<()> {
    accept_websocket(&mut stream)?;
    let mut writer = stream.try_clone()?;
    let (tx, rx) = mpsc::sync_channel::<String>(256);
    let client_id = hub.register(tx.clone());

    let writer_thread = thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            if write_text_frame_server(&mut writer, &msg).is_err() {
                break;
            }
        }
    });

    let result = (|| -> io::Result<()> {
        // 不主动推送 hello，避免与客户端握手读缓冲区粘包丢帧；由客户端发 ping。
        loop {
            let Some(msg) = read_text_frame(&mut stream)? else {
                return Ok(());
            };
            let reply = dispatch(&msg, &state, &hub);
            if tx.send(reply).is_err() {
                return Ok(());
            }
        }
    })();

    hub.unregister(client_id);
    drop(tx);
    let _ = writer_thread.join();
    result
}

fn dispatch(msg: &str, state: &Arc<Mutex<ControlState>>, hub: &Arc<PushHub>) -> String {
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
            Ok(mut g) => {
                g.reap_finished_replay();
                ok_result(&id, &status_json(&g))
            }
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "session.clear" => match state.lock() {
            Ok(mut g) => {
                g.reap_finished_replay();
                if g.recording.is_some() {
                    return err_result(&id, "录制中无法清空会话，请先 record.stop");
                }
                if g.replaying.is_some() {
                    return err_result(&id, "重放中无法清空会话，请先 replay.stop");
                }
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
        "session.export" => handle_session_export(&id, msg, state),
        "record.start" => match state.lock() {
            Ok(mut g) => {
                g.reap_finished_replay();
                if g.replaying.is_some() {
                    return err_result(&id, "重放中无法录制，请先 replay.stop");
                }
                handle_record_start(&id, msg, &mut g.recording, hub)
            }
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "record.stop" => {
            // 先取出 ActiveRecording，join 时不持状态锁，避免推送路径间接等待
            let mut recording = match state.lock() {
                Ok(mut g) => g.recording.take(),
                Err(_) => return err_result(&id, "状态锁损坏"),
            };
            let mut session_tmp = None;
            let reply = handle_record_stop(&id, &mut recording, &mut session_tmp);
            if let Ok(mut g) = state.lock() {
                g.recording = recording;
                if let Some(s) = session_tmp {
                    g.session = Some(s);
                }
            }
            reply
        }
        "replay.start" => match state.lock() {
            Ok(mut g) => {
                g.reap_finished_replay();
                handle_replay_start(&id, msg, &mut g, hub)
            }
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "replay.stop" => {
            let mut replaying = match state.lock() {
                Ok(mut g) => {
                    g.reap_finished_replay();
                    g.replaying.take()
                }
                Err(_) => return err_result(&id, "状态锁损坏"),
            };
            let reply = handle_replay_stop(&id, &mut replaying);
            // stop 已 take 并 join；槽位保持空
            let _ = replaying;
            reply
        }
        other => err_result(&id, &format!("未知 cmd: {other}")),
    }
}

/// 将内存会话写成二进制 `.fcap`，经 base64 回传（浏览器可下载）。
/// 可选 `meta` 对象覆盖 SessionMeta；录制/重放中拒绝。
fn handle_session_export(
    id: &str,
    msg: &str,
    state: &Arc<Mutex<ControlState>>,
) -> String {
    let meta_override = extract_object(msg, "meta");
    match state.lock() {
        Ok(mut g) => {
            g.reap_finished_replay();
            if g.recording.is_some() {
                return err_result(id, "录制中无法导出，请先 record.stop");
            }
            if g.replaying.is_some() {
                return err_result(id, "重放中无法导出，请先 replay.stop");
            }
            let Some(session) = g.session.as_mut() else {
                return err_result(id, "无会话可导出，请先录制或 session.open/load");
            };
            if session.packets.is_empty() {
                return err_result(id, "会话无帧，无法导出");
            }
            if let Some(ref m) = meta_override {
                session.meta_json = Some(m.clone());
            }
            match export_session_bytes(session, session.meta_json.as_deref()) {
                Ok((bytes, count)) => {
                    if bytes.len() < 4 || &bytes[..4] != b"FCAP" {
                        return err_result(id, "导出校验失败：非 FCAP 魔数");
                    }
                    let b64s = b64::encode(&bytes);
                    ok_result(
                        id,
                        &format!(
                            "{{\"exported\":true,\"packetCount\":{count},\"bytes\":{},\"b64\":\"{b64s}\"}}",
                            bytes.len()
                        ),
                    )
                }
                Err(e) => err_result(id, &format!("导出失败: {e}")),
            }
        }
        Err(_) => err_result(id, "状态锁损坏"),
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
            g.reap_finished_replay();
            if g.recording.is_some() {
                return err_result(id, "录制中无法载入会话");
            }
            if g.replaying.is_some() {
                return err_result(id, "重放中无法载入会话");
            }
            g.session = Some(loaded);
            ok_result(id, &result)
        }
        Err(_) => err_result(id, "状态锁损坏"),
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

/// 自检：临时目录写一小包 → serve 环回 ping + session.open + record.* + replay.*。
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
        w.append_packet(&PacketRecord {
            relative_us: 50_000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:1".to_vec(),
            payload: b"hello-e5c".to_vec(),
        })?;
        w.finalize()?;
    }

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let state = Arc::new(Mutex::new(ControlState::new()));
    let hub = Arc::new(PushHub::new());
    let st = Arc::clone(&state);
    let hb = Arc::clone(&hub);
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let _ = handle_client(stream, st, hb);
        }
    });

    thread::sleep(Duration::from_millis(50));
    let mut client = TcpStream::connect(addr)?;
    client.set_read_timeout(Some(Duration::from_secs(5)))?;
    client.set_write_timeout(Some(Duration::from_secs(5)))?;
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

    // E5d：导出二进制 .fcap（魔数 + 帧数往返）
    self_check_export_via_ws(&mut client)?;

    // E5c：先测重放（会话仍是 sample.fcap），再测录制（会覆盖内存会话）
    self_check_replay_via_ws(&mut client, &path)?;
    self_check_record_via_ws(&mut client)?;
    Ok(())
}

fn self_check_export_via_ws(client: &mut TcpStream) -> io::Result<()> {
    write_text_frame_client(
        client,
        r#"{"id":"e5d","cmd":"session.export","meta":{"session_title":"e5d-export","notes":"self-check"}}"#,
    )?;
    let exported = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 session.export 响应")
    })?;
    if !exported.contains("\"ok\":true") || !exported.contains("\"exported\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session.export 失败: {exported}"),
        ));
    }
    let b64s = extract_str(&exported, "b64").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "session.export 缺 b64")
    })?;
    let bytes = b64::decode(&b64s).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("export b64 无效: {e}"))
    })?;
    if bytes.len() < 4 || &bytes[..4] != b"FCAP" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "export 字节非 FCAP 魔数",
        ));
    }
    let mut reader = SessionReader::open_bytes(bytes)?;
    let packets = reader.read_all_packets()?;
    if packets.len() != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("export 往返帧数不对: {}", packets.len()),
        ));
    }
    let meta = reader.meta_json().unwrap_or("");
    if !meta.contains("e5d-export") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("export meta 未写入: {meta}"),
        ));
    }
    eprintln!("  控制面 session.export 二进制往返：通过");
    Ok(())
}

fn self_check_replay_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let recv = UdpSocket::bind("127.0.0.1:0")?;
    recv.set_read_timeout(Some(Duration::from_millis(500)))?;
    let target = recv.local_addr()?;

    // 确保会话仍是 sample（录制自检会换掉）
    let open = format!(
        r#"{{"id":"r0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 session.open 响应")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("replay 前 session.open 失败: {opened}"),
        ));
    }

    let start = format!(
        r#"{{"id":"r1","cmd":"replay.start","type":"udp","config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        target.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 replay.start 响应")
    })?;
    if !started.contains("\"ok\":true") || !started.contains("\"started\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("replay.start 失败: {started}"),
        ));
    }

    let mut got = 0u32;
    let mut saw_progress = false;
    let mut saw_done = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    client.set_read_timeout(Some(Duration::from_millis(200)))?;
    while std::time::Instant::now() < deadline && (!saw_done || got < 2) {
        let mut buf = [0u8; 256];
        match recv.recv_from(&mut buf) {
            Ok((n, _)) => {
                if n > 0 {
                    got += 1;
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"replay.progress\"") {
                    saw_progress = true;
                }
                if frame.contains("\"event\":\"replay.done\"") {
                    saw_done = true;
                }
            }
            Ok(None) => break,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
    }
    client.set_read_timeout(Some(Duration::from_secs(5)))?;

    // 若自然结束未到，主动 stop
    if !saw_done {
        write_text_frame_client(client, r#"{"id":"r2","cmd":"replay.stop"}"#)?;
        for _ in 0..16 {
            let frame = read_text_frame(client)?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 replay.stop 响应")
            })?;
            if frame.contains("\"id\":\"r2\"") {
                if !frame.contains("\"ok\":true") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("replay.stop 失败: {frame}"),
                    ));
                }
                break;
            }
            if frame.contains("\"event\":\"replay.progress\"") {
                saw_progress = true;
            }
            if frame.contains("\"event\":\"replay.done\"") {
                saw_done = true;
            }
        }
    } else {
        // 自然结束后槽位应已空；再 stop 应报错或视为空闲——发 status 即可
        write_text_frame_client(client, r#"{"id":"r3","cmd":"status"}"#)?;
        let st = read_text_frame(client)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 status 响应")
        })?;
        if !st.contains("\"ok\":true") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("status 失败: {st}"),
            ));
        }
    }

    if got < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("期望至少收到 2 个 UDP 重放包，实际 {got}；progress={saw_progress} done={saw_done}"),
        ));
    }

    eprintln!("  控制面 replay.start UDP 环回：通过");
    Ok(())
}

fn self_check_record_via_ws(client: &mut TcpStream) -> io::Result<()> {
    let probe = UdpSocket::bind("127.0.0.1:0")?;
    let udp_addr = probe.local_addr()?;
    drop(probe);

    let out = std::env::temp_dir()
        .join("fieldreplay_e5b_live")
        .join("live.fcap");
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    let _ = std::fs::remove_file(&out);

    let start = format!(
        r#"{{"id":"3","cmd":"record.start","type":"udp","name":"live-udp","sessionTitle":"e5b","out":"{}","config":{{"bind_ip":"127.0.0.1","bind_port":{}}}}}"#,
        escape_path_for_json(&out),
        udp_addr.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 record.start 响应")
    })?;
    if !started.contains("\"ok\":true") || !started.contains("\"started\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record.start 失败: {started}"),
        ));
    }

    thread::sleep(Duration::from_millis(100));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    for i in 0..3u8 {
        sender.send_to(&[b'L', b'I', b'V', b'E', i], udp_addr)?;
        thread::sleep(Duration::from_millis(40));
    }

    // 收取预览推送（可能夹杂在 stop 之前）
    let mut saw_packet_event = false;
    client.set_read_timeout(Some(Duration::from_millis(300)))?;
    for _ in 0..8 {
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"record.packet\"") {
                    saw_packet_event = true;
                }
            }
            Ok(None) => break,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(e) => return Err(e),
        }
    }
    client.set_read_timeout(Some(Duration::from_secs(5)))?;

    write_text_frame_client(client, r#"{"id":"4","cmd":"record.stop"}"#)?;
    // stop 响应前可能还有残留推送
    let mut stop_reply = String::new();
    for _ in 0..16 {
        let frame = read_text_frame(client)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 record.stop 响应")
        })?;
        if frame.contains("\"id\":\"4\"") {
            stop_reply = frame;
            break;
        }
        if frame.contains("\"event\":\"record.packet\"") {
            saw_packet_event = true;
        }
    }
    if !stop_reply.contains("\"ok\":true") || !stop_reply.contains("\"stopped\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record.stop 失败: {stop_reply}"),
        ));
    }
    if !saw_packet_event {
        // 推送可能因调度错过；至少校验落盘帧数
        if !stop_reply.contains("\"packetCount\":") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "record.stop 未返回 packetCount",
            ));
        }
    }
    // 期望至少收到若干帧
    if let Some(n) = extract_packet_count(&stop_reply) {
        if n < 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("期望至少 3 帧，实际 {n}；推送={saw_packet_event}"),
            ));
        }
    }

    eprintln!("  控制面 record.start/stop UDP 环回：通过");
    Ok(())
}

fn extract_packet_count(json: &str) -> Option<u64> {
    super::json_util::extract_u64(json, "packetCount")
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
