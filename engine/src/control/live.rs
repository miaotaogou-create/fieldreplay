//! 控制面实时录制：`record.start` / `record.stop`，经 PushHub 推送预览帧。

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::fcap::{ChannelType, PacketRecord, SessionReader};
use crate::mqtt::MqttConfig;
use crate::record::{
    record_mqtt_to_file_with, record_serial_to_file_with, record_tcp_to_file_with,
    record_udp_to_file_with, MqttRecordOptions, OnPacket, RecordOptions, SerialRecordOptions,
    TcpRecordOptions,
};
use crate::serial::{Parity, SerialConfig};
use crate::tcp::{TcpConfig, TcpRole};

use super::dto::packet_event_json;
use super::hub::PushHub;
use super::json_util::{
    err_result, escape_str, extract_object, extract_str, extract_u64, ok_result,
};
use super::state::{ActiveRecording, LoadedSession};

pub fn handle_record_start(
    id: &str,
    msg: &str,
    state_recording: &mut Option<ActiveRecording>,
    hub: &Arc<PushHub>,
) -> String {
    if state_recording.is_some() {
        return err_result(id, "已在录制中，请先 record.stop");
    }

    let ty = match extract_str(msg, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None => return err_result(id, "record.start 需要 type"),
    };
    let name = extract_str(msg, "name").unwrap_or_else(|| ty.clone());
    let session_title = extract_str(msg, "sessionTitle");
    let out = extract_str(msg, "out").unwrap_or_else(default_out_path);
    let config_raw = extract_object(msg, "config").unwrap_or_else(|| "{}".into());

    let out_path = PathBuf::from(&out);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicU64::new(0));
    let hub_c = Arc::clone(hub);
    let stop_c = Arc::clone(&stop);
    let count_c = Arc::clone(&packet_count);
    let out_c = out_path.clone();
    let name_c = name.clone();
    let title_c = session_title.clone();
    let ty_c = ty.clone();
    let config_c = config_raw.clone();

    let join = thread::spawn(move || {
        run_record(
            &ty_c,
            &config_c,
            &out_c,
            &name_c,
            title_c.as_deref(),
            &stop_c,
            &count_c,
            &hub_c,
        )
    });

    let channel_json = format!(
        "{{\"index\":0,\"type\":\"{}\",\"name\":\"{}\",\"config\":{}}}",
        escape_str(&ty),
        escape_str(&name),
        if config_raw.trim().is_empty() {
            "{}"
        } else {
            config_raw.as_str()
        }
    );

    *state_recording = Some(ActiveRecording {
        stop,
        out_path: out_path.clone(),
        channel_type: ty,
        packet_count,
        join: Some(join),
    });

    ok_result(
        id,
        &format!(
            "{{\"started\":true,\"out\":\"{}\",\"channel\":{channel_json}}}",
            escape_str(&out_path.to_string_lossy())
        ),
    )
}

pub fn handle_record_stop(
    id: &str,
    state_recording: &mut Option<ActiveRecording>,
    session_slot: &mut Option<LoadedSession>,
) -> String {
    let Some(mut active) = state_recording.take() else {
        return err_result(id, "当前未在录制");
    };
    active.stop.store(true, Ordering::SeqCst);
    let join = active.join.take();
    let out = active.out_path.clone();

    let count = match join {
        Some(h) => match h.join() {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                return err_result(id, &format!("录制线程失败: {e}"));
            }
            Err(_) => return err_result(id, "录制线程异常退出"),
        },
        None => active.packet_count.load(Ordering::SeqCst),
    };

    match load_recorded_session(&out) {
        Ok(loaded) => {
            let duration_us = loaded.duration_us;
            *session_slot = Some(loaded);
            ok_result(
                id,
                &format!(
                    "{{\"stopped\":true,\"out\":\"{}\",\"packetCount\":{count},\"durationUs\":{duration_us},\"finalized\":true}}",
                    escape_str(&out.to_string_lossy())
                ),
            )
        }
        Err(e) => ok_result(
            id,
            &format!(
                "{{\"stopped\":true,\"out\":\"{}\",\"packetCount\":{count},\"durationUs\":0,\"finalized\":false,\"warning\":\"{}\"}}",
                escape_str(&out.to_string_lossy()),
                escape_str(&e.to_string())
            ),
        ),
    }
}

fn load_recorded_session(path: &PathBuf) -> io::Result<LoadedSession> {
    let mut reader = SessionReader::open(path)?;
    let packets = reader.read_all_packets()?;
    Ok(LoadedSession {
        source: path.to_string_lossy().into_owned(),
        start_wall_unix_us: reader.header().start_wall_unix_us,
        duration_us: reader.header().duration_us,
        total_payload_bytes: reader.header().total_payload_bytes,
        finalized: reader.header().is_finalized(),
        meta_json: reader.meta_json().map(|s| s.to_string()),
        channels: reader.channels().to_vec(),
        packets,
    })
}

