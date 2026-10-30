//! 本地 WebSocket 控制面：ping / status / session.* / record.* / replay.* / serial.list；可选 HTTP 静态 UI。
//! E13b：`session.addMarker`；E13c：`session.trim`；E14c：`session.importPcap`；E16b/d：`record.pause`/`resume`。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionReader, SessionWriter};

use super::b64;
use super::dto::{session_to_json, status_json};
use super::hub::PushHub;
use super::http_static::{self, get_path, is_head, serve_file, serve_no_ui};
use super::json_util::{
    err_result, escape_str, extract_object, extract_str, extract_u64, ok_result,
};
use super::import_pcap::handle_session_import_pcap;
use super::live::{
    handle_record_pause, handle_record_resume, handle_record_start, handle_record_stop,
};
use super::marker::handle_session_add_marker;
use super::replay_live::{
    handle_replay_pause, handle_replay_resume, handle_replay_seek, handle_replay_start,
    handle_replay_stop,
};
use super::session_file::{export_session_bytes, export_session_pcap_bytes};
use super::trim::handle_session_trim;
use super::state::{ControlState, LoadedSession};
use super::ws::{
    complete_websocket_upgrade, is_websocket_upgrade, read_http_headers, read_text_frame,
    write_text_frame as write_text_frame_server,
};

pub const DEFAULT_BIND: &str = "127.0.0.1:17890";
pub const API_VERSION: u32 = 1;
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn serve(bind: SocketAddr, ui_dir: Option<PathBuf>) -> io::Result<()> {
    let listener = TcpListener::bind(bind)?;
    match &ui_dir {
        Some(dir) => eprintln!(
            "FieldReplay 控制面监听 http://{bind}/ 与 ws://{bind} （API v{API_VERSION}；UI={dir}）",
            dir = dir.display()
        ),
        None => eprintln!("FieldReplay 控制面监听 ws://{bind} （API v{API_VERSION}；未挂载 UI）"),
    }
    let state = Arc::new(Mutex::new(ControlState::new()));
    let hub = Arc::new(PushHub::new());
    let ui = Arc::new(ui_dir);
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let st = Arc::clone(&state);
                let hb = Arc::clone(&hub);
                let ui_dir = Arc::clone(&ui);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, st, hb, ui_dir.as_ref().as_ref()) {
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
    ui_dir: Option<&PathBuf>,
) -> io::Result<()> {
    let req = read_http_headers(&mut stream)?;
    if is_websocket_upgrade(&req) {
        complete_websocket_upgrade(&mut stream, &req)?;
        return handle_websocket(stream, state, hub);
    }
    if let Some(path) = get_path(&req) {
        let head_only = is_head(&req);
        return match ui_dir {
            Some(dir) => serve_file(&mut stream, dir, &path, head_only),
            None => serve_no_ui(&mut stream, head_only),
        };
    }
    let _ = http_static::write_response(
        &mut stream,
        400,
        "Bad Request",
        "text/plain; charset=utf-8",
        b"expected WebSocket upgrade or GET",
        false,
    );
    Ok(())
}

