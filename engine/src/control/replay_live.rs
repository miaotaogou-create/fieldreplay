//! 控制面实时重放：`replay.start` / `pause` / `resume` / `seek` / `stop`，经 PushHub 推送进度。

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::fcap::ChannelType;
use crate::mqtt::MqttConfig;
use crate::multi_replay::{
    open_datagram_target, open_mqtt_multi_target, open_serial_multi_target, open_tcp_multi_target,
    replay_multi_from_file_with, MultiReplayOptions, MultiReplayTarget,
};
use crate::replay::{
    replay_mqtt_from_file_with, replay_serial_with_port_with, replay_tcp_from_file_with,
    replay_udp_from_file_with, replay_video_from_file_with, replay_video_rtsp_from_file_with,
    FaultInjection, MqttReplayOptions, OnReplayProgress, ReplayOptions, RtspReplayOptions,
    SerialReplayOptions, TcpReplayOptions,
};
use crate::serial::{Parity, SerialConfig, SerialPort};
use crate::tcp::{TcpConfig, TcpRole};

use super::dto::channel_type_str;
use super::hub::PushHub;
use crate::clock::SessionClock;

use super::json_util::{
    err_result, escape_str, extract_array, extract_bool, extract_f64, extract_object, extract_str,
    extract_u64, ok_result, split_json_array_elements,
};
use super::session_file::write_loaded_session;
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

    let topic_prefix = extract_str(msg, "topicPrefix")
        .or_else(|| extract_str(msg, "topic_prefix"))
        .unwrap_or_default();
    let speed = SessionClock::clamp_speed(extract_f64(msg, "speed").unwrap_or(1.0));
    let from_us = extract_u64(msg, "fromUs")
        .or_else(|| extract_u64(msg, "from_us"))
        .unwrap_or(0);
    let looping = extract_bool(msg, "loop").unwrap_or(false);
    let fault = parse_fault(msg);

    // E11c：非空 targets[] 优先；否则走旧单路 type+config
    if let Some(arr) = extract_array(msg, "targets") {
        let elems = split_json_array_elements(&arr);
        if !elems.is_empty() {
            return start_multi_replay(
                id,
                &elems,
                speed,
                from_us,
                looping,
                fault,
                state,
                hub,
            );
        }
    }

    let ty = match extract_str(msg, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None => return err_result(id, "replay.start 需要 type 或非空 targets"),
    };
    let config_raw = extract_object(msg, "config").unwrap_or_else(|| "{}".into());

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
    let paused = Arc::new(AtomicBool::new(false));
    let seek_us = Arc::new(AtomicU64::new(SessionClock::NO_SEEK));
    let finished = Arc::new(AtomicBool::new(false));
    let sent_count = Arc::new(AtomicU64::new(0));
    let hub_c = Arc::clone(hub);
    let stop_c = Arc::clone(&stop);
    let paused_c = Arc::clone(&paused);
    let seek_c = Arc::clone(&seek_us);
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
            speed,
            from_us,
            looping,
            fault,
            &stop_c,
            &paused_c,
            &seek_c,
            &sent_c,
            &hub_c,
        );
        let sent = sent_c.load(Ordering::SeqCst);
        let stopped = stop_c.load(Ordering::SeqCst);
        let done = match &result {
            Ok(_) => format!(
                "{{\"event\":\"replay.done\",\"sent\":{sent},\"stopped\":{}}}",
                if stopped { "true" } else { "false" }
            ),
            Err(e) => format!(
                "{{\"event\":\"replay.done\",\"sent\":{sent},\"stopped\":{},\"error\":\"{}\"}}",
                if stopped { "true" } else { "false" },
                escape_str(&e.to_string())
            ),
        };
        hub_c.broadcast(&done);
        finished_c.store(true, Ordering::SeqCst);
        result
    });

    state.replaying = Some(ActiveReplay {
        stop,
        paused,
        seek_us,
        finished,
        channel_type: ty.clone(),
        sent_count,
        duration_us,
        join: Some(join),
    });

    ok_result(
        id,
        &format!(
            "{{\"started\":true,\"type\":\"{}\",\"config\":{},\"durationUs\":{duration_us},\"speed\":{speed},\"fromUs\":{from_us},\"loop\":{},\"fault\":{{\"enabled\":{},\"lossRatePercent\":{},\"corruptCrc\":{}}}}}",
            escape_str(&ty),
            if config_raw.trim().is_empty() {
                "{}"
            } else {
                config_raw.as_str()
            },
            if looping { "true" } else { "false" },
            if fault.enabled { "true" } else { "false" },
            fault.loss_rate_percent,
            if fault.corrupt_payload {
                "true"
            } else {
                "false"
            }
        ),
    )
}

