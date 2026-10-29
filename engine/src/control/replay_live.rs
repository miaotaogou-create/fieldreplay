//! 控制面实时重放：`replay.start` / `replay.stop`，经 PushHub 推送进度。

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::fcap::SessionWriter;
use crate::mqtt::MqttConfig;
use crate::replay::{
    replay_mqtt_from_file_with, replay_serial_with_port_with, replay_tcp_from_file_with,
    replay_udp_from_file_with, MqttReplayOptions, OnReplayProgress, ReplayOptions,
    SerialReplayOptions, TcpReplayOptions,
};
use crate::serial::{Parity, SerialConfig, SerialPort};

use super::hub::PushHub;
use super::json_util::{
    err_result, escape_str, extract_object, extract_str, extract_u64, ok_result,
};
use super::state::{ActiveReplay, ControlState, LoadedSession};

pub fn handle_replay_start(
    id: &str,
    msg: &str,
    state: &mut ControlState,
    hub: &Arc<PushHub>,
) -> String {
    if state.recording.is_some() {
        return err_result(id, "录制中无法重放，请先 record.stop");
    }
    // 调用方应已 reap；此处再防一层
    if state.replaying.is_some() {
        return err_result(id, "已在重放中，请先 replay.stop");
    }
    if state.session.as_ref().map(|s| s.packets.is_empty()).unwrap_or(true) {
        if state.session.is_none() {
            return err_result(id, "请先 session.open 或 session.load");
        }
        return err_result(id, "会话无帧，无法重放");
    }

    let ty = match extract_str(msg, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None => return err_result(id, "replay.start 需要 type"),
    };
    let config_raw = extract_object(msg, "config").unwrap_or_else(|| "{}".into());
    let topic_prefix = extract_str(msg, "topicPrefix")
        .or_else(|| extract_str(msg, "topic_prefix"))
        .unwrap_or_default();

    let (in_path, duration_us) = {
        let session = state.session.as_ref().unwrap();
        let in_path = match materialize_session(session) {
            Ok(p) => p,
            Err(e) => return err_result(id, &format!("无法准备会话文件: {e}")),
        };
        let duration_us = session.duration_us.max(
            session
                .packets
                .last()
                .map(|p| p.relative_us)
                .unwrap_or(0),
        );
        (in_path, duration_us)
    };

    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let sent_count = Arc::new(AtomicU64::new(0));
    let hub_c = Arc::clone(hub);
    let stop_c = Arc::clone(&stop);
    let finished_c = Arc::clone(&finished);
    let sent_c = Arc::clone(&sent_count);
    let ty_c = ty.clone();
    let config_c = config_raw.clone();
    let path_c = in_path.clone();
    let prefix_c = topic_prefix.clone();

    let join = thread::spawn(move || {
        let result = run_replay(
            &ty_c,
            &config_c,
            &path_c,
            &prefix_c,
            duration_us,
            &stop_c,
            &sent_c,
            &hub_c,
        );
        let sent = sent_c.load(Ordering::SeqCst);
        let stopped = stop_c.load(Ordering::SeqCst);
        let done = format!(
            "{{\"event\":\"replay.done\",\"sent\":{sent},\"stopped\":{}}}",
            if stopped { "true" } else { "false" }
        );
        hub_c.broadcast(&done);
        finished_c.store(true, Ordering::SeqCst);
        result
    });

    state.replaying = Some(ActiveReplay {
        stop,
        finished,
        channel_type: ty.clone(),
        sent_count,
        duration_us,
        join: Some(join),
    });

    ok_result(
        id,
        &format!(
            "{{\"started\":true,\"type\":\"{}\",\"config\":{},\"durationUs\":{duration_us}}}",
            escape_str(&ty),
            if config_raw.trim().is_empty() {
                "{}"
            } else {
                config_raw.as_str()
            }
        ),
    )
}

pub fn handle_replay_stop(
    id: &str,
    state_replaying: &mut Option<ActiveReplay>,
) -> String {
    let Some(mut active) = state_replaying.take() else {
        return err_result(id, "当前未在重放");
    };
    active.stop.store(true, Ordering::SeqCst);
    let join = active.join.take();

    let sent = match join {
        Some(h) => match h.join() {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                active.finished.store(true, Ordering::SeqCst);
                return err_result(id, &format!("重放线程失败: {e}"));
            }
            Err(_) => {
                active.finished.store(true, Ordering::SeqCst);
                return err_result(id, "重放线程异常退出");
            }
        },
        None => active.sent_count.load(Ordering::SeqCst),
    };
    active.finished.store(true, Ordering::SeqCst);

    ok_result(
        id,
        &format!("{{\"stopped\":true,\"sent\":{sent}}}"),
    )
}

fn materialize_session(session: &LoadedSession) -> io::Result<PathBuf> {
    if session.source != "upload" {
        let p = PathBuf::from(&session.source);
        if p.is_file() {
            return Ok(p);
        }
    }

    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join("fieldreplay_replay");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("sess_{ms}.fcap"));
    write_session_file(&path, session)?;
    Ok(path)
}