fn handle_websocket(
    mut stream: TcpStream,
    state: Arc<Mutex<ControlState>>,
    hub: Arc<PushHub>,
) -> io::Result<()> {
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
        "session.exportPcap" => handle_session_export_pcap(&id, state),
        "session.importPcap" => handle_session_import_pcap(&id, msg, state),
        "session.addMarker" => handle_session_add_marker(&id, msg, state),
        "session.trim" => handle_session_trim(&id, msg, state),
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
        "record.pause" => match state.lock() {
            Ok(mut g) => handle_record_pause(&id, &mut g),
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "record.resume" => match state.lock() {
            Ok(mut g) => handle_record_resume(&id, &mut g),
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "replay.start" => match state.lock() {
            Ok(mut g) => {
                g.reap_finished_replay();
                handle_replay_start(&id, msg, &mut g, hub)
            }
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "replay.pause" => match state.lock() {
            Ok(mut g) => handle_replay_pause(&id, &mut g),
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "replay.resume" => match state.lock() {
            Ok(mut g) => handle_replay_resume(&id, &mut g),
            Err(_) => err_result(&id, "状态锁损坏"),
        },
        "replay.seek" => match state.lock() {
            Ok(mut g) => handle_replay_seek(&id, msg, &mut g),
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
        "serial.list" => match crate::serial::list_ports() {
            Ok(ports) => {
                let mut arr = String::from("[");
                for (i, p) in ports.iter().enumerate() {
                    if i > 0 {
                        arr.push(',');
                    }
                    arr.push('"');
                    arr.push_str(&escape_str(p));
                    arr.push('"');
                }
                arr.push(']');
                ok_result(&id, &format!("{{\"ports\":{arr}}}"))
            }
            Err(e) => err_result(&id, &e.to_string()),
        },
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

/// 将内存会话写成经典 `.pcap`，经 base64 回传；门禁同 `session.export`。
fn handle_session_export_pcap(id: &str, state: &Arc<Mutex<ControlState>>) -> String {
    match state.lock() {
        Ok(mut g) => {
            g.reap_finished_replay();
            if g.recording.is_some() {
                return err_result(id, "录制中无法导出，请先 record.stop");
            }
            if g.replaying.is_some() {
                return err_result(id, "重放中无法导出，请先 replay.stop");
            }
            let Some(session) = g.session.as_ref() else {
                return err_result(id, "无会话可导出，请先录制或 session.open/load");
            };
            if session.packets.is_empty() {
                return err_result(id, "会话无帧，无法导出");
            }
            match export_session_pcap_bytes(session) {
                Ok((bytes, stats)) => {
                    if bytes.len() < 4 {
                        return err_result(id, "导出校验失败：pcap 过短");
                    }
                    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    if magic != crate::pcap::PCAP_MAGIC_USEC {
                        return err_result(id, "导出校验失败：非 pcap 魔数");
                    }
                    if stats.written == 0 {
                        return err_result(
                            id,
                            "无可导出的网络帧（仅串口/MQTT 或无 data 帧时无法生成 pcap）",
                        );
                    }
                    let b64s = b64::encode(&bytes);
                    ok_result(
                        id,
                        &format!(
                            "{{\"exported\":true,\"written\":{},\"skipped\":{},\"bytes\":{},\"b64\":\"{b64s}\"}}",
                            stats.written,
                            stats.skipped,
                            bytes.len()
                        ),
                    )
                }
                Err(e) => err_result(id, &format!("pcap 导出失败: {e}")),
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
            let _ = handle_client(stream, st, hb, None);
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

    write_text_frame_client(&mut client, r#"{"id":"1b","cmd":"serial.list"}"#)?;
    let ports_resp = read_text_frame(&mut client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 serial.list 响应")
    })?;
    if !ports_resp.contains("\"ok\":true") || !ports_resp.contains("\"ports\":") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("serial.list 失败: {ports_resp}"),
        ));
    }
    eprintln!("  控制面 serial.list：通过");

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
    self_check_replay_pause_via_ws(&mut client, &path)?;
    self_check_replay_seek_via_ws(&mut client, &path)?;
    self_check_replay_fault_via_ws(&mut client, &path)?;
    self_check_replay_loop_via_ws(&mut client, &path)?;
    // E13b：离线插 marker（放在重放自检之后，避免改内存会话干扰）
    self_check_add_marker_via_ws(&mut client, &path)?;
    // E13c：时间窗 trim（独立多通道样本，不污染后续录制自检）
    self_check_trim_via_ws(&mut client)?;
    // E14c：pcap 导入（独立样本；默认替换内存会话）
    self_check_import_pcap_via_ws(&mut client)?;
    self_check_record_via_ws(&mut client)?;
    // E16b：录制 pause/resume + 相对戳冻结
    self_check_record_pause_via_ws(&mut client)?;
    // E16d：多路共享轴 + 暂停期无推送/无落盘
    self_check_record_pause_multi_via_ws(&mut client)?;
    // R2b：控制面 multi targets[]（UDP+TCP）环回
    self_check_replay_multi_targets_via_ws(&mut client)?;
    // E6a：同端口静态 UI
    self_check_http_static()?;
    Ok(())
}

/// E14c：写出小 pcap → `session.importPcap` → 校验统计 / FCAP b64 / 内存会话。
fn self_check_import_pcap_via_ws(client: &mut TcpStream) -> io::Result<()> {
    use crate::pcap::session_to_pcap_bytes;

    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Udp,
        name: "udp0".into(),
        config_json: r#"{"bind_ip":"127.0.0.1","bind_port":9000}"#.into(),
    }];
    let packets = [PacketRecord {
        relative_us: 0,
        channel_index: 0,
        kind: PacketKind::Data,
        direction: 0,
        flags: 0,
        aux: b"10.0.0.2:5000".to_vec(),
        payload: b"e14c-ws".to_vec(),
    }];
    let (pcap, stats) = session_to_pcap_bytes(1_800_000_000_000_000, &channels, &packets)?;
    if stats.written != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("E14c 自检导出 written 期望 1，得 {}", stats.written),
        ));
    }
    let pcap_b64 = b64::encode(&pcap);

    write_text_frame_client(
        client,
        &format!(
            r#"{{"id":"e14c1","cmd":"session.importPcap","b64":"{pcap_b64}","sourceHint":"e14c.pcap"}}"#
        ),
    )?;
    let imported = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 session.importPcap 响应")
    })?;
    if !imported.contains("\"ok\":true")
        || !imported.contains("\"imported\":true")
        || !imported.contains("\"written\":1")
        || !imported.contains("\"channelCount\":1")
        || !imported.contains("\"replaced\":true")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session.importPcap 失败: {imported}"),
        ));
    }
    let expect_payload = b64::encode(b"e14c-ws");
    if !imported.contains(&expect_payload) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("import 响应缺 payload: {imported}"),
        ));
    }
    let fcap_b64 = extract_str(&imported, "b64").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "session.importPcap 缺 b64")
    })?;
    let fcap_bytes = b64::decode(&fcap_b64).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("import fcap b64 无效: {e}"))
    })?;
    if fcap_bytes.len() < 4 || &fcap_bytes[..4] != b"FCAP" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "import 回传非 FCAP",
        ));
    }
    let mut reader = SessionReader::open_bytes(fcap_bytes)?;
    let got = reader.read_all_packets()?;
    if got.len() != 1 || got[0].payload.as_slice() != b"e14c-ws" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("import 写出 .fcap 帧不符: len={}", got.len()),
        ));
    }

    // 无可导入帧应失败
    let mut empty = Vec::new();
    empty.extend_from_slice(&crate::pcap::PCAP_MAGIC_USEC.to_le_bytes());
    empty.extend_from_slice(&2u16.to_le_bytes());
    empty.extend_from_slice(&4u16.to_le_bytes());
    empty.extend_from_slice(&0i32.to_le_bytes());
    empty.extend_from_slice(&0u32.to_le_bytes());
    empty.extend_from_slice(&65535u32.to_le_bytes());
    empty.extend_from_slice(&1u32.to_le_bytes()); // LINKTYPE_ETHERNET
    let empty_b64 = b64::encode(&empty);
    write_text_frame_client(
        client,
        &format!(r#"{{"id":"e14c-empty","cmd":"session.importPcap","b64":"{empty_b64}"}}"#),
    )?;
    let empty_resp = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到空 pcap import 响应")
    })?;
    if empty_resp.contains("\"ok\":true") || !empty_resp.contains("无可导入") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("空 pcap 应失败: {empty_resp}"),
        ));
    }

    eprintln!("  控制面 session.importPcap 写读：通过");
    Ok(())
}

fn self_check_add_marker_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let open = format!(
        r#"{{"id":"e13b0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 addMarker 前 open 响应")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("addMarker 前 session.open 失败: {opened}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"e13b1","cmd":"session.addMarker","text":"e13b-mark","relativeUs":25000,"ch":0}"#,
    )?;
    let added = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 session.addMarker 响应")
    })?;
    if !added.contains("\"ok\":true")
        || !added.contains("\"added\":true")
        || !added.contains("e13b-mark")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session.addMarker 失败: {added}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"e13b2","cmd":"session.export","meta":{"session_title":"e13b-marker"}}"#,
    )?;
    let exported = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 addMarker 后 export 响应")
    })?;
    if !exported.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("addMarker 后 export 失败: {exported}"),
        ));
    }
    let b64s = extract_str(&exported, "b64").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "addMarker export 缺 b64")
    })?;
    let bytes = b64::decode(&b64s).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("addMarker export b64 无效: {e}"))
    })?;
    let mut reader = SessionReader::open_bytes(bytes)?;
    let packets = reader.read_all_packets()?;
    let marker = packets
        .iter()
        .find(|p| p.kind == PacketKind::Marker)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "导出结果中无 marker 帧")
        })?;
    if marker.aux.as_slice() != b"e13b-mark" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "marker aux 不一致: {:?}",
                String::from_utf8_lossy(&marker.aux)
            ),
        ));
    }
    if marker.relative_us != 25_000 || !marker.payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "marker 字段异常: relative={} payload={}",
                marker.relative_us,
                marker.payload.len()
            ),
        ));
    }
    // 恢复干净 sample，避免后续重放自检吃到 marker
    write_text_frame_client(client, &open)?;
    let _ = read_text_frame(client)?;

    eprintln!("  控制面 session.addMarker 离线写读：通过");
    Ok(())
}

