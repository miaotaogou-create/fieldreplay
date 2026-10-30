//! 控制面 `session.importPcap`：经典以太网 pcap → 内存会话 / `.fcap` b64。

use std::sync::{Arc, Mutex};

use crate::pcap::{pcap_bytes_to_session, read_pcap_to_session, PcapImportSession};

use super::b64;
use super::dto::session_to_json;
use super::json_util::{
    err_result, extract_bool, extract_str, ok_result,
};
use super::session_file::export_session_bytes;
use super::state::{ControlState, LoadedSession};

/// `session.importPcap`：参数 `b64` 或 `path`（其一）；可选 `replaceSession`（默认 true）。
pub fn handle_session_import_pcap(
    id: &str,
    msg: &str,
    state: &Arc<Mutex<ControlState>>,
) -> String {
    let path = extract_str(msg, "path").filter(|s| !s.is_empty());
    let b64s = extract_str(msg, "b64").filter(|s| !s.is_empty());
    if path.is_none() && b64s.is_none() {
        return err_result(id, "session.importPcap 需要 b64 或 path");
    }
    let replace = extract_bool(msg, "replaceSession").unwrap_or(true);

    let imported = if let Some(ref p) = path {
        match read_pcap_to_session(std::path::Path::new(p)) {
            Ok(s) => s,
            Err(e) => return err_result(id, &e.to_string()),
        }
    } else {
        let raw = match b64::decode(b64s.as_deref().unwrap_or("")) {
            Ok(v) => v,
            Err(e) => return err_result(id, &format!("b64 无效: {e}")),
        };
        let hint = extract_str(msg, "sourceHint")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "upload.pcap".into());
        match pcap_bytes_to_session(&raw, &hint) {
            Ok(s) => s,
            Err(e) => return err_result(id, &e.to_string()),
        }
    };

    let source = path
        .clone()
        .unwrap_or_else(|| "pcap-import".into());
    let written = imported.stats.written;
    let skipped = imported.stats.skipped;
    let channel_count = imported.stats.channel_count;
    let loaded = loaded_from_import(imported, source);

    match state.lock() {
        Ok(mut g) => {
            g.reap_finished_replay();
            if g.recording.is_some() {
                return err_result(id, "录制中无法导入 pcap，请先 record.stop");
            }
            if g.replaying.is_some() {
                return err_result(id, "重放中无法导入 pcap，请先 replay.stop");
            }
            match export_session_bytes(&loaded, loaded.meta_json.as_deref()) {
                Ok((bytes, count)) => {
                    if bytes.len() < 4 || &bytes[..4] != b"FCAP" {
                        return err_result(id, "导入校验失败：非 FCAP 魔数");
                    }
                    let b64_out = b64::encode(&bytes);
                    let sess = session_to_json(&loaded);
                    if replace {
                        g.session = Some(loaded);
                    }
                    ok_result(
                        id,
                        &format!(
                            "{{\"imported\":true,\"written\":{written},\"skipped\":{skipped},\
                             \"channelCount\":{channel_count},\"packetCount\":{count},\
                             \"bytes\":{},\"replaced\":{},\"b64\":\"{b64_out}\",\
                             \"session\":{sess}}}",
                            bytes.len(),
                            if replace { "true" } else { "false" },
                        ),
                    )
                }
                Err(e) => err_result(id, &format!("导入写出失败: {e}")),
            }
        }
        Err(_) => err_result(id, "状态锁损坏"),
    }
}