enum BoundMulti {
    Udp {
        ch: u16,
        expect: ChannelType,
        addr: SocketAddr,
    },
    Tcp {
        ch: u16,
        endpoint: SocketAddr,
        play_role: TcpRole,
    },
    Serial {
        ch: u16,
        config: SerialConfig,
    },
    Mqtt {
        ch: u16,
        config: MqttConfig,
        topic_prefix: String,
    },
}

fn start_multi_replay(
    id: &str,
    elems: &[String],
    speed: f64,
    from_us: u64,
    looping: bool,
    fault: FaultInjection,
    state: &mut ControlState,
    hub: &Arc<PushHub>,
) -> String {
    let session = state.session.as_ref().unwrap();
    let mut bound: Vec<BoundMulti> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut targets_json = String::from("[");

    for (i, elem) in elems.iter().enumerate() {
        let ty_s = match extract_str(elem, "type") {
            Some(t) => t.to_ascii_lowercase(),
            None => return err_result(id, &format!("targets[{i}] 缺 type")),
        };
        let ch = extract_u64(elem, "ch")
            .or_else(|| extract_u64(elem, "channel"))
            .or_else(|| extract_u64(elem, "channelIndex"))
            .unwrap_or(i as u64);
        if ch > u16::MAX as u64 {
            return err_result(id, &format!("targets[{i}] ch 超出范围"));
        }
        let ch = ch as u16;
        if !seen.insert(ch) {
            return err_result(id, &format!("targets 中通道 {ch} 重复"));
        }
        let Some(entry) = session.channels.iter().find(|c| c.channel_index == ch) else {
            return err_result(id, &format!("targets[{i}] 会话无通道 {ch}"));
        };
        let expect = match ty_s.as_str() {
            "udp" => ChannelType::Udp,
            "video" | "video_stream" => ChannelType::Video,
            "tcp" => ChannelType::Tcp,
            "serial" => ChannelType::Serial,
            "mqtt" => ChannelType::Mqtt,
            other => {
                return err_result(id, &format!("targets[{i}] 不支持的类型: {other}"));
            }
        };
        if entry.channel_type != expect {
            return err_result(
                id,
                &format!(
                    "targets[{i}] 通道 {ch} 类型不匹配：会话 {}，目标 {ty_s}",
                    channel_type_str(entry.channel_type)
                ),
            );
        }
        let config_raw = extract_object(elem, "config").unwrap_or_else(|| "{}".into());
        // 多路视频暂不支持 RTSP listen（会阻塞整会话时间轴）
        if expect == ChannelType::Video {
            if let Ok(Some(_)) = parse_rtsp_listen(&config_raw) {
                return err_result(
                    id,
                    &format!("targets[{i}] 多路对齐重放暂不支持 RTSP 服务端，请用 UDP 推送"),
                );
            }
        }
        if !targets_json.ends_with('[') {
            targets_json.push(',');
        }
        targets_json.push_str(&format!(
            "{{\"ch\":{ch},\"type\":\"{}\",\"config\":{}}}",
            escape_str(&ty_s),
            if config_raw.trim().is_empty() {
                "{}"
            } else {
                config_raw.as_str()
            }
        ));

        match expect {
            ChannelType::Udp | ChannelType::Video => {
                let addr = match parse_udp_target(&config_raw) {
                    Ok(a) => a,
                    Err(e) => return err_result(id, &format!("targets[{i}] {e}")),
                };
                bound.push(BoundMulti::Udp {
                    ch,
                    expect,
                    addr,
                });
            }
            ChannelType::Tcp => {
                let endpoint = match parse_tcp_endpoint(&config_raw) {
                    Ok(a) => a,
                    Err(e) => return err_result(id, &format!("targets[{i}] {e}")),
                };
                let recorded = match TcpConfig::from_config_json(&entry.config_json) {
                    Ok(c) => c,
                    Err(e) => {
                        return err_result(
                            id,
                            &format!("targets[{i}] 会话 TCP 通道配置无效: {e}"),
                        );
                    }
                };
                bound.push(BoundMulti::Tcp {
                    ch,
                    endpoint,
                    play_role: recorded.role.invert(),
                });
            }
            ChannelType::Serial => {
                let cfg = match parse_serial(&config_raw) {
                    Ok(c) => c,
                    Err(e) => return err_result(id, &format!("targets[{i}] {e}")),
                };
                bound.push(BoundMulti::Serial { ch, config: cfg });
            }
            ChannelType::Mqtt => {
                let (cfg, topic_prefix) = match resolve_mqtt_multi_target(&config_raw, &entry.config_json)
                {
                    Ok(v) => v,
                    Err(e) => return err_result(id, &format!("targets[{i}] {e}")),
                };
                bound.push(BoundMulti::Mqtt {
                    ch,
                    config: cfg,
                    topic_prefix,
                });
            }
        }
    }
    targets_json.push(']');

    let mut skipped_json = String::from("[");
    let mut skipped_n = 0usize;
    for entry in &session.channels {
        if seen.contains(&entry.channel_index) {
            continue;
        }
        if skipped_n > 0 {
            skipped_json.push(',');
        }
        skipped_json.push_str(&format!(
            "{{\"ch\":{},\"type\":\"{}\",\"name\":\"{}\"}}",
            entry.channel_index,
            channel_type_str(entry.channel_type),
            escape_str(&entry.name)
        ));
        skipped_n += 1;
    }
    skipped_json.push(']');

    if bound.is_empty() {
        return err_result(id, "没有可绑定的重放目标");
    }

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

    let mut sinks: Vec<MultiReplayTarget> = Vec::with_capacity(bound.len());
    for b in &bound {
        match b {
            BoundMulti::Udp {
                ch, expect, addr, ..
            } => match open_datagram_target(*ch, *expect, *addr, "0.0.0.0:0".parse().unwrap())
            {
                Ok(t) => sinks.push(MultiReplayTarget::Udp(t)),
                Err(e) => {
                    return err_result(id, &format!("打开目标通道 {ch} 失败: {e}"));
                }
            },
            BoundMulti::Tcp {
                ch,
                endpoint,
                play_role,
                ..
            } => {
                sinks.push(MultiReplayTarget::Tcp(open_tcp_multi_target(
                    *ch,
                    *endpoint,
                    *play_role,
                    Duration::from_millis(200),
                )));
            }
            BoundMulti::Serial { ch, config } => {
                match open_serial_multi_target(*ch, config, Duration::from_millis(200)) {
                    Ok(t) => sinks.push(MultiReplayTarget::Serial(t)),
                    Err(e) => {
                        return err_result(id, &format!("打开目标通道 {ch} 失败: {e}"));
                    }
                }
            }
            BoundMulti::Mqtt {
                ch,
                config,
                topic_prefix,
            } => match open_mqtt_multi_target(
                *ch,
                config,
                topic_prefix.clone(),
                None,
                Duration::from_millis(200),
            ) {
                Ok(t) => sinks.push(MultiReplayTarget::Mqtt(t)),
                Err(e) => {
                    return err_result(id, &format!("打开目标通道 {ch} 失败: {e}"));
                }
            },
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let seek_us = Arc::new(AtomicU64::new(SessionClock::NO_SEEK));
    let finished = Arc::new(AtomicBool::new(false));
    let sent_count = Arc::new(AtomicU64::new(0));
    let hub_c = Arc::clone(hub);
    let stop_c = Arc::clone(&stop);
    let paused_c = Arc::clone(&paused);
    let seek_c = Arc::clone(&seek_us);
    let finished_c = Arc::clone(&finished);
    let sent_c = Arc::clone(&sent_count);
    let path_c = in_path;

    let join = thread::spawn(move || {
        let hub = Arc::clone(&hub_c);
        let count = Arc::clone(&sent_c);
        let mut hook = move |sent: u64, relative_us: u64| {
            count.store(sent, Ordering::SeqCst);
            hub.broadcast(&format!(
                "{{\"event\":\"replay.progress\",\"sent\":{sent},\"relativeUs\":{relative_us},\"durationUs\":{duration_us}}}"
            ));
        };
        let mut on: OnReplayProgress<'_> = Some(&mut hook);
        let result = replay_multi_from_file_with(
            &MultiReplayOptions {
                in_path: path_c,
                speed,
                fault,
                looping,
            },
            &mut sinks,
            &stop_c,
            &paused_c,
            &seek_c,
            from_us,
            &mut on,
        );
        let sent = sent_c.load(Ordering::SeqCst);
        let stopped = stop_c.load(Ordering::SeqCst);
        let done = match &result {
            Ok(_) => format!(
                "{{\"event\":\"replay.done\",\"sent\":{sent},\"stopped\":{}}}",
                if stopped { "true" } else { "false" }
            ),
            Err(e) => format!(
                "{{\"event\":\"replay.done\",\"sent\":{sent},\"stopped\":{},\"error\":\"{}\"}}",
                if stopped { "true" } else { "false" },
                escape_str(&e.to_string())
            ),
        };
        hub_c.broadcast(&done);
        finished_c.store(true, Ordering::SeqCst);
        result
    });

    state.replaying = Some(ActiveReplay {
        stop,
        paused,
        seek_us,
        finished,
        channel_type: "multi".into(),
        sent_count,
        duration_us,
        join: Some(join),
    });

    ok_result(
        id,
        &format!(
            "{{\"started\":true,\"targets\":{targets_json},\"skipped\":{skipped_json},\"durationUs\":{duration_us},\"speed\":{speed},\"fromUs\":{from_us},\"loop\":{},\"fault\":{{\"enabled\":{},\"lossRatePercent\":{},\"corruptCrc\":{}}}}}",
            if looping { "true" } else { "false" },
            if fault.enabled { "true" } else { "false" },
            fault.loss_rate_percent,
            if fault.corrupt_payload {
                "true"
            } else {
                "false"
            }
        ),
    )
}

pub fn handle_replay_pause(id: &str, state: &mut ControlState) -> String {
    state.reap_finished_replay();
    let Some(active) = state.replaying.as_ref() else {
        return err_result(id, "当前未在重放");
    };
    if active.paused.load(Ordering::SeqCst) {
        return ok_result(id, "{\"paused\":true,\"already\":true}");
    }
    active.paused.store(true, Ordering::SeqCst);
    ok_result(id, "{\"paused\":true}")
}

pub fn handle_replay_resume(id: &str, state: &mut ControlState) -> String {
    state.reap_finished_replay();
    let Some(active) = state.replaying.as_ref() else {
        return err_result(id, "当前未在重放");
    };
    if !active.paused.load(Ordering::SeqCst) {
        return ok_result(id, "{\"paused\":false,\"already\":true}");
    }
    active.paused.store(false, Ordering::SeqCst);
    ok_result(id, "{\"paused\":false}")
}

pub fn handle_replay_seek(id: &str, msg: &str, state: &mut ControlState) -> String {
    state.reap_finished_replay();
    let Some(active) = state.replaying.as_ref() else {
        return err_result(id, "当前未在重放，无法 seek");
    };
    let relative_us = match extract_u64(msg, "relativeUs").or_else(|| extract_u64(msg, "relative_us"))
    {
        Some(v) => v,
        None => return err_result(id, "replay.seek 需要 relativeUs"),
    };
    if relative_us > active.duration_us && active.duration_us > 0 {
        // 允许略超尾：夹到 duration
        active
            .seek_us
            .store(active.duration_us, Ordering::SeqCst);
        return ok_result(
            id,
            &format!(
                "{{\"seeked\":true,\"relativeUs\":{},\"clamped\":true}}",
                active.duration_us
            ),
        );
    }
    active.seek_us.store(relative_us, Ordering::SeqCst);
    ok_result(
        id,
        &format!("{{\"seeked\":true,\"relativeUs\":{relative_us},\"clamped\":false}}"),
    )
}

pub fn handle_replay_stop(
    id: &str,
    state_replaying: &mut Option<ActiveReplay>,
) -> String {
    let Some(mut active) = state_replaying.take() else {
        return err_result(id, "当前未在重放");
    };
    // 若卡在暂停等待，先解除以便线程能看到 stop
    active.paused.store(false, Ordering::SeqCst);
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
    write_loaded_session(&path, session, None)?;
    Ok(path)
}

fn run_replay(
    ty: &str,
    config: &str,
    in_path: &Path,
    topic_prefix: &str,
    duration_us: u64,
    speed: f64,
    from_us: u64,
    looping: bool,
    fault: FaultInjection,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
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
    let fault = fault.clamped();

    match ty {
        "udp" => {
            let target = parse_udp_target(config)?;
            let opts = ReplayOptions {
                in_path: in_path.to_path_buf(),
                target,
                local_bind: "0.0.0.0:0".parse().unwrap(),
                speed,
                fault,
                looping,
            };
            replay_udp_from_file_with(&opts, stop, paused, seek_us, from_us, &mut on)
        }
        "video" | "video_stream" => {
            // E7d：mode=rtsp_listen 或 listen_port → RTSP 服务端；否则 UDP 推送
            if let Some(listen) = parse_rtsp_listen(config)? {
                let opts = RtspReplayOptions {
                    in_path: in_path.to_path_buf(),
                    listen,
                    speed,
                    fault,
                    looping,
                };
                replay_video_rtsp_from_file_with(&opts, stop, paused, seek_us, from_us, &mut on)
            } else {
                let target = parse_udp_target(config)?;
                let opts = ReplayOptions {
                    in_path: in_path.to_path_buf(),
                    target,
                    local_bind: "0.0.0.0:0".parse().unwrap(),
                    speed,
                    fault,
                    looping,
                };
                replay_video_from_file_with(&opts, stop, paused, seek_us, from_us, &mut on)
            }
        }
        "serial" => {
            let cfg = parse_serial(config)?;
            let opts = SerialReplayOptions {
                in_path: in_path.to_path_buf(),
                config: cfg,
                speed,
                fault,
                looping,
            };
            let port = SerialPort::open(&opts.config, poll)?;
            replay_serial_with_port_with(&opts, &port, stop, paused, seek_us, from_us, &mut on)
        }
        "tcp" => {
            let endpoint = parse_tcp_endpoint(config)?;
            let opts = TcpReplayOptions {
                in_path: in_path.to_path_buf(),
                endpoint,
                poll_timeout: poll,
                speed,
                fault,
                looping,
            };
            replay_tcp_from_file_with(&opts, stop, paused, seek_us, from_us, &mut on)
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
            let username = extract_str(config, "username").filter(|s| !s.is_empty());
            let password = extract_str(config, "password").filter(|s| !s.is_empty());
            let tls = extract_bool(config, "tls");
            let ca_cert_path = extract_str(config, "ca_cert_path")
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from);
            let tls_server_name =
                extract_str(config, "tls_server_name").filter(|s| !s.is_empty());
            let opts = MqttReplayOptions {
                in_path: in_path.to_path_buf(),
                broker,
                topic_prefix: prefix,
                qos_override: None,
                username,
                password,
                tls,
                ca_cert_path,
                tls_server_name,
                poll_timeout: poll,
                speed,
                fault,
                looping,
            };
            replay_mqtt_from_file_with(&opts, stop, paused, seek_us, from_us, &mut on)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("不支持的重放类型: {other}"),
        )),
    }
}

