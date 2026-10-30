//! 控制面 `session.addMarker`：录制热写 / 离线插入 marker。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::fcap::{PacketKind, PacketRecord};
use crate::multi_record::LiveRecordSink;

use super::json_util::{err_result, escape_str, extract_str, extract_u64, ok_result};
use super::state::{ActiveRecording, ControlState, LoadedSession};

/// `session.addMarker`：参数 `text`（必填可空）、`relativeUs?`、`ch?`。
pub fn handle_session_add_marker(
    id: &str,
    msg: &str,
    state: &Arc<Mutex<ControlState>>,
) -> String {
    let Some(text) = extract_str(msg, "text") else {
        return err_result(id, "session.addMarker 需要 text（可为空字符串）");
    };
    let ch = extract_u64(msg, "ch").unwrap_or(0);
    if ch > u16::MAX as u64 {
        return err_result(id, "ch 超出 u16 范围");
    }
    let ch = ch as u16;
    let relative_us = extract_u64(msg, "relativeUs");

    match state.lock() {
        Ok(mut g) => {
            g.reap_finished_replay();
            if g.replaying.is_some() {
                return err_result(id, "重放中无法插入 marker，请先 replay.stop");
            }
            if let Some(ref active) = g.recording {
                if relative_us.is_some() {
                    return err_result(
                        id,
                        "录制中请省略 relativeUs（自动使用当前相对戳）",
                    );
                }
                return add_marker_live(id, &text, ch, active);
            }
            let Some(session) = g.session.as_mut() else {
                return err_result(id, "无会话可标注，请先录制或 session.open/load");
            };
            let Some(rel) = relative_us else {
                return err_result(id, "离线插入 marker 需要 relativeUs");
            };
            match insert_marker_offline(session, &text, rel, ch) {
                Ok(pkt) => marker_ok(id, &pkt, false),
                Err(e) => err_result(id, &e),
            }
        }
        Err(_) => err_result(id, "状态锁损坏"),
    }
}

fn add_marker_live(id: &str, text: &str, ch: u16, active: &ActiveRecording) -> String {
    let sink = match wait_live_sink(&active.live_sink) {
        Ok(s) => s,
        Err(e) => return err_result(id, &e),
    };
    match sink.append_marker(text, ch) {
        Ok(pkt) => {
            // packet_count 由 hook 累加；无 hook 时补一刀（不应发生）
            let _ = active.packet_count.load(Ordering::SeqCst);
            marker_ok(id, &pkt, true)
        }
        Err(e) => err_result(id, &e.to_string()),
    }
}

fn wait_live_sink(
    slot: &Arc<Mutex<Option<Arc<LiveRecordSink>>>>,
) -> Result<Arc<LiveRecordSink>, String> {
    for _ in 0..100 {
        if let Ok(g) = slot.lock() {
            if let Some(s) = g.as_ref() {
                return Ok(Arc::clone(s));
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err("录制写盘尚未就绪，请稍后重试".into())
}

fn insert_marker_offline(
    session: &mut LoadedSession,
    text: &str,
    relative_us: u64,
    ch: u16,
) -> Result<PacketRecord, String> {
    if session.channels.is_empty() {
        return Err("无通道表，无法插入 marker".into());
    }
    if !session.channels.iter().any(|c| c.channel_index == ch) {
        return Err(format!("通道索引 {ch} 不在通道表中"));
    }
    let pkt = PacketRecord {
        relative_us,
        channel_index: ch,
        kind: PacketKind::Marker,
        direction: 0,
        flags: 0,
        aux: text.as_bytes().to_vec(),
        payload: Vec::new(),
    };
    // 插在同戳已有帧之后，保持相对戳单调非降
    let pos = session
        .packets
        .partition_point(|p| p.relative_us <= relative_us);
    session.packets.insert(pos, pkt.clone());
    if relative_us > session.duration_us {
        session.duration_us = relative_us;
    }
    Ok(pkt)
}

fn marker_ok(id: &str, pkt: &PacketRecord, live: bool) -> String {
    let aux = String::from_utf8_lossy(&pkt.aux);
    ok_result(
        id,
        &format!(
            "{{\"added\":true,\"live\":{},\"kind\":{},\"relativeUs\":{},\"ch\":{},\"text\":\"{}\"}}",
            if live { "true" } else { "false" },
            pkt.kind as u8,
            pkt.relative_us,
            pkt.channel_index,
            escape_str(&aux)
        ),
    )
}
