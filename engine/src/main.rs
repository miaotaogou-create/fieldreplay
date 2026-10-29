//! FieldReplay 引擎 CLI：`record` / `replay` / `self-check`。

use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fieldreplay_engine::fcap::{
    ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionReader, SessionWriter,
};
use fieldreplay_engine::{
    record_udp_to_file, replay_udp_from_file, RecordOptions, ReplayOptions,
};

fn main() {
    if let Err(e) = run() {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "record" => cmd_record(args),
        "replay" => cmd_replay(args),
        "self-check" => cmd_self_check(),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("未知命令: {other}");
            print_help();
            Err(io::Error::new(io::ErrorKind::InvalidInput, "未知命令"))
        }
    }
}

fn print_help() {
    eprintln!(
        "\
FieldReplay 引擎（E1：UDP 录 → .fcap → 按时间轴重放）

用法:
  fieldreplay-engine record  --bind <ip:port> --out <file.fcap> [--duration <秒>]
  fieldreplay-engine replay  --in <file.fcap> --target <ip:port>
  fieldreplay-engine self-check

record：无 --duration 时按 Enter 停止并收尾写盘。
"
    );
}

fn cmd_record(mut args: impl Iterator<Item = String>) -> io::Result<()> {
    let mut bind: Option<SocketAddr> = None;
    let mut out: Option<PathBuf> = None;
    let mut duration: Option<Duration> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--bind" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--bind 需要地址"))?;
                bind = Some(v.parse().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("bind: {e}"))
                })?);
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| err_arg("--out 需要路径"))?,
                ));
            }
            "--duration" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--duration 需要秒数"))?;
                let secs: f64 = v.parse().map_err(|_| err_arg("--duration 须为数字"))?;
                duration = Some(Duration::from_secs_f64(secs));
            }
            other => return Err(err_arg(format!("未知参数: {other}"))),
        }
    }

    let bind = bind.ok_or_else(|| err_arg("缺少 --bind"))?;
    let out = out.ok_or_else(|| err_arg("缺少 --out"))?;

    let stop = Arc::new(AtomicBool::new(false));
    if duration.is_none() {
        let stop2 = Arc::clone(&stop);
        std::thread::spawn(move || {
            eprintln!("按 Enter 停止录制…");
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line);
            stop2.store(true, Ordering::SeqCst);
        });
    }

    let opts = RecordOptions {
        bind,
        out_path: out,
        channel_name: format!("UDP-{}", bind.port()),
        session_title: Some(format!("record-{}", bind.port())),
        max_duration: duration,
        poll_timeout: Duration::from_millis(200),
    };
    let n = record_udp_to_file(&opts, &stop)?;
    println!("ok packets={n}");
    Ok(())
}

fn cmd_replay(mut args: impl Iterator<Item = String>) -> io::Result<()> {
    let mut input: Option<PathBuf> = None;
    let mut target: Option<SocketAddr> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--in" => {
                input = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| err_arg("--in 需要路径"))?,
                ));
            }
            "--target" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--target 需要地址"))?;
                target = Some(v.parse().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("target: {e}"))
                })?);
            }
            other => return Err(err_arg(format!("未知参数: {other}"))),
        }
    }

    let input = input.ok_or_else(|| err_arg("缺少 --in"))?;
    let target = target.ok_or_else(|| err_arg("缺少 --target"))?;

    let opts = ReplayOptions {
        in_path: input,
        target,
        local_bind: "0.0.0.0:0".parse().unwrap(),
    };
    let n = replay_udp_from_file(&opts)?;
    println!("ok sent={n}");
    Ok(())
}

/// 写两帧 UDP → 读回校验；再做本机环回录放抽查。
fn cmd_self_check() -> io::Result<()> {
    print!("self-check: fcap 往返… ");
    let _ = io::stdout().flush();
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "fieldreplay_selfcheck_{}.fcap",
        std::process::id()
    ));

    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Udp,
        name: "UDP-self".into(),
        config_json: r#"{"bind_ip":"127.0.0.1","bind_port":0,"peer_ip":"","peer_port":0}"#
            .into(),
    }];
    let mut w = SessionWriter::create(&path, 1_700_000_000_000_000, &channels, None)?;
    let payloads: [&[u8]; 2] = [b"alpha", b"beta-payload"];
    for (i, payload) in payloads.iter().enumerate() {
        w.append_packet(&PacketRecord {
            relative_us: (i as u64 + 1) * 1_000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:1".to_vec(),
            payload: payload.to_vec(),
        })?;
    }
    w.finalize()?;

    let mut r = SessionReader::open(&path)?;
    let pkts = r.read_all_packets()?;
    assert_eq!(pkts.len(), 2, "帧数");
    assert_eq!(pkts[0].relative_us, 1000);
    assert_eq!(pkts[0].payload, b"alpha");
    assert_eq!(pkts[1].relative_us, 2000);
    assert_eq!(pkts[1].payload, b"beta-payload");
    println!("ok");

    print!("self-check: UDP 录放环回… ");
    let _ = io::stdout().flush();
    udp_loopback_check()?;
    println!("ok");

    let _ = std::fs::remove_file(&path);
    println!("self-check 全部通过");
    Ok(())
}

fn udp_loopback_check() -> io::Result<()> {
    use std::net::UdpSocket;
    use std::thread;
    use std::time::Instant;

    // 先占端口拿地址再立刻释放，避免与录制 bind 冲突
    let rec_addr = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };

    let fcap = std::env::temp_dir().join(format!("fieldreplay_loop_{}.fcap", std::process::id()));
    let fcap2 = fcap.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);

    let recorder = thread::spawn(move || {
        let opts = RecordOptions {
            bind: rec_addr,
            out_path: fcap2,
            channel_name: "loop".into(),
            session_title: Some("loop".into()),
            max_duration: Some(Duration::from_secs(2)),
            poll_timeout: Duration::from_millis(50),
        };
        record_udp_to_file(&opts, &stop_rec)
    });

    thread::sleep(Duration::from_millis(150));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.send_to(b"ping-1", rec_addr)?;
    thread::sleep(Duration::from_millis(50));
    sender.send_to(b"ping-2", rec_addr)?;
    thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::SeqCst);

    let n = recorder.join().map_err(|_| err_arg("录制线程 panic"))??;
    if n < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("录制帧数不足: {n}"),
        ));
    }

    let recv_sock = UdpSocket::bind("127.0.0.1:0")?;
    let recv_addr = recv_sock.local_addr()?;
    recv_sock.set_read_timeout(Some(Duration::from_millis(200)))?;

    let listener = thread::spawn(move || -> io::Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        let deadline = Instant::now() + Duration::from_secs(3);
        while out.len() < 2 && Instant::now() < deadline {
            match recv_sock.recv_from(&mut buf) {
                Ok((n, _)) => out.push(buf[..n].to_vec()),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    });

    let sent = replay_udp_from_file(&ReplayOptions {
        in_path: fcap.clone(),
        target: recv_addr,
        local_bind: "127.0.0.1:0".parse().unwrap(),
    })?;
    let got = listener.join().map_err(|_| err_arg("接收线程 panic"))??;
    let _ = std::fs::remove_file(&fcap);

    if sent < 2 || got.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("重放不足: sent={sent} got={}", got.len()),
        ));
    }
    if got[0] != b"ping-1" || got[1] != b"ping-2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("载荷不一致: {got:?}"),
        ));
    }
    Ok(())
}

fn err_arg(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}