/// E13c：多通道 + marker 会话 → `session.trim` → 读回校验再基与通道表。
fn self_check_trim_via_ws(client: &mut TcpStream) -> io::Result<()> {
    let dir = std::env::temp_dir().join("fieldreplay_e13c_trim");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("multi.fcap");
    {
        let channels = [
            ChannelEntry {
                channel_index: 0,
                channel_type: ChannelType::Udp,
                name: "udp0".into(),
                config_json: r#"{"bind":"127.0.0.1:9"}"#.into(),
            },
            ChannelEntry {
                channel_index: 1,
                channel_type: ChannelType::Serial,
                name: "ser1".into(),
                config_json: r#"{"port_name":"COM9","baud_rate":115200}"#.into(),
            },
        ];
        let mut w = SessionWriter::create(
            &path,
            2_000_000,
            &channels,
            Some(r#"{"session_title":"e13c","notes":"pre"}"#),
        )?;
        for (rel, ch, kind, aux, payload) in [
            (0u64, 0u16, PacketKind::Data, &b""[..], &b"a"[..]),
            (10_000, 1, PacketKind::Data, &b""[..], &b"bb"[..]),
            (20_000, 0, PacketKind::Marker, &b"cut-me"[..], &b""[..]),
            (40_000, 0, PacketKind::Data, &b""[..], &b"ccc"[..]),
            (50_000, 1, PacketKind::Data, &b""[..], &b"d"[..]),
        ] {
            w.append_packet(&PacketRecord {
                relative_us: rel,
                channel_index: ch,
                kind,
                direction: 0,
                flags: 0,
                aux: aux.to_vec(),
                payload: payload.to_vec(),
            })?;
        }
        w.finalize()?;
    }

    let open = format!(
        r#"{{"id":"e13c0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(&path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 trim 前 open 响应")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("trim 前 session.open 失败: {opened}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"e13c-bad","cmd":"session.trim","fromUs":40,"toUs":10}"#,
    )?;
    let bad = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到非法窗 trim 响应")
    })?;
    if bad.contains("\"ok\":true") || !bad.contains("fromUs") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("非法时间窗应失败: {bad}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"e13c-empty","cmd":"session.trim","fromUs":60000,"toUs":70000}"#,
    )?;
    let empty = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到空窗 trim 响应")
    })?;
    if empty.contains("\"ok\":true") || !empty.contains("无帧") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("空时间窗应失败: {empty}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"e13c1","cmd":"session.trim","fromUs":10000,"toUs":40000}"#,
    )?;
    let trimmed = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "未收到 session.trim 响应")
    })?;
    if !trimmed.contains("\"ok\":true")
        || !trimmed.contains("\"trimmed\":true")
        || !trimmed.contains("\"packetCount\":3")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session.trim 失败: {trimmed}"),
        ));
    }
    let b64s = extract_str(&trimmed, "b64").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "session.trim 缺 b64")
    })?;
    let bytes = b64::decode(&b64s).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("trim b64 无效: {e}"))
    })?;
    let mut reader = SessionReader::open_bytes(bytes)?;
    if reader.header().start_wall_unix_us != 2_010_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "墙钟起点未按 fromUs 偏移: {}",
                reader.header().start_wall_unix_us
            ),
        ));
    }
    if reader.channels().len() != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("通道表应完整保留 2 路，实际 {}", reader.channels().len()),
        ));
    }
    let packets = reader.read_all_packets()?;
    if packets.len() != 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("trim 后帧数应为 3，实际 {}", packets.len()),
        ));
    }
    if packets[0].relative_us != 0 || packets[0].payload.as_slice() != b"bb" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trim 后首帧 relative/payload 不符",
        ));
    }
    let marker = packets
        .iter()
        .find(|p| p.kind == PacketKind::Marker)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "trim 后缺 marker"))?;
    if marker.aux.as_slice() != b"cut-me" || marker.relative_us != 10_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "marker 再基异常: rel={} aux={:?}",
                marker.relative_us,
                String::from_utf8_lossy(&marker.aux)
            ),
        ));
    }
    if packets.last().map(|p| p.relative_us) != Some(30_000) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "末帧 relative 应为 30000，实际 {:?}",
                packets.last().map(|p| p.relative_us)
            ),
        ));
    }
    let meta = reader.meta_json().unwrap_or("");
    if !meta.contains("trim [10000,40000]") || !meta.contains("pre") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SessionMeta notes 未追加 trim 说明: {meta}"),
        ));
    }

    eprintln!("  控制面 session.trim 再基写读：通过");
    Ok(())
}

fn self_check_http_static() -> io::Result<()> {
    let dir = std::env::temp_dir().join("fieldreplay_e6a_ui");
    let _ = std::fs::create_dir_all(&dir);
    let index = dir.join("index.html");
    std::fs::write(&index, b"<!doctype html><title>e6a</title>ok-e6a")?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let state = Arc::new(Mutex::new(ControlState::new()));
    let hub = Arc::new(PushHub::new());
    let ui = dir.clone();
    thread::spawn(move || {
        // 连续接两路：GET / 与 WS，验证同端口分流
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let st = Arc::clone(&state);
                let hb = Arc::clone(&hub);
                let ui_dir = ui.clone();
                let _ = handle_client(stream, st, hb, Some(&ui_dir));
            }
        }
    });
    thread::sleep(Duration::from_millis(50));

    {
        let mut http = TcpStream::connect(addr)?;
        http.set_read_timeout(Some(Duration::from_secs(3)))?;
        http.set_write_timeout(Some(Duration::from_secs(3)))?;
        http.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 512];
        loop {
            match http.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                    break;
                }
                Err(e) => return Err(e),
            }
            if buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() > 64 {
                // 等到有 body
                if buf.windows(b"ok-e6a".len()).any(|w| w == b"ok-e6a") {
                    break;
                }
            }
        }
        let resp = String::from_utf8_lossy(&buf);
        if !resp.contains("200 OK") || !resp.contains("ok-e6a") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("静态 GET / 失败: {resp}"),
            ));
        }
    }

    {
        let mut client = TcpStream::connect(addr)?;
        client.set_read_timeout(Some(Duration::from_secs(3)))?;
        client.set_write_timeout(Some(Duration::from_secs(3)))?;
        ws_client_handshake(&mut client)?;
        write_text_frame_client(&mut client, r#"{"id":"e6a","cmd":"ping"}"#)?;
        let pong = read_text_frame(&mut client)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "静态口 WS ping 无响应")
        })?;
        if !pong.contains("\"pong\":true") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("静态口 WS ping 失败: {pong}"),
            ));
        }
    }

    eprintln!("  控制面 HTTP 静态 UI + 同端口 WS：通过");
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