/// 解析 `fault` 对象或顶层 `lossRatePercent` / `corruptCrc`。
fn parse_fault(msg: &str) -> FaultInjection {
    let src = extract_object(msg, "fault").unwrap_or_else(|| msg.to_string());
    let enabled = extract_bool(&src, "enabled").unwrap_or_else(|| {
        // 无 enabled 时：有丢包率或篡改开关即视为启用
        extract_u64(&src, "lossRatePercent")
            .or_else(|| extract_u64(&src, "loss_rate_percent"))
            .map(|v| v > 0)
            .unwrap_or(false)
            || extract_bool(&src, "corruptCrc")
                .or_else(|| extract_bool(&src, "corrupt_crc"))
                .or_else(|| extract_bool(&src, "corruptPayload"))
                .unwrap_or(false)
    });
    let loss = extract_u64(&src, "lossRatePercent")
        .or_else(|| extract_u64(&src, "loss_rate_percent"))
        .unwrap_or(0)
        .min(100) as u8;
    let corrupt = extract_bool(&src, "corruptCrc")
        .or_else(|| extract_bool(&src, "corrupt_crc"))
        .or_else(|| extract_bool(&src, "corruptPayload"))
        .unwrap_or(false);
    FaultInjection {
        enabled,
        loss_rate_percent: loss,
        corrupt_payload: corrupt,
    }
    .clamped()
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

/// `mode=rtsp_listen` 或存在 `listen_port` → RTSP 服务端；否则走 UDP 推送。
fn parse_rtsp_listen(config: &str) -> io::Result<Option<SocketAddr>> {
    let mode = extract_str(config, "mode").unwrap_or_default();
    let listen_port =
        extract_u64(config, "listen_port").or_else(|| extract_u64(config, "rtsp_port"));
    if !mode.eq_ignore_ascii_case("rtsp_listen") && listen_port.is_none() {
        return Ok(None);
    }
    let port = listen_port.unwrap_or(8554);
    if port > u16::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "listen_port 超出范围",
        ));
    }
    let ip = extract_str(config, "listen_ip")
        .or_else(|| extract_str(config, "bind_ip"))
        .unwrap_or_else(|| "0.0.0.0".into());
    format!("{ip}:{port}")
        .parse()
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("RTSP listen 无效: {e}")))
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