fn loaded_from_import(imp: PcapImportSession, source: String) -> LoadedSession {
    let duration_us = imp
        .packets
        .iter()
        .map(|p| p.relative_us)
        .max()
        .unwrap_or(0);
    let total_payload_bytes = imp
        .packets
        .iter()
        .map(|p| p.payload.len() as u64)
        .sum();
    LoadedSession {
        source,
        start_wall_unix_us: imp.start_wall_unix_us,
        duration_us,
        total_payload_bytes,
        finalized: true,
        meta_json: Some(imp.meta_json),
        channels: imp.channels,
        packets: imp.packets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord};
    use crate::pcap::session_to_pcap_bytes;
    use crate::tcp::{TcpConfig, TcpRole};
    use std::sync::{Arc, Mutex};

    fn sample_pcap_b64() -> String {
        let channels = vec![ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "udp0".into(),
            config_json: r#"{"bind_ip":"127.0.0.1","bind_port":9000}"#.into(),
        }];
        let packets = vec![PacketRecord {
            relative_us: 0,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"10.0.0.2:5000".to_vec(),
            payload: b"e14c".to_vec(),
        }];
        let (bytes, _) =
            session_to_pcap_bytes(1_700_000_000_000_000, &channels, &packets).expect("pcap");
        b64::encode(&bytes)
    }

    #[test]
    fn import_pcap_replaces_by_default_and_returns_stats() {
        let state = Arc::new(Mutex::new(ControlState::new()));
        let msg = format!(
            r#"{{"id":"t1","cmd":"session.importPcap","b64":"{}"}}"#,
            sample_pcap_b64()
        );
        let resp = handle_session_import_pcap("t1", &msg, &state);
        assert!(resp.contains("\"ok\":true"), "{resp}");
        assert!(resp.contains("\"imported\":true"));
        assert!(resp.contains("\"written\":1"));
        assert!(resp.contains("\"channelCount\":1"));
        assert!(resp.contains("\"replaced\":true"));
        assert!(resp.contains("\"b64\":"));
        let g = state.lock().unwrap();
        assert!(g.session.is_some());
        assert_eq!(g.session.as_ref().unwrap().packets[0].payload, b"e14c");
    }

    #[test]
    fn import_pcap_replace_false_keeps_memory() {
        let state = Arc::new(Mutex::new(ControlState::new()));
        {
            let mut g = state.lock().unwrap();
            g.session = Some(LoadedSession {
                source: "keep-me".into(),
                start_wall_unix_us: 1,
                duration_us: 0,
                total_payload_bytes: 0,
                finalized: true,
                meta_json: None,
                channels: vec![],
                packets: vec![],
            });
        }
        let msg = format!(
            r#"{{"id":"t2","cmd":"session.importPcap","b64":"{}","replaceSession":false}}"#,
            sample_pcap_b64()
        );
        let resp = handle_session_import_pcap("t2", &msg, &state);
        assert!(resp.contains("\"ok\":true"), "{resp}");
        assert!(resp.contains("\"replaced\":false"));
        let g = state.lock().unwrap();
        assert_eq!(g.session.as_ref().unwrap().source, "keep-me");
    }

    #[test]
    fn import_pcap_requires_input() {
        let state = Arc::new(Mutex::new(ControlState::new()));
        let resp = handle_session_import_pcap("t3", r#"{"id":"t3"}"#, &state);
        assert!(resp.contains("\"ok\":false"));
        assert!(resp.contains("b64") || resp.contains("path"));
    }

    #[test]
    fn loaded_from_import_computes_duration_and_payload() {
        let channels = vec![ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Tcp,
            name: "tcp_80".into(),
            config_json: TcpConfig::new(TcpRole::Server, "127.0.0.1", 80).to_config_json(),
        }];
        let packets = vec![PacketRecord {
            relative_us: 42,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"1.2.3.4:9".to_vec(),
            payload: b"x".to_vec(),
        }];
        let imp = PcapImportSession {
            start_wall_unix_us: 99,
            channels,
            packets,
            meta_json: r#"{"notes":"imported 1 written, 2 skipped, 1 channels"}"#.into(),
            stats: crate::pcap::PcapImportStats {
                written: 1,
                skipped: 2,
                channel_count: 1,
            },
        };
        let loaded = loaded_from_import(imp, "t.pcap".into());
        assert_eq!(loaded.duration_us, 42);
        assert_eq!(loaded.total_payload_bytes, 1);
        assert_eq!(loaded.source, "t.pcap");
    }
}
