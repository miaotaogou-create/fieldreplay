//! 控制面会话 / 帧 JSON 拼装。

use std::sync::atomic::Ordering;

use crate::fcap::{ChannelType, PacketKind, PacketRecord};
use crate::mqtt::{mqtt_qos_from_flags, mqtt_retain_from_flags};

use super::b64;
use super::json_util::escape_str;
use super::state::{ActiveRecording, ActiveReplay, ControlState, LoadedSession};

pub fn status_json(g: &ControlState) -> String {
    let recording = g
        .recording
        .as_ref()
        .map(recording_status_json)
        .unwrap_or_else(|| "null".into());
    let replaying = g
        .replaying
        .as_ref()
        .map(replaying_status_json)
        .unwrap_or_else(|| "null".into());
    let state = if g.recording.is_some() {
        "recording"
    } else if g.replaying.is_some() {
        "replaying"
    } else {
        "idle"
    };
    match &g.session {
        None => format!(
            "{{\"state\":\"{state}\",\"sessionLoaded\":false,\"packetCount\":0,\"recording\":{recording},\"replaying\":{replaying}}}"
        ),
        Some(s) => format!(
            "{{\"state\":\"{state}\",\"sessionLoaded\":true,\"packetCount\":{},\"source\":\"{}\",\"recording\":{recording},\"replaying\":{replaying}}}",
            s.packets.len(),
            escape_str(&s.source)
        ),
    }
}

pub fn recording_status_json(active: &ActiveRecording) -> String {
    format!(
        "{{\"out\":\"{}\",\"packetCount\":{},\"type\":\"{}\"}}",
        escape_str(&active.out_path.to_string_lossy()),
        active.packet_count.load(Ordering::SeqCst),
        escape_str(&active.channel_type)
    )
}

pub fn replaying_status_json(active: &ActiveReplay) -> String {
    format!(
        "{{\"sent\":{},\"type\":\"{}\",\"durationUs\":{},\"paused\":{}}}",
        active.sent_count.load(Ordering::SeqCst),
        escape_str(&active.channel_type),
        active.duration_us,
        if active.paused.load(Ordering::SeqCst) {
            "true"
        } else {
            "false"
        }
    )
}

pub fn session_to_json(s: &LoadedSession) -> String {
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
        pkts.push_str(&packet_fields_json(i as u64, p, ch_ty));
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

pub fn packet_fields_json(i: u64, p: &PacketRecord, ch_ty: Option<ChannelType>) -> String {
    let topic = if ch_ty == Some(ChannelType::Mqtt) {
        String::from_utf8_lossy(&p.aux).into_owned()
    } else {
        String::new()
    };
    let qos = mqtt_qos_from_flags(p.flags);
    let retain = mqtt_retain_from_flags(p.flags);
    format!(
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
    )
}

pub fn packet_event_json(i: u64, p: &PacketRecord, ch_ty: ChannelType) -> String {
    format!(
        "{{\"event\":\"record.packet\",\"packet\":{}}}",
        packet_fields_json(i, p, Some(ch_ty))
    )
}

pub fn channel_type_str(t: ChannelType) -> &'static str {
    match t {
        ChannelType::Udp => "udp",
        ChannelType::Serial => "serial",
        ChannelType::Tcp => "tcp",
        ChannelType::Mqtt => "mqtt",
        ChannelType::Video => "video_stream",
    }
}

pub fn packet_kind_u8(k: PacketKind) -> u8 {
    match k {
        PacketKind::Data => 0,
        PacketKind::TcpConnect => 1,
        PacketKind::TcpDisconnect => 2,
        PacketKind::Marker => 3,
    }
}