/// 验证 `replay.pause` / `resume`：暂停后 status 带 paused，恢复后可正常 stop。
fn self_check_replay_pause_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let recv = UdpSocket::bind("127.0.0.1:0")?;
    recv.set_read_timeout(Some(Duration::from_millis(200)))?;
    let target = recv.local_addr()?;

    let open = format!(
        r#"{{"id":"p0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "pause 自检未收到 session.open")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pause 自检 session.open 失败: {opened}"),
        ));
    }

    // 0.1x 拉长墙钟间隔，便于抢在首帧前/间暂停
    let start = format!(
        r#"{{"id":"p1","cmd":"replay.start","type":"udp","speed":0.1,"config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        target.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "pause 自检未收到 replay.start")
    })?;
    if !started.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pause 自检 replay.start 失败: {started}"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"p2","cmd":"replay.pause"}"#)?;
    let paused = drain_until_id(client, "p2")?;
    if !paused.contains("\"ok\":true") || !paused.contains("\"paused\":true") {
        let _ = write_text_frame_client(client, r#"{"id":"px","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("replay.pause 失败: {paused}"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"p3","cmd":"status"}"#)?;
    let st = drain_until_id(client, "p3")?;
    if !st.contains("\"paused\":true") {
        let _ = write_text_frame_client(client, r#"{"id":"px","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("暂停后 status 应含 paused:true: {st}"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"p4","cmd":"replay.resume"}"#)?;
    let resumed = drain_until_id(client, "p4")?;
    if !resumed.contains("\"ok\":true") || !resumed.contains("\"paused\":false") {
        let _ = write_text_frame_client(client, r#"{"id":"px","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("replay.resume 失败: {resumed}"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"p5","cmd":"replay.stop"}"#)?;
    let stopped = drain_until_id(client, "p5")?;
    if !stopped.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pause 自检 replay.stop 失败: {stopped}"),
        ));
    }

    eprintln!("  控制面 replay.pause/resume：通过");
    Ok(())
}

/// 验证 `replay.seek`：慢速启动后 seek 到后半段，进度相对时戳应跳到目标附近。
fn self_check_replay_seek_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let recv = UdpSocket::bind("127.0.0.1:0")?;
    recv.set_read_timeout(Some(Duration::from_millis(200)))?;
    let target = recv.local_addr()?;

    let open = format!(
        r#"{{"id":"s0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "seek 自检未收到 session.open")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("seek 自检 session.open 失败: {opened}"),
        ));
    }

    // sample 帧约在 1000us / 50000us；0.05x 拉长墙钟，便于抢在第二帧前 seek
    let start = format!(
        r#"{{"id":"s1","cmd":"replay.start","type":"udp","speed":0.05,"config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        target.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "seek 自检未收到 replay.start")
    })?;
    if !started.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("seek 自检 replay.start 失败: {started}"),
        ));
    }

    write_text_frame_client(
        client,
        r#"{"id":"s2","cmd":"replay.seek","relativeUs":50000}"#,
    )?;
    let seeked = drain_until_id(client, "s2")?;
    if !seeked.contains("\"ok\":true") || !seeked.contains("\"seeked\":true") {
        let _ = write_text_frame_client(client, r#"{"id":"sx","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("replay.seek 失败: {seeked}"),
        ));
    }

    // 等 progress 推到 ≥40000（seek 回调或后续发帧）
    let mut saw_seek_progress = false;
    client.set_read_timeout(Some(Duration::from_secs(3)))?;
    for _ in 0..48 {
        let frame = match read_text_frame(client)? {
            Some(f) => f,
            None => break,
        };
        if frame.contains("\"event\":\"replay.progress\"") && frame.contains("\"relativeUs\":") {
            if let Some(us) = extract_u64(&frame, "relativeUs") {
                if us >= 40_000 {
                    saw_seek_progress = true;
                    break;
                }
            }
        }
        if frame.contains("\"event\":\"replay.done\"") {
            break;
        }
    }
    if !saw_seek_progress {
        let _ = write_text_frame_client(client, r#"{"id":"sx","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "seek 后未看到 relativeUs≥40000 的 progress",
        ));
    }

    write_text_frame_client(client, r#"{"id":"s3","cmd":"replay.stop"}"#)?;
    let stopped = drain_until_id(client, "s3")?;
    if !stopped.contains("\"ok\":true") {
        // seek 到末帧后线程可能已自然结束并回收槽位
        if !(saw_seek_progress && stopped.contains("当前未在重放")) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("seek 自检 replay.stop 失败: {stopped}"),
            ));
        }
    }

    eprintln!("  控制面 replay.seek：通过");
    Ok(())
}