fn default_out_path() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join("fieldreplay_live");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("rec_{ms}.fcap"))
        .to_string_lossy()
        .into_owned()
}

fn run_record(
    ty: &str,
    config: &str,
    out: &PathBuf,
    name: &str,
    title: Option<&str>,
    stop: &AtomicBool,
    packet_count: &Arc<AtomicU64>,
    hub: &Arc<PushHub>,
) -> io::Result<u64> {
    let hub = Arc::clone(hub);
    let count = Arc::clone(packet_count);
    let ch_ty = parse_channel_type(ty)?;
    let mut hook = move |pkt: &PacketRecord| {
        let i = count.fetch_add(1, Ordering::SeqCst);
        hub.broadcast(&packet_event_json(i, pkt, ch_ty));
    };
    let mut on: OnPacket<'_> = Some(&mut hook);
    let poll = Duration::from_millis(200);
    let title_owned = title.map(|s| s.to_string());

    match ty {
        "udp" => {
            let bind = parse_udp_bind(config)?;
            let opts = RecordOptions {
                bind,
                out_path: out.clone(),
                channel_name: name.to_string(),
                session_title: title_owned,
                max_duration: None,
                poll_timeout: poll,
            };
            record_udp_to_file_with(&opts, stop, &mut on)
        }
        "serial" => {
            let cfg = parse_serial(config)?;
            let opts = SerialRecordOptions {
                config: cfg,
                out_path: out.clone(),
                channel_name: name.to_string(),
                session_title: title_owned,
                max_duration: None,
                poll_timeout: poll,
            };
            record_serial_to_file_with(&opts, stop, &mut on)
        }
        "tcp" => {
            let cfg = TcpConfig::from_config_json(config).or_else(|_| parse_tcp_loose(config))?;
            let opts = TcpRecordOptions {
                config: cfg,
                out_path: out.clone(),
                channel_name: name.to_string(),
                session_title: title_owned,
                max_duration: None,
                poll_timeout: poll,
            };
            record_tcp_to_file_with(&opts, stop, &mut on)
        }
        "mqtt" => {
            let cfg = MqttConfig::from_config_json(config)?;
            let opts = MqttRecordOptions {
                config: cfg,
                out_path: out.clone(),
                channel_name: name.to_string(),
                session_title: title_owned,
                max_duration: None,
                poll_timeout: poll,
            };
            record_mqtt_to_file_with(&opts, stop, &mut on)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("不支持的通道类型: {other}"),
        )),
    }
}

fn parse_channel_type(ty: &str) -> io::Result<ChannelType> {
    match ty {
        "udp" => Ok(ChannelType::Udp),
        "serial" => Ok(ChannelType::Serial),
        "tcp" => Ok(ChannelType::Tcp),
        "mqtt" => Ok(ChannelType::Mqtt),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("未知 type: {other}"),
        )),
    }
}

fn parse_udp_bind(config: &str) -> io::Result<SocketAddr> {
    let ip = extract_str(config, "bind_ip")
        .or_else(|| extract_str(config, "ip"))
        .unwrap_or_else(|| "0.0.0.0".into());
    let port = extract_u64(config, "bind_port")
        .or_else(|| extract_u64(config, "port"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "UDP config 缺 port"))?;
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UDP port 超出范围",
        ));
    }
    format!("{ip}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("UDP bind 无效: {e}")))
}

fn parse_serial(config: &str) -> io::Result<SerialConfig> {
    let port_name = extract_str(config, "port_name")
        .or_else(|| extract_str(config, "portName"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "串口缺 port_name"))?;
    let baud = extract_u64(config, "baud")
        .or_else(|| extract_u64(config, "baudRate"))
        .unwrap_or(115200) as u32;
    let data_bits = extract_u64(config, "data_bits")
        .or_else(|| extract_u64(config, "dataBits"))
        .unwrap_or(8) as u8;
    let stop_bits = extract_u64(config, "stop_bits")
        .or_else(|| extract_u64(config, "stopBits"))
        .unwrap_or(1) as u8;
    let parity = extract_str(config, "parity")
        .and_then(|p| Parity::parse(&p))
        .unwrap_or(Parity::None);
    Ok(SerialConfig {
        port_name,
        baud,
        data_bits,
        stop_bits,
        parity,
    })
}

fn parse_tcp_loose(config: &str) -> io::Result<TcpConfig> {
    let role = extract_str(config, "role")
        .and_then(|r| TcpRole::parse(&r))
        .unwrap_or(TcpRole::Server);
    let host = extract_str(config, "host")
        .or_else(|| extract_str(config, "ip"))
        .unwrap_or_else(|| "0.0.0.0".into());
    let port = extract_u64(config, "port")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TCP 缺 port"))?;
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TCP port 超出范围",
        ));
    }
    Ok(TcpConfig::new(role, host, port as u16))
}