/// 多路 MQTT：目标 config 覆盖 broker/凭据/TLS；topic 回退会话通道（仅 CONNECT 占位，发布用帧 aux）。
fn resolve_mqtt_multi_target(
    target_config: &str,
    recorded_config_json: &str,
) -> io::Result<(MqttConfig, String)> {
    let recorded = MqttConfig::from_config_json(recorded_config_json).unwrap_or_else(|_| {
        MqttConfig::new("127.0.0.1", 1883, "#", 0)
    });
    let broker = parse_mqtt_broker(target_config)?;
    let topic_prefix = extract_str(target_config, "topic_prefix")
        .or_else(|| extract_str(target_config, "topicPrefix"))
        .unwrap_or_default();
    let username = extract_str(target_config, "username")
        .filter(|s| !s.is_empty())
        .or_else(|| recorded.username.clone());
    let password = extract_str(target_config, "password")
        .filter(|s| !s.is_empty())
        .or_else(|| recorded.password.clone());
    let tls = extract_bool(target_config, "tls").unwrap_or(recorded.tls);
    let ca_cert_path = extract_str(target_config, "ca_cert_path")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| recorded.ca_cert_path.clone());
    let tls_server_name = extract_str(target_config, "tls_server_name")
        .filter(|s| !s.is_empty())
        .or_else(|| recorded.tls_server_name.clone());
    let config = MqttConfig::new(
        broker.ip().to_string(),
        broker.port(),
        recorded.topic.clone(),
        recorded.qos,
    )
    .with_credentials(username, password)
    .with_tls(tls, ca_cert_path, tls_server_name);
    Ok((config, topic_prefix))
}