/// 验证 `replay.start.fault`：100% 丢包时线上应收不到 UDP。
fn self_check_replay_fault_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let recv = UdpSocket::bind("127.0.0.1:0")?;
    recv.set_read_timeout(Some(Duration::from_millis(200)))?;
    let target = recv.local_addr()?;

    let open = format!(
        r#"{{"id":"f0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "fault 自检未收到 session.open")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("fault 自检 session.open 失败: {opened}"),
        ));
    }

    let start = format!(
        r#"{{"id":"f1","cmd":"replay.start","type":"udp","speed":10,"fault":{{"enabled":true,"lossRatePercent":100,"corruptCrc":false}},"config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        target.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "fault 自检未收到 replay.start")
    })?;
    if !started.contains("\"ok\":true") || !started.contains("\"started\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("fault 自检 replay.start 失败: {started}"),
        ));
    }
    if !started.contains("\"lossRatePercent\":100") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("fault 自检未回显丢包率: {started}"),
        ));
    }

    let mut got = 0u32;
    let mut saw_done = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    client.set_read_timeout(Some(Duration::from_millis(150)))?;
    while std::time::Instant::now() < deadline && !saw_done {
        let mut buf = [0u8; 256];
        match recv.recv_from(&mut buf) {
            Ok((n, _)) if n > 0 => got += 1,
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
        match read_text_frame(client) {
            Ok(Some(frame)) => {
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

    if !saw_done {
        write_text_frame_client(client, r#"{"id":"f2","cmd":"replay.stop"}"#)?;
        let _ = drain_until_id(client, "f2")?;
    }

    if got != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("100% 丢包时期望线上 0 包，实际收到 {got}"),
        ));
    }

    eprintln!("  控制面 replay.fault 100% 丢包：通过");
    Ok(())
}

/// 验证 `replay.start.loop`：高速循环后 sent 超过单遍帧数，再 stop。
fn self_check_replay_loop_via_ws(client: &mut TcpStream, session_path: &Path) -> io::Result<()> {
    let recv = UdpSocket::bind("127.0.0.1:0")?;
    recv.set_read_timeout(Some(Duration::from_millis(50)))?;
    let target = recv.local_addr()?;

    let open = format!(
        r#"{{"id":"l0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(session_path)
    );
    write_text_frame_client(client, &open)?;
    let opened = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "loop 自检未收到 session.open")
    })?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("loop 自检 session.open 失败: {opened}"),
        ));
    }

    let start = format!(
        r#"{{"id":"l1","cmd":"replay.start","type":"udp","speed":20,"loop":true,"config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        target.port()
    );
    write_text_frame_client(client, &start)?;
    let started = read_text_frame(client)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "loop 自检未收到 replay.start")
    })?;
    if !started.contains("\"ok\":true") || !started.contains("\"started\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("loop 自检 replay.start 失败: {started}"),
        ));
    }
    if !started.contains("\"loop\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("loop 自检未回显 loop:true: {started}"),
        ));
    }

    // sample 约 2 帧；循环后 sent≥3 即越过单遍
    let mut max_sent = 0u64;
    let mut saw_wrap = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    client.set_read_timeout(Some(Duration::from_millis(100)))?;
    while std::time::Instant::now() < deadline && !saw_wrap {
        let mut buf = [0u8; 256];
        let _ = recv.recv_from(&mut buf);
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"replay.done\"") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("循环重放不应在 stop 前自然 done: {frame}"),
                    ));
                }
                if let Some(s) = extract_progress_sent(&frame) {
                    if s > max_sent {
                        max_sent = s;
                    }
                    if s >= 3 {
                        saw_wrap = true;
                    }
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

    write_text_frame_client(client, r#"{"id":"l2","cmd":"replay.stop"}"#)?;
    let stopped = drain_until_id(client, "l2")?;
    if !stopped.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("loop 自检 replay.stop 失败: {stopped}"),
        ));
    }

    if !saw_wrap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("循环重放未越过单遍（max sent={max_sent}，期望≥3）"),
        ));
    }

    eprintln!("  控制面 replay.loop：通过");
    Ok(())
}

/// 从 progress 推送里抠 sent（粗解析）。
fn extract_progress_sent(frame: &str) -> Option<u64> {
    if !frame.contains("\"event\":\"replay.progress\"") {
        return None;
    }
    let key = "\"sent\":";
    let i = frame.find(key)?;
    let rest = &frame[i + key.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// 读到匹配 `id` 的响应；其间丢弃推送事件。
fn drain_until_id(client: &mut TcpStream, id: &str) -> io::Result<String> {
    let needle = format!("\"id\":\"{id}\"");
    client.set_read_timeout(Some(Duration::from_secs(3)))?;
    for _ in 0..32 {
        let frame = read_text_frame(client)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, format!("等待 id={id} 时连接关闭"))
        })?;
        if frame.contains(&needle) {
            return Ok(frame);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("未等到 id={id} 的响应"),
    ))
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

    // E13b：录制中热插 marker
    write_text_frame_client(
        client,
        r#"{"id":"e13b-live","cmd":"session.addMarker","text":"live-mark"}"#,
    )?;
    let mut live_mark_ok = false;
    client.set_read_timeout(Some(Duration::from_millis(800)))?;
    for _ in 0..24 {
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"id\":\"e13b-live\"") {
                    if frame.contains("\"ok\":true") && frame.contains("live-mark") {
                        live_mark_ok = true;
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("录制中 addMarker 失败: {frame}"),
                        ));
                    }
                    break;
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
    if !live_mark_ok {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "录制中 session.addMarker 无成功响应",
        ));
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
    // 期望至少收到若干帧（含热插 marker）
    if let Some(n) = extract_packet_count(&stop_reply) {
        if n < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("期望至少 4 帧（含 marker），实际 {n}；推送={saw_packet_event}"),
            ));
        }
    }

    let mut reader = SessionReader::open(&out)?;
    let packets = reader.read_all_packets()?;
    let live_marker = packets
        .iter()
        .find(|p| p.kind == PacketKind::Marker)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "录制落盘无 marker 帧")
        })?;
    if live_marker.aux.as_slice() != b"live-mark" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "录制 marker aux 不一致: {:?}",
                String::from_utf8_lossy(&live_marker.aux)
            ),
        ));
    }

    eprintln!("  控制面 record.start/stop UDP 环回 + 热插 marker：通过");
    Ok(())
}

