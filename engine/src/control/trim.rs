//! 控制面 `session.trim`：按相对时间窗裁出会话并再基写出新 `.fcap`。

use std::sync::{Arc, Mutex};

use crate::fcap::PacketRecord;

use super::b64;
use super::json_util::{
    err_result, escape_str, extract_bool, extract_str, extract_u64, ok_result,
};
use super::session_file::export_session_bytes;
use super::state::{ControlState, LoadedSession};

/// `session.trim`：参数 `fromUs`、`toUs`；可选 `replaceSession`。
pub fn handle_session_trim(
    id: &str,
    msg: &str,
    state: &Arc<Mutex<ControlState>>,
) -> String {
    let Some(from_us) = extract_u64(msg, "fromUs") else {
        return err_result(id, "session.trim 需要 fromUs");
    };
    let Some(to_us) = extract_u64(msg, "toUs") else {
        return err_result(id, "session.trim 需要 toUs");
    };
    let replace = extract_bool(msg, "replaceSession").unwrap_or(false);

    match state.lock() {
        Ok(mut g) => {
            g.reap_finished_replay();
            if g.recording.is_some() {
                return err_result(id, "录制中无法 trim，请先 record.stop");
            }
            if g.replaying.is_some() {
                return err_result(id, "重放中无法 trim，请先 replay.stop");
            }
            let Some(session) = g.session.as_ref() else {
                return err_result(id, "无会话可 trim，请先录制或 session.open/load");
            };
            let trimmed = match trim_session(session, from_us, to_us) {
                Ok(t) => t,
                Err(e) => return err_result(id, &e),
            };
            match export_session_bytes(&trimmed, trimmed.meta_json.as_deref()) {
                Ok((bytes, count)) => {
                    if bytes.len() < 4 || &bytes[..4] != b"FCAP" {
                        return err_result(id, "trim 校验失败：非 FCAP 魔数");
                    }
                    let b64s = b64::encode(&bytes);
                    if replace {
                        g.session = Some(trimmed);
                    }
                    ok_result(
                        id,
                        &format!(
                            "{{\"trimmed\":true,\"fromUs\":{from_us},\"toUs\":{to_us},\
                             \"packetCount\":{count},\"bytes\":{},\"replaced\":{},\
                             \"b64\":\"{b64s}\"}}",
                            bytes.len(),
                            if replace { "true" } else { "false" },
                        ),
                    )
                }
                Err(e) => err_result(id, &format!("trim 写出失败: {e}")),
            }
        }
        Err(_) => err_result(id, "状态锁损坏"),
    }
}

/// 闭区间 `[from_us, to_us]` 过滤帧并再基；整表保留通道；notes 追加 trim 行。
pub fn trim_session(
    session: &LoadedSession,
    from_us: u64,
    to_us: u64,
) -> Result<LoadedSession, String> {
    if from_us > to_us {
        return Err("时间窗非法：fromUs 大于 toUs".into());
    }
    let mut packets: Vec<PacketRecord> = Vec::new();
    let mut total_payload_bytes = 0u64;
    for pkt in &session.packets {
        if pkt.relative_us < from_us || pkt.relative_us > to_us {
            continue;
        }
        let mut out = pkt.clone();
        out.relative_us = pkt.relative_us - from_us;
        total_payload_bytes =
            total_payload_bytes.saturating_add(out.payload.len() as u64);
        packets.push(out);
    }
    if packets.is_empty() {
        return Err("时间窗内无帧，拒绝写出空会话".into());
    }
    let duration_us = packets
        .iter()
        .map(|p| p.relative_us)
        .max()
        .unwrap_or(0);
    let meta_json = Some(append_trim_note(
        session.meta_json.as_deref(),
        from_us,
        to_us,
    ));
    Ok(LoadedSession {
        source: format!("trim:{}", session.source),
        start_wall_unix_us: session.start_wall_unix_us.saturating_add(from_us),
        duration_us,
        total_payload_bytes,
        finalized: true,
        meta_json,
        channels: session.channels.clone(),
        packets,
    })
}

