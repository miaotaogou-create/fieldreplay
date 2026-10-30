//! 控制面实时录制：`record.start` / `record.stop`，经 PushHub 推送预览帧。

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::fcap::{ChannelType, PacketRecord, SessionReader};
use crate::mqtt::MqttConfig;
use crate::multi_record::{
    record_multi_to_file, ChannelJob, LiveRecordSink, MultiRecordOptions, PacketHook,
};
use crate::serial::{Parity, SerialConfig};
use crate::tcp::{TcpConfig, TcpRole};

use super::dto::packet_event_json;
use super::hub::PushHub;
use super::json_util::{
    err_result, escape_str, extract_array, extract_object, extract_str, extract_u64, ok_result,
    split_json_array_elements,
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

    let session_title = extract_str(msg, "sessionTitle");
    let out = extract_str(msg, "out").unwrap_or_else(default_out_path);
    let out_path = PathBuf::from(&out);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // E11a：非空 channels[] 优先；单路 type+config 也走多路写盘（共享 LiveRecordSink）
    if let Some(arr) = extract_array(msg, "channels") {
        let elems = split_json_array_elements(&arr);
        if !elems.is_empty() {
            return start_multi_record(
                id,
                &elems,
                session_title,
                out_path,
                state_recording,
                hub,
                "multi".into(),
            );
        }
    }

    let ty = match extract_str(msg, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None => return err_result(id, "record.start 需要 type 或非空 channels"),
    };
    let name = extract_str(msg, "name").unwrap_or_else(|| ty.clone());
    let config_raw = extract_object(msg, "config").unwrap_or_else(|| "{}".into());
    let elem = format!(
        "{{\"type\":\"{}\",\"name\":\"{}\",\"config\":{}}}",
        escape_str(&ty),
        escape_str(&name),
        if config_raw.trim().is_empty() {
            "{}"
        } else {
            config_raw.as_str()
        }
    );
    start_multi_record(
        id,
        &[elem],
        session_title,
        out_path,
        state_recording,
        hub,
        ty,
    )
}

fn start_multi_record(
    id: &str,
    elems: &[String],
    session_title: Option<String>,
    out_path: PathBuf,
    state_recording: &mut Option<ActiveRecording>,
    hub: &Arc<PushHub>,
    channel_type: String,
) -> String {
    let mut jobs = Vec::with_capacity(elems.len());
    let mut channels_json = String::from("[");
    for (i, elem) in elems.iter().enumerate() {
        let job = match parse_channel_job(elem, i) {
            Ok(j) => j,
            Err(e) => return err_result(id, &e.to_string()),
        };
        if i > 0 {
            channels_json.push(',');
        }
        channels_json.push_str(&format!(
            "{{\"index\":{i},\"type\":\"{}\",\"name\":\"{}\"}}",
            escape_str(&channel_job_type_str(&job)),
            escape_str(job.name())
        ));
        jobs.push(job);
    }
    channels_json.push(']');

    let stop = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicU64::new(0));
    let live_sink: Arc<Mutex<Option<Arc<LiveRecordSink>>>> = Arc::new(Mutex::new(None));
    let hub_c = Arc::clone(hub);
    let stop_c = Arc::clone(&stop);
    let count_c = Arc::clone(&packet_count);
    let live_c = Arc::clone(&live_sink);
    let out_c = out_path.clone();
    let title_c = session_title;

    // 预览：按帧类型推断推送 type 字符串
    let type_by_ch: Vec<String> = jobs.iter().map(channel_job_type_str).collect();
    let reply_single = jobs.len() == 1 && channel_type != "multi";
    let single_channel_json = reply_single.then(|| {
        let job = &jobs[0];
        format!(
            "{{\"index\":0,\"type\":\"{}\",\"name\":\"{}\"}}",
            escape_str(&channel_job_type_str(job)),
            escape_str(job.name())
        )
    });

    let join = thread::spawn(move || {
        let hub_hook = Arc::clone(&hub_c);
        let count_hook = Arc::clone(&count_c);
        let types = type_by_ch;
        let hook: PacketHook = Arc::new(move |pkt: &PacketRecord| {
            let i = count_hook.fetch_add(1, Ordering::SeqCst);
            let ty = types
                .get(pkt.channel_index as usize)
                .map(|s| s.as_str())
                .unwrap_or("udp");
            let ch_ty = parse_channel_type(ty).unwrap_or(ChannelType::Udp);
            hub_hook.broadcast(&packet_event_json(i, pkt, ch_ty));
        });
        let result = record_multi_to_file(
            &MultiRecordOptions {
                out_path: out_c,
                session_title: title_c,
                max_duration: None,
                poll_timeout: Duration::from_millis(200),
            },
            &jobs,
            &stop_c,
            Some(hook),
            Some(live_c),
        );
        if let Err(ref e) = result {
            hub_c.broadcast(&format!(
                "{{\"event\":\"record.error\",\"error\":\"{}\"}}",
                escape_str(&e.to_string())
            ));
        }
        result
    });

    *state_recording = Some(ActiveRecording {
        stop,
        out_path: out_path.clone(),
        channel_type,
        packet_count,
        join: Some(join),
        live_sink,
    });

    if let Some(channel_json) = single_channel_json {
        ok_result(
            id,
            &format!(
                "{{\"started\":true,\"out\":\"{}\",\"channel\":{channel_json}}}",
                escape_str(&out_path.to_string_lossy())
            ),
        )
    } else {
        ok_result(
            id,
            &format!(
                "{{\"started\":true,\"out\":\"{}\",\"channels\":{channels_json}}}",
                escape_str(&out_path.to_string_lossy())
            ),
        )
    }
}