/// E16b：录制 pause → 墙钟等待 → resume；相对戳连续；暂停中 marker 冻结；paused 下可 stop。
fn self_check_record_pause_via_ws(client: &mut TcpStream) -> io::Result<()> {
    // 空闲态 pause 应拒绝
    write_text_frame_client(client, r#"{"id":"e16b-idle","cmd":"record.pause"}"#)?;
    let idle = drain_until_id(client, "e16b-idle")?;
    if idle.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("空闲态 record.pause 应失败: {idle}"),
        ));
    }

    let probe = UdpSocket::bind("127.0.0.1:0")?;
    let udp_addr = probe.local_addr()?;
    drop(probe);

    let out = std::env::temp_dir()
        .join("fieldreplay_e16b_pause")
        .join("pause.fcap");
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    let _ = std::fs::remove_file(&out);

    let start = format!(
        r#"{{"id":"e16b-1","cmd":"record.start","type":"udp","name":"pause-udp","sessionTitle":"e16b","out":"{}","config":{{"bind_ip":"127.0.0.1","bind_port":{}}}}}"#,
        escape_path_for_json(&out),
        udp_addr.port()
    );
    write_text_frame_client(client, &start)?;
    let started = drain_until_id(client, "e16b-1")?;
    if !started.contains("\"ok\":true") || !started.contains("\"started\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("e16b record.start 失败: {started}"),
        ));
    }

    thread::sleep(Duration::from_millis(80));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.send_to(b"PRE1", udp_addr)?;
    thread::sleep(Duration::from_millis(40));
    sender.send_to(b"PRE2", udp_addr)?;
    thread::sleep(Duration::from_millis(60));

    write_text_frame_client(client, r#"{"id":"e16b-2","cmd":"record.pause"}"#)?;
    let paused = drain_until_id(client, "e16b-2")?;
    if !paused.contains("\"ok\":true") || !paused.contains("\"paused\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record.pause 失败: {paused}"),
        ));
    }
    // 幂等
    write_text_frame_client(client, r#"{"id":"e16b-2b","cmd":"record.pause"}"#)?;
    let paused2 = drain_until_id(client, "e16b-2b")?;
    if !paused2.contains("\"already\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record.pause 幂等应 already: {paused2}"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"e16b-st","cmd":"status"}"#)?;
    let st = drain_until_id(client, "e16b-st")?;
    if !st.contains("\"paused\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("暂停后 status 应含 paused:true: {st}"),
        ));
    }

    // 暂停中丢弃收包；热插 marker 落冻结戳
    sender.send_to(b"DROP", udp_addr)?;
    thread::sleep(Duration::from_millis(30));
    write_text_frame_client(
        client,
        r#"{"id":"e16b-mk","cmd":"session.addMarker","text":"pause-mark"}"#,
    )?;
    let mk = drain_until_id(client, "e16b-mk")?;
    if !mk.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("暂停中 addMarker 失败: {mk}"),
        ));
    }

    let pause_wall = Duration::from_millis(200);
    thread::sleep(pause_wall);
    sender.send_to(b"DROP2", udp_addr)?;
    thread::sleep(Duration::from_millis(30));

    write_text_frame_client(client, r#"{"id":"e16b-3","cmd":"record.resume"}"#)?;
    let resumed = drain_until_id(client, "e16b-3")?;
    if !resumed.contains("\"ok\":true") || !resumed.contains("\"paused\":false") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("record.resume 失败: {resumed}"),
        ));
    }

    thread::sleep(Duration::from_millis(40));
    sender.send_to(b"POST", udp_addr)?;
    thread::sleep(Duration::from_millis(80));

    // paused 下可 stop：再 pause 后直接 stop
    write_text_frame_client(client, r#"{"id":"e16b-4","cmd":"record.pause"}"#)?;
    let _ = drain_until_id(client, "e16b-4")?;
    write_text_frame_client(client, r#"{"id":"e16b-5","cmd":"record.stop"}"#)?;
    let stop_reply = drain_until_id(client, "e16b-5")?;
    if !stop_reply.contains("\"ok\":true") || !stop_reply.contains("\"stopped\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("paused 下 record.stop 失败: {stop_reply}"),
        ));
    }

    let mut reader = SessionReader::open(&out)?;
    let packets = reader.read_all_packets()?;
    let _ = std::fs::remove_file(&out);

    let data: Vec<_> = packets
        .iter()
        .filter(|p| p.kind == PacketKind::Data)
        .collect();
    let payloads: Vec<&[u8]> = data.iter().map(|p| p.payload.as_slice()).collect();
    if payloads.iter().any(|p| p.starts_with(b"DROP")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("暂停期收包不应落盘: {payloads:?}"),
        ));
    }
    if !payloads.iter().any(|p| *p == b"PRE1") || !payloads.iter().any(|p| *p == b"POST") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("应含暂停前后数据帧: {payloads:?}"),
        ));
    }

    let marker = packets
        .iter()
        .find(|p| p.kind == PacketKind::Marker && p.aux.as_slice() == b"pause-mark")
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "缺暂停中写入的 marker")
        })?;

    let pre_last = data
        .iter()
        .filter(|p| p.payload.as_slice() == b"PRE2" || p.payload.as_slice() == b"PRE1")
        .map(|p| p.relative_us)
        .max()
        .unwrap_or(0);
    let post = data
        .iter()
        .find(|p| p.payload.as_slice() == b"POST")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "缺 POST 帧"))?;

    if marker.relative_us < pre_last.saturating_sub(5_000) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "暂停 marker 应落在冻结戳附近: mark={} pre_last={pre_last}",
                marker.relative_us
            ),
        ));
    }
    if post.relative_us < marker.relative_us {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "恢复后相对戳应 ≥ marker: post={} mark={}",
                post.relative_us, marker.relative_us
            ),
        ));
    }
    let gap = post.relative_us.saturating_sub(pre_last);
    let pause_us = pause_wall.as_micros() as u64;
    if gap >= pause_us {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "相对戳间隔应远小于暂停墙钟: gap={gap}us pause_wall={pause_us}us"
            ),
        ));
    }

    eprintln!("  控制面 record.pause/resume 相对戳冻结：通过");
    Ok(())
}