fn append_trim_note(meta: Option<&str>, from_us: u64, to_us: u64) -> String {
    let line = format!("trim [{from_us},{to_us}] → rebased");
    let raw = meta.map(str::trim).filter(|s| !s.is_empty());
    let Some(json) = raw else {
        return format!(r#"{{"notes":"{}"}}"#, escape_str(&line));
    };
    if let Some(old) = extract_str(json, "notes") {
        let merged = if old.is_empty() {
            line.clone()
        } else {
            format!("{old}\n{line}")
        };
        if let Some(replaced) = replace_json_str_field(json, "notes", &merged) {
            return replaced;
        }
    }
    insert_notes_field(json, &line)
}

fn insert_notes_field(json: &str, notes: &str) -> String {
    let trimmed = json.trim_end();
    let Some(without) = trimmed.strip_suffix('}') else {
        return format!(r#"{{"notes":"{}"}}"#, escape_str(notes));
    };
    let body = without.trim_end();
    let sep = if body.ends_with('{') || body.is_empty() {
        ""
    } else {
        ","
    };
    format!(r#"{body}{sep}"notes":"{}"}}"#, escape_str(notes))
}

/// 替换对象中某字符串字段的值（仅处理 `"key":"..."` 形状）。
fn replace_json_str_field(json: &str, key: &str, new_val: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let key_at = json.find(&needle)?;
    let after_key = &json[key_at + needle.len()..];
    let value_rel = after_key
        .find('"')
        .filter(|&i| after_key[..i].chars().all(|c| c == ':' || c.is_whitespace()))?;
    let value_open = key_at + needle.len() + value_rel;
    let rest = &json[value_open + 1..];
    let mut end = None;
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                let _ = chars.next()?;
            }
            '"' => {
                end = Some(value_open + 1 + i);
                break;
            }
            _ => {}
        }
    }
    let value_close = end?;
    let mut out = String::with_capacity(json.len() + new_val.len());
    out.push_str(&json[..value_open + 1]);
    out.push_str(&escape_str(new_val));
    out.push_str(&json[value_close..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcap::{ChannelEntry, ChannelType, PacketKind};

    fn sample_session() -> LoadedSession {
        LoadedSession {
            source: "t".into(),
            start_wall_unix_us: 1_000_000,
            duration_us: 50_000,
            total_payload_bytes: 6,
            finalized: true,
            meta_json: Some(r#"{"session_title":"demo","notes":"base"}"#.into()),
            channels: vec![
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
            ],
            packets: vec![
                PacketRecord {
                    relative_us: 0,
                    channel_index: 0,
                    kind: PacketKind::Data,
                    direction: 0,
                    flags: 0,
                    aux: Vec::new(),
                    payload: b"a".to_vec(),
                },
                PacketRecord {
                    relative_us: 10_000,
                    channel_index: 1,
                    kind: PacketKind::Data,
                    direction: 0,
                    flags: 0,
                    aux: Vec::new(),
                    payload: b"bb".to_vec(),
                },
                PacketRecord {
                    relative_us: 20_000,
                    channel_index: 0,
                    kind: PacketKind::Marker,
                    direction: 0,
                    flags: 0,
                    aux: b"mid".to_vec(),
                    payload: Vec::new(),
                },
                PacketRecord {
                    relative_us: 40_000,
                    channel_index: 0,
                    kind: PacketKind::Data,
                    direction: 0,
                    flags: 0,
                    aux: Vec::new(),
                    payload: b"ccc".to_vec(),
                },
                PacketRecord {
                    relative_us: 50_000,
                    channel_index: 1,
                    kind: PacketKind::Data,
                    direction: 0,
                    flags: 0,
                    aux: Vec::new(),
                    payload: b"d".to_vec(),
                },
            ],
        }
    }

    #[test]
    fn trim_rebases_keeps_channels_and_marker() {
        let src = sample_session();
        let out = trim_session(&src, 10_000, 40_000).expect("trim");
        assert_eq!(out.packets.len(), 3);
        assert_eq!(out.packets[0].relative_us, 0);
        assert_eq!(out.packets[0].payload, b"bb");
        assert_eq!(out.packets[1].kind, PacketKind::Marker);
        assert_eq!(out.packets[1].aux, b"mid");
        assert_eq!(out.packets[1].relative_us, 10_000);
        assert_eq!(out.packets[2].relative_us, 30_000);
        assert_eq!(out.start_wall_unix_us, 1_010_000);
        assert_eq!(out.channels.len(), 2);
        assert_eq!(out.duration_us, 30_000);
        let meta = out.meta_json.as_deref().unwrap_or("");
        assert!(meta.contains("trim [10000,40000]"));
        assert!(meta.contains("base"));
        assert!(meta.contains("demo"));
    }

    #[test]
    fn trim_rejects_bad_or_empty_window() {
        let src = sample_session();
        assert!(trim_session(&src, 30_000, 10_000).is_err());
        assert!(trim_session(&src, 60_000, 70_000).is_err());
    }
}