fn write_session_file(path: &Path, session: &LoadedSession) -> io::Result<()> {
    let mut w = SessionWriter::create(
        path,
        session.start_wall_unix_us,
        &session.channels,
        session.meta_json.as_deref(),
    )?;
    for pkt in &session.packets {
        w.append_packet(pkt)?;
    }
    w.finalize()?;
    Ok(())
}

fn run_replay(
    ty: &str,
    config: &str,
    in_path: &Path,
    topic_prefix: &str,
    duration_us: u64,
    stop: &AtomicBool,
    sent_count: &Arc<AtomicU64>,
    hub: &Arc<PushHub>,
) -> io::Result<u64> {
    let hub = Arc::clone(hub);
    let count = Arc::clone(sent_count);
    let mut hook = move |sent: u64, relative_us: u64| {
        count.store(sent, Ordering::SeqCst);
        hub.broadcast(&format!(
            "{{\"event\":\"replay.progress\",\"sent\":{sent},\"relativeUs\":{relative_us},\"durationUs\":{duration_us}}}"
        ));
    };
    let mut on: OnReplayProgress<'_> = Some(&mut hook);
    let poll = Duration::from_millis(200);

    match ty {
        "udp" => {
            let target = parse_udp_target(config)?;
            let opts = ReplayOptions {
                in_path: in_path.to_path_buf(),
                target,
                local_bind: "0.0.0.0:0".parse().unwrap(),
            };
            replay_udp_from_file_with(&opts, stop, &mut on)
        }
        "serial" => {
            let cfg = parse_serial(config)?;
            let opts = SerialReplayOptions {
                in_path: in_path.to_path_buf(),
                config: cfg,
            };
            let port = SerialPort::open(&opts.config, poll)?;
            replay_serial_with_port_with(&opts, &port, stop, &mut on)
        }
        "tcp" => {
            let endpoint = parse_tcp_endpoint(config)?;
            let opts = TcpReplayOptions {
                in_path: in_path.to_path_buf(),
                endpoint,
                poll_timeout: poll,
            };
            replay_tcp_from_file_with(&opts, stop, &mut on)
        }
        "mqtt" => {
            let broker = parse_mqtt_broker(config)?;
            let prefix = if topic_prefix.is_empty() {
                extract_str(config, "topic_prefix")
                    .or_else(|| extract_str(config, "topicPrefix"))
                    .unwrap_or_default()
            } else {
                topic_prefix.to_string()
            };
            let opts = MqttReplayOptions {
                in_path: in_path.to_path_buf(),
                broker,
                topic_prefix: prefix,
                qos_override: None,
                poll_timeout: poll,
            };
            replay_mqtt_from_file_with(&opts, stop, &mut on)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("不支持的重放类型: {other}"),
        )),
    }
}

fn parse_udp_target(config: &str) -> io::Result<SocketAddr> {
    if let Some(target) = extract_str(config, "target") {
        return target.parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("UDP target 无效: {e}"))
        });
    }
    let ip = extract_str(config, "target_ip")
        .or_else(|| extract_str(config, "ip"))
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = extract_u64(config, "target_port")
        .or_else(|| extract_u64(config, "port"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "UDP config 缺 target_port"))?;
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UDP port 超出范围",
        ));
    }
    format!("{ip}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("UDP target 无效: {e}")))
}

fn parse_serial(config: &str) -> io::Result<SerialConfig> {
    let port_name = extract_str(config, "port_name")
        .or_else(|| extract_str(config, "portName"))
        .or_else(|| extract_str(config, "port"))
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

fn parse_tcp_endpoint(config: &str) -> io::Result<SocketAddr> {
    if let Some(ep) = extract_str(config, "endpoint") {
        return ep.parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("TCP endpoint 无效: {e}"))
        });
    }
    // 角色由文件内录制配置取反；此处只需重放侧地址
    let host = extract_str(config, "host")
        .or_else(|| extract_str(config, "ip"))
        .or_else(|| extract_str(config, "clientTargetHost"))
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = extract_u64(config, "port")
        .or_else(|| extract_u64(config, "serverPort"))
        .or_else(|| extract_u64(config, "clientTargetPort"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TCP 缺 port"))?;
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TCP port 超出范围",
        ));
    }
    format!("{host}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("TCP endpoint 无效: {e}")))
}

fn parse_mqtt_broker(config: &str) -> io::Result<SocketAddr> {
    if let Ok(cfg) = MqttConfig::from_config_json(config) {
        return format!("{}:{}", cfg.broker, cfg.port).parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("MQTT broker 无效: {e}"))
        });
    }
    let host = extract_str(config, "broker")
        .or_else(|| extract_str(config, "brokerHost"))
        .or_else(|| extract_str(config, "host"))
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = extract_u64(config, "port")
        .or_else(|| extract_u64(config, "brokerPort"))
        .unwrap_or(1883);
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MQTT port 超出范围",
        ));
    }
    format!("{host}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("MQTT broker 无效: {e}")))
}