/// E16d：UDP+TCP 多路共享轴 pause/resume；暂停期无 `record.packet`、无新增 data。
fn self_check_record_pause_multi_via_ws(client: &mut TcpStream) -> io::Result<()> {
    let udp_probe = UdpSocket::bind("127.0.0.1:0")?;
    let udp_addr = udp_probe.local_addr()?;
    drop(udp_probe);

    let tcp_probe = TcpListener::bind("127.0.0.1:0")?;
    let tcp_addr = tcp_probe.local_addr()?;
    drop(tcp_probe);

    let out = std::env::temp_dir()
        .join("fieldreplay_e16d_pause_multi")
        .join("pause_multi.fcap");
    let _ = std::fs::create_dir_all(out.parent().unwrap());
    let _ = std::fs::remove_file(&out);

    let mut start = String::from(
        r#"{"id":"e16d-1","cmd":"record.start","sessionTitle":"e16d","out":""#,
    );
    start.push_str(&escape_path_for_json(&out));
    start.push_str(r#"","channels":[{"type":"udp","name":"u0","config":{"bind_ip":"127.0.0.1","bind_port":"#);
    start.push_str(&udp_addr.port().to_string());
    start.push_str(r#"}},{"type":"tcp","name":"t0","config":{"role":"server","host":"127.0.0.1","port":"#);
    start.push_str(&tcp_addr.port().to_string());
    start.push_str(r#"}}]}"#);
    write_text_frame_client(client, &start)?;
    let started = drain_until_id(client, "e16d-1")?;
    if !started.contains("\"ok\":true")
        || !started.contains("\"started\":true")
        || !started.contains("\"channels\"")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("e16d 多路 record.start 失败: {started}"),
        ));
    }

    // 等两路就绪（TCP listen + UDP bind）
    thread::sleep(Duration::from_millis(120));
    let udp_tx = UdpSocket::bind("127.0.0.1:0")?;
    let mut tcp_tx = TcpStream::connect(tcp_addr)?;
    tcp_tx.set_write_timeout(Some(Duration::from_secs(2)))?;
    thread::sleep(Duration::from_millis(80));

    udp_tx.send_to(b"UPRE", udp_addr)?;
    tcp_tx.write_all(b"TPRE")?;
    thread::sleep(Duration::from_millis(80));

    write_text_frame_client(client, r#"{"id":"e16d-2","cmd":"record.pause"}"#)?;
    let paused = drain_until_id(client, "e16d-2")?;
    if !paused.contains("\"ok\":true") || !paused.contains("\"paused\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("e16d record.pause 失败: {paused}"),
        ));
    }

    // 排空暂停前残留推送
    let _ = count_record_packet_events(client, Duration::from_millis(150))?;

    // 暂停窗口内边灌包边监听：应无新 record.packet
    let pause_wall = Duration::from_millis(220);
    let during = pump_and_count_packet_events(
        client,
        &udp_tx,
        udp_addr,
        &mut tcp_tx,
        pause_wall,
    )?;
    if during > 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("暂停期不应推送 record.packet，实际 {during} 条"),
        ));
    }

    write_text_frame_client(client, r#"{"id":"e16d-3","cmd":"record.resume"}"#)?;
    let resumed = drain_until_id(client, "e16d-3")?;
    if !resumed.contains("\"ok\":true") || !resumed.contains("\"paused\":false") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("e16d record.resume 失败: {resumed}"),
        ));
    }

    thread::sleep(Duration::from_millis(40));
    udp_tx.send_to(b"UPOST", udp_addr)?;
    tcp_tx.write_all(b"TPOST")?;
    thread::sleep(Duration::from_millis(100));

    write_text_frame_client(client, r#"{"id":"e16d-4","cmd":"record.stop"}"#)?;
    let stop_reply = drain_until_id(client, "e16d-4")?;
    if !stop_reply.contains("\"ok\":true") || !stop_reply.contains("\"stopped\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("e16d record.stop 失败: {stop_reply}"),
        ));
    }

    let mut reader = SessionReader::open(&out)?;
    let channels = reader.channels().to_vec();
    let packets = reader.read_all_packets()?;
    let _ = std::fs::remove_file(&out);

    if channels.len() != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("通道表应为 2 路，实际 {}", channels.len()),
        ));
    }
    if channels[0].channel_type != ChannelType::Udp || channels[1].channel_type != ChannelType::Tcp {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "期望 UDP+TCP，实际 {:?}+{:?}",
                channels[0].channel_type, channels[1].channel_type
            ),
        ));
    }

    let data: Vec<_> = packets
        .iter()
        .filter(|p| p.kind == PacketKind::Data)
        .collect();
    if data.iter().any(|p| {
        let pl = p.payload.as_slice();
        pl.starts_with(b"UDROP") || pl.starts_with(b"TDROP") || pl == b"DROP"
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "暂停期收包不应落盘: {:?}",
                data.iter().map(|p| p.payload.as_slice()).collect::<Vec<_>>()
            ),
        ));
    }

    let has_upre = data.iter().any(|p| p.channel_index == 0 && p.payload.as_slice() == b"UPRE");
    let has_tpre = data.iter().any(|p| p.channel_index == 1 && p.payload.as_slice() == b"TPRE");
    let has_upost = data.iter().any(|p| p.channel_index == 0 && p.payload.as_slice() == b"UPOST");
    let has_tpost = data.iter().any(|p| p.channel_index == 1 && p.payload.as_slice() == b"TPOST");
    if !(has_upre && has_tpre && has_upost && has_tpost) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "两路均应有暂停前后 data: upre={has_upre} tpre={has_tpre} upost={has_upost} tpost={has_tpost}"
            ),
        ));
    }

    let pre_last = data
        .iter()
        .filter(|p| {
            let pl = p.payload.as_slice();
            pl == b"UPRE" || pl == b"TPRE"
        })
        .map(|p| p.relative_us)
        .max()
        .unwrap_or(0);
    let post_first = data
        .iter()
        .filter(|p| {
            let pl = p.payload.as_slice();
            pl == b"UPOST" || pl == b"TPOST"
        })
        .map(|p| p.relative_us)
        .min()
        .unwrap_or(u64::MAX);
    if post_first < pre_last {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("共享轴恢复后相对戳应 ≥ 暂停前: post={post_first} pre={pre_last}"),
        ));
    }
    let gap = post_first.saturating_sub(pre_last);
    let pause_us = pause_wall.as_micros() as u64;
    if gap >= pause_us {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("多路共享轴间隔应远小于暂停墙钟: gap={gap}us pause_wall={pause_us}us"),
        ));
    }

    eprintln!("  控制面多路 record.pause/resume（无推送/无落盘）：通过");
    Ok(())
}