fn channel_job_type_str(job: &ChannelJob) -> String {
    match job {
        ChannelJob::Udp { .. } => "udp".into(),
        ChannelJob::Video { .. } => "video".into(),
        ChannelJob::Serial { .. } => "serial".into(),
        ChannelJob::Tcp { .. } => "tcp".into(),
        ChannelJob::Mqtt { .. } => "mqtt".into(),
    }
}

fn parse_channel_job(elem: &str, index: usize) -> io::Result<ChannelJob> {
    let ty = extract_str(elem, "type")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("channels[{index}] 缺 type")))?
        .to_ascii_lowercase();
    let name = extract_str(elem, "name").unwrap_or_else(|| format!("{ty}-{index}"));
    let config = extract_object(elem, "config").unwrap_or_else(|| "{}".into());
    match ty.as_str() {
        "udp" => Ok(ChannelJob::Udp {
            name,
            bind: parse_udp_bind(&config)?,
        }),
        "video" | "video_stream" => {
            let rtsp = extract_str(&config, "rtsp_url").or_else(|| extract_str(&config, "streamUrl"));
            let has_rtsp = rtsp.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false);
            let bind = if has_rtsp {
                parse_udp_bind(&config).unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap())
            } else {
                parse_udp_bind(&config)?
            };
            Ok(ChannelJob::Video {
                name,
                bind,
                rtsp_url: rtsp,
            })
        }
        "serial" => Ok(ChannelJob::Serial {
            name,
            config: parse_serial(&config)?,
        }),
        "tcp" => {
            let cfg = TcpConfig::from_config_json(&config).or_else(|_| parse_tcp_loose(&config))?;
            Ok(ChannelJob::Tcp { name, config: cfg })
        }
        "mqtt" => Ok(ChannelJob::Mqtt {
            name,
            config: MqttConfig::from_config_json(&config)?,
        }),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("channels[{index}] 不支持的类型: {other}"),
        )),
    }
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

fn parse_channel_type(ty: &str) -> io::Result<ChannelType> {
    match ty {
        "udp" => Ok(ChannelType::Udp),
        "serial" => Ok(ChannelType::Serial),
        "tcp" => Ok(ChannelType::Tcp),
        "mqtt" => Ok(ChannelType::Mqtt),
        "video" | "video_stream" => Ok(ChannelType::Video),
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