/// R2b：控制面 `replay.start` 非空 `targets[]`（UDP+TCP）环回收到载荷。
fn self_check_replay_multi_targets_via_ws(client: &mut TcpStream) -> io::Result<()> {
    use crate::tcp::{TcpConfig, TcpRole};

    let dir = std::env::temp_dir().join("fieldreplay_r2b_multi_targets");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("multi.fcap");
    let _ = std::fs::remove_file(&path);

    let tcp_rec = TcpConfig::new(TcpRole::Server, "127.0.0.1", 19001);
    let channels = [
        ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "u0".into(),
            config_json: r#"{"bind_ip":"127.0.0.1","bind_port":19000}"#.into(),
        },
        ChannelEntry {
            channel_index: 1,
            channel_type: ChannelType::Tcp,
            name: "t0".into(),
            config_json: tcp_rec.to_config_json(),
        },
    ];
    {
        let mut w = SessionWriter::create(&path, 1_700_000_000_000_000, &channels, None)?;
        w.append_packet(&PacketRecord {
            relative_us: 1_000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:1".to_vec(),
            payload: b"RU1".to_vec(),
        })?;
        w.append_packet(&PacketRecord {
            relative_us: 2_000,
            channel_index: 1,
            kind: PacketKind::TcpConnect,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:2".to_vec(),
            payload: Vec::new(),
        })?;
        w.append_packet(&PacketRecord {
            relative_us: 3_000,
            channel_index: 1,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: Vec::new(),
            payload: b"RT1".to_vec(),
        })?;
        w.append_packet(&PacketRecord {
            relative_us: 4_000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:1".to_vec(),
            payload: b"RU2".to_vec(),
        })?;
        w.append_packet(&PacketRecord {
            relative_us: 5_000,
            channel_index: 1,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: Vec::new(),
            payload: b"RT2".to_vec(),
        })?;
        w.finalize()?;
    }

    let open = format!(
        r#"{{"id":"r2b-0","cmd":"session.open","path":"{}"}}"#,
        escape_path_for_json(&path)
    );
    write_text_frame_client(client, &open)?;
    let opened = drain_until_id(client, "r2b-0")?;
    if !opened.contains("\"ok\":true") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("r2b session.open 失败: {opened}"),
        ));
    }

    let udp_rx = UdpSocket::bind("127.0.0.1:0")?;
    udp_rx.set_read_timeout(Some(Duration::from_millis(800)))?;
    let udp_addr = udp_rx.local_addr()?;

    let tcp_listener = TcpListener::bind("127.0.0.1:0")?;
    let tcp_addr = tcp_listener.local_addr()?;
    let tcp_got = Arc::new(Mutex::new(Vec::<u8>::new()));
    let tcp_got_c = Arc::clone(&tcp_got);
    let acceptor = thread::spawn(move || -> io::Result<()> {
        tcp_listener.set_nonblocking(false)?;
        let (mut stream, _) = tcp_listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
        let mut buf = [0u8; 256];
        let mut acc = Vec::new();
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
            if acc.len() >= 6 {
                break;
            }
        }
        *tcp_got_c
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "tcp_got 锁中毒"))? = acc;
        Ok(())
    });

    let mut start = String::from(
        r#"{"id":"r2b-1","cmd":"replay.start","speed":50,"targets":["#,
    );
    start.push_str(&format!(
        r#"{{"ch":0,"type":"udp","config":{{"target_ip":"127.0.0.1","target_port":{}}}}}"#,
        udp_addr.port()
    ));
    start.push(',');
    start.push_str(&format!(
        r#"{{"ch":1,"type":"tcp","config":{{"host":"127.0.0.1","port":{}}}}}"#,
        tcp_addr.port()
    ));
    start.push_str("]}");

    write_text_frame_client(client, &start)?;
    let started = drain_until_id(client, "r2b-1")?;
    if !started.contains("\"ok\":true")
        || !started.contains("\"started\":true")
        || !started.contains("\"targets\"")
    {
        let _ = write_text_frame_client(client, r#"{"id":"r2b-x","cmd":"replay.stop"}"#);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("r2b replay.start targets[] 失败: {started}"),
        ));
    }

    // 等 replay.done 或超时后再 stop
    let mut saw_done = false;
    client.set_read_timeout(Some(Duration::from_millis(400)))?;
    for _ in 0..40 {
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"replay.done\"") {
                    saw_done = true;
                    break;
                }
            }
            Ok(None) => break,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                if saw_done {
                    break;
                }
                continue;
            }
            Err(e) => {
                client.set_read_timeout(Some(Duration::from_secs(5)))?;
                return Err(e);
            }
        }
    }
    client.set_read_timeout(Some(Duration::from_secs(5)))?;

    if !saw_done {
        write_text_frame_client(client, r#"{"id":"r2b-2","cmd":"replay.stop"}"#)?;
        let _ = drain_until_id(client, "r2b-2")?;
    }

    let mut udp_buf = [0u8; 64];
    let mut udp_acc = Vec::new();
    for _ in 0..8 {
        match udp_rx.recv_from(&mut udp_buf) {
            Ok((n, _)) => udp_acc.extend_from_slice(&udp_buf[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(e) => return Err(e),
        }
    }

    acceptor
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "r2b TCP accept 线程 panic"))??;
    let tcp_acc = tcp_got
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "tcp_got 锁中毒"))?
        .clone();

    let _ = std::fs::remove_file(&path);

    let udp_ok = udp_acc.windows(3).any(|w| w == b"RU1")
        && udp_acc.windows(3).any(|w| w == b"RU2");
    let tcp_ok = tcp_acc.windows(3).any(|w| w == b"RT1")
        && tcp_acc.windows(3).any(|w| w == b"RT2");
    if !udp_ok || !tcp_ok {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "r2b 多路 targets 环回载荷不足: udp={udp_acc:?} tcp={tcp_acc:?} done={saw_done}"
            ),
        ));
    }

    eprintln!("  控制面 replay.start targets[] UDP+TCP 环回：通过");
    Ok(())
}

/// 在窗口内统计 `record.packet` 推送条数（忽略其它帧）。
fn count_record_packet_events(client: &mut TcpStream, window: Duration) -> io::Result<usize> {
    let mut n = 0usize;
    client.set_read_timeout(Some(Duration::from_millis(40)))?;
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"record.packet\"") {
                    n += 1;
                }
            }
            Ok(None) => break,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                client.set_read_timeout(Some(Duration::from_secs(5)))?;
                return Err(e);
            }
        }
    }
    client.set_read_timeout(Some(Duration::from_secs(5)))?;
    Ok(n)
}

/// 暂停观察：周期向 UDP/TCP 灌丢弃载荷，同时统计是否仍有 `record.packet`。
fn pump_and_count_packet_events(
    client: &mut TcpStream,
    udp_tx: &UdpSocket,
    udp_addr: SocketAddr,
    tcp_tx: &mut TcpStream,
    window: Duration,
) -> io::Result<usize> {
    let mut n = 0usize;
    client.set_read_timeout(Some(Duration::from_millis(30)))?;
    let deadline = Instant::now() + window;
    let mut next_pump = Instant::now();
    while Instant::now() < deadline {
        if Instant::now() >= next_pump {
            let _ = udp_tx.send_to(b"UDROP", udp_addr);
            let _ = tcp_tx.write_all(b"TDROP");
            next_pump = Instant::now() + Duration::from_millis(40);
        }
        match read_text_frame(client) {
            Ok(Some(frame)) => {
                if frame.contains("\"event\":\"record.packet\"") {
                    n += 1;
                }
            }
            Ok(None) => break,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                client.set_read_timeout(Some(Duration::from_secs(5)))?;
                return Err(e);
            }
        }
    }
    client.set_read_timeout(Some(Duration::from_secs(5)))?;
    Ok(n)
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

#[cfg(test)]
mod tests {
    use super::self_check_control;

    #[test]
    fn control_plane_self_check_includes_trim_and_import_pcap() {
        self_check_control().expect("控制面自检（含 session.trim / importPcap）");
    }
}
