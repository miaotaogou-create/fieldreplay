//! FieldReplay 引擎 CLI：`record` / `replay` / `self-check`。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fieldreplay_engine::fcap::{
    ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionReader, SessionWriter,
};
use fieldreplay_engine::{
    record_serial_with_port, record_tcp_to_file, record_udp_to_file, replay_serial_with_port,
    replay_tcp_from_file, replay_udp_from_file, Parity, RecordOptions, ReplayOptions, SerialConfig,
    SerialPort, SerialRecordOptions, SerialReplayOptions, TcpConfig, TcpRecordOptions,
    TcpReplayOptions, TcpRole,
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
FieldReplay 引擎（E3：UDP + 串口 + TCP 录 → .fcap → 按时间轴重放）

用法:
  fieldreplay-engine record  --bind <ip:port> --out <file.fcap> [--duration <秒>]
  fieldreplay-engine record  --port <COMx> [--baud <n>] [--parity none|odd|even] \\
                             [--data-bits 8] [--stop-bits 1] --out <file.fcap> [--duration <秒>]
  fieldreplay-engine record  --tcp-role <server|client> --tcp <ip:port> --out <file.fcap> \\
                             [--duration <秒>]
  fieldreplay-engine replay  --in <file.fcap> --target <ip:port>
  fieldreplay-engine replay  --in <file.fcap> --port <COMx> [--baud <n>] ...
  fieldreplay-engine replay  --in <file.fcap> --tcp <ip:port>
  fieldreplay-engine self-check

record：无 --duration 时按 Enter 停止并收尾写盘。
--bind / --port / --tcp 互斥；replay 的 --target / --port / --tcp 互斥。
TCP 重放：按文件内录制角色自动取反（server↔client），--tcp 为重放侧地址。
"
    );
}

fn cmd_record(mut args: impl Iterator<Item = String>) -> io::Result<()> {
    let mut bind: Option<SocketAddr> = None;
    let mut port_name: Option<String> = None;
    let mut tcp_endpoint: Option<SocketAddr> = None;
    let mut tcp_role: Option<TcpRole> = None;
    let mut baud: u32 = 115_200;
    let mut data_bits: u8 = 8;
    let mut stop_bits: u8 = 1;
    let mut parity = Parity::None;
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
            "--port" => {
                port_name = Some(
                    args.next()
                        .ok_or_else(|| err_arg("--port 需要串口名"))?,
                );
            }
            "--tcp" => {
                let v = args.next().ok_or_else(|| err_arg("--tcp 需要地址"))?;
                tcp_endpoint = Some(v.parse().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("tcp: {e}"))
                })?);
            }
            "--tcp-role" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--tcp-role 需要 server|client"))?;
                tcp_role = Some(
                    TcpRole::parse(&v).ok_or_else(|| err_arg("tcp-role 须为 server|client"))?,
                );
            }
            "--baud" => {
                let v = args.next().ok_or_else(|| err_arg("--baud 需要数值"))?;
                baud = v
                    .parse()
                    .map_err(|_| err_arg("--baud 须为整数"))?;
            }
            "--data-bits" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--data-bits 需要数值"))?;
                data_bits = v
                    .parse()
                    .map_err(|_| err_arg("--data-bits 须为整数"))?;
            }
            "--stop-bits" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--stop-bits 需要数值"))?;
                stop_bits = v
                    .parse()
                    .map_err(|_| err_arg("--stop-bits 须为整数"))?;
            }
            "--parity" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--parity 需要 none|odd|even"))?;
                parity = Parity::parse(&v).ok_or_else(|| err_arg("parity 须为 none|odd|even"))?;
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

    let out = out.ok_or_else(|| err_arg("缺少 --out"))?;
    let modes = [
        bind.is_some(),
        port_name.is_some(),
        tcp_endpoint.is_some() || tcp_role.is_some(),
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    if modes > 1 {
        return Err(err_arg("--bind / --port / --tcp 不能同时使用"));
    }

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

    if let Some(bind) = bind {
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
        return Ok(());
    }

    if let Some(endpoint) = tcp_endpoint {
        let role = tcp_role.ok_or_else(|| err_arg("TCP 录制需要 --tcp-role server|client"))?;
        let config = TcpConfig::new(role, endpoint.ip().to_string(), endpoint.port());
        let opts = TcpRecordOptions {
            config,
            out_path: out,
            channel_name: format!("TCP-{}-{}", role.as_str(), endpoint.port()),
            session_title: Some(format!("record-tcp-{}", endpoint.port())),
            max_duration: duration,
            poll_timeout: Duration::from_millis(200),
        };
        let n = record_tcp_to_file(&opts, &stop)?;
        println!("ok packets={n}");
        return Ok(());
    }
    if tcp_role.is_some() {
        return Err(err_arg("TCP 录制需要 --tcp <ip:port>"));
    }

    let port_name = port_name.ok_or_else(|| err_arg("缺少 --bind、--port 或 --tcp"))?;
    let config = SerialConfig {
        port_name: port_name.clone(),
        baud,
        data_bits,
        stop_bits,
        parity,
    };
    let opts = SerialRecordOptions {
        config,
        out_path: out,
        channel_name: format!("SERIAL-{port_name}"),
        session_title: Some(format!("record-{port_name}")),
        max_duration: duration,
        poll_timeout: Duration::from_millis(200),
    };
    let n = fieldreplay_engine::record_serial_to_file(&opts, &stop)?;
    println!("ok packets={n}");
    Ok(())
}

fn cmd_replay(mut args: impl Iterator<Item = String>) -> io::Result<()> {
    let mut input: Option<PathBuf> = None;
    let mut target: Option<SocketAddr> = None;
    let mut tcp_endpoint: Option<SocketAddr> = None;
    let mut port_name: Option<String> = None;
    let mut baud: u32 = 115_200;
    let mut data_bits: u8 = 8;
    let mut stop_bits: u8 = 1;
    let mut parity = Parity::None;

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
            "--tcp" => {
                let v = args.next().ok_or_else(|| err_arg("--tcp 需要地址"))?;
                tcp_endpoint = Some(v.parse().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("tcp: {e}"))
                })?);
            }
            "--port" => {
                port_name = Some(
                    args.next()
                        .ok_or_else(|| err_arg("--port 需要串口名"))?,
                );
            }
            "--baud" => {
                let v = args.next().ok_or_else(|| err_arg("--baud 需要数值"))?;
                baud = v
                    .parse()
                    .map_err(|_| err_arg("--baud 须为整数"))?;
            }
            "--data-bits" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--data-bits 需要数值"))?;
                data_bits = v
                    .parse()
                    .map_err(|_| err_arg("--data-bits 须为整数"))?;
            }
            "--stop-bits" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--stop-bits 需要数值"))?;
                stop_bits = v
                    .parse()
                    .map_err(|_| err_arg("--stop-bits 须为整数"))?;
            }
            "--parity" => {
                let v = args
                    .next()
                    .ok_or_else(|| err_arg("--parity 需要 none|odd|even"))?;
                parity = Parity::parse(&v).ok_or_else(|| err_arg("parity 须为 none|odd|even"))?;
            }
            other => return Err(err_arg(format!("未知参数: {other}"))),
        }
    }

    let input = input.ok_or_else(|| err_arg("缺少 --in"))?;
    let modes = [target.is_some(), port_name.is_some(), tcp_endpoint.is_some()]
        .iter()
        .filter(|&&x| x)
        .count();
    if modes > 1 {
        return Err(err_arg("--target / --port / --tcp 不能同时使用"));
    }

    if let Some(target) = target {
        let opts = ReplayOptions {
            in_path: input,
            target,
            local_bind: "0.0.0.0:0".parse().unwrap(),
        };
        let n = replay_udp_from_file(&opts)?;
        println!("ok sent={n}");
        return Ok(());
    }

    if let Some(endpoint) = tcp_endpoint {
        let stop = Arc::new(AtomicBool::new(false));
        let opts = TcpReplayOptions {
            in_path: input,
            endpoint,
            poll_timeout: Duration::from_millis(200),
        };
        let n = replay_tcp_from_file(&opts, &stop)?;
        println!("ok sent={n}");
        return Ok(());
    }

    let port_name = port_name.ok_or_else(|| err_arg("缺少 --target、--port 或 --tcp"))?;
    let config = SerialConfig {
        port_name,
        baud,
        data_bits,
        stop_bits,
        parity,
    };
    let opts = SerialReplayOptions {
        in_path: input,
        config,
    };
    let n = fieldreplay_engine::replay_serial_from_file(&opts)?;
    println!("ok sent={n}");
    Ok(())
}

/// 写 UDP/串口/TCP 帧往返校验；本机环回录放。
fn cmd_self_check() -> io::Result<()> {
    print!("self-check: fcap 往返… ");
    let _ = io::stdout().flush();
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "fieldreplay_selfcheck_{}.fcap",
        std::process::id()
    ));

    let channels = [
        ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "UDP-self".into(),
            config_json: r#"{"bind_ip":"127.0.0.1","bind_port":0,"peer_ip":"","peer_port":0}"#
                .into(),
        },
        ChannelEntry {
            channel_index: 1,
            channel_type: ChannelType::Serial,
            name: "SERIAL-self".into(),
            config_json: SerialConfig::new("COM3", 115_200).to_config_json(),
        },
        ChannelEntry {
            channel_index: 2,
            channel_type: ChannelType::Tcp,
            name: "TCP-self".into(),
            config_json: TcpConfig::new(TcpRole::Server, "127.0.0.1", 9000).to_config_json(),
        },
    ];
    let mut w = SessionWriter::create(&path, 1_700_000_000_000_000, &channels, None)?;
    w.append_packet(&PacketRecord {
        relative_us: 1000,
        channel_index: 0,
        kind: PacketKind::Data,
        direction: 0,
        flags: 0,
        aux: b"127.0.0.1:1".to_vec(),
        payload: b"alpha".to_vec(),
    })?;
    w.append_packet(&PacketRecord {
        relative_us: 2000,
        channel_index: 1,
        kind: PacketKind::Data,
        direction: 0,
        flags: 0,
        aux: Vec::new(),
        payload: b"serial-beta".to_vec(),
    })?;
    w.append_packet(&PacketRecord {
        relative_us: 3000,
        channel_index: 2,
        kind: PacketKind::TcpConnect,
        direction: 0,
        flags: 0,
        aux: b"127.0.0.1:1".to_vec(),
        payload: Vec::new(),
    })?;
    w.append_packet(&PacketRecord {
        relative_us: 3500,
        channel_index: 2,
        kind: PacketKind::Data,
        direction: 0,
        flags: 0,
        aux: Vec::new(),
        payload: b"tcp-gamma".to_vec(),
    })?;
    w.finalize()?;

    let mut r = SessionReader::open(&path)?;
    let pkts = r.read_all_packets()?;
    assert_eq!(pkts.len(), 4, "帧数");
    assert_eq!(r.channels()[2].channel_type, ChannelType::Tcp);
    assert_eq!(pkts[3].payload, b"tcp-gamma");
    println!("ok");

    print!("self-check: UDP 录放环回… ");
    let _ = io::stdout().flush();
    udp_loopback_check()?;
    println!("ok");

    print!("self-check: 串口内存环回录放… ");
    let _ = io::stdout().flush();
    serial_loopback_check()?;
    println!("ok");

    print!("self-check: TCP server 录 / client 放… ");
    let _ = io::stdout().flush();
    tcp_server_record_loopback()?;
    println!("ok");

    print!("self-check: TCP client 录 / server 放… ");
    let _ = io::stdout().flush();
    tcp_client_record_loopback()?;
    println!("ok");

    let _ = std::fs::remove_file(&path);
    println!("self-check 全部通过");
    Ok(())
}

fn udp_loopback_check() -> io::Result<()> {
    use std::net::UdpSocket;
    use std::thread;

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

fn serial_loopback_check() -> io::Result<()> {
    use std::thread;

    let (end_rec, end_inj) = SerialPort::open_loopback_pair(Duration::from_millis(50));
    let fcap = std::env::temp_dir().join(format!(
        "fieldreplay_serial_loop_{}.fcap",
        std::process::id()
    ));
    let fcap2 = fcap.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);

    let cfg = SerialConfig::new("LOOP-A", 115_200);
    let recorder = thread::spawn(move || {
        let opts = SerialRecordOptions {
            config: cfg,
            out_path: fcap2,
            channel_name: "SERIAL-loop".into(),
            session_title: Some("serial-loop".into()),
            max_duration: Some(Duration::from_secs(2)),
            poll_timeout: Duration::from_millis(50),
        };
        record_serial_with_port(&opts, &stop_rec, &end_rec)
    });

    thread::sleep(Duration::from_millis(80));
    end_inj.write_all(b"ping-1")?;
    thread::sleep(Duration::from_millis(40));
    end_inj.write_all(b"ping-2")?;
    thread::sleep(Duration::from_millis(120));
    stop.store(true, Ordering::SeqCst);

    let n = recorder.join().map_err(|_| err_arg("串口录制线程 panic"))??;
    if n < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("串口录制帧数不足: {n}"),
        ));
    }

    // 新环回：重放写 A，监听读 B
    let (end_play, end_listen) = SerialPort::open_loopback_pair(Duration::from_millis(200));
    let listener = thread::spawn(move || -> io::Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        let deadline = Instant::now() + Duration::from_secs(3);
        while out.len() < 2 && Instant::now() < deadline {
            if let Some(n) = end_listen.read(&mut buf)? {
                if n > 0 {
                    out.push(buf[..n].to_vec());
                }
            }
        }
        Ok(out)
    });

    let sent = replay_serial_with_port(
        &SerialReplayOptions {
            in_path: fcap.clone(),
            config: SerialConfig::new("LOOP-B", 115_200),
        },
        &end_play,
    )?;
    let got = listener
        .join()
        .map_err(|_| err_arg("串口接收线程 panic"))??;
    let _ = std::fs::remove_file(&fcap);

    if sent < 2 || got.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("串口重放不足: sent={sent} got={}", got.len()),
        ));
    }
    if got[0] != b"ping-1" || got[1] != b"ping-2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("串口载荷不一致: {got:?}"),
        ));
    }
    Ok(())
}

/// 录制角色 server → 重放取反为 client：连上监听端并写出 RX 载荷。
fn tcp_server_record_loopback() -> io::Result<()> {
    use std::thread;

    let rec_addr = {
        let l = TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?
    };

    let fcap = std::env::temp_dir().join(format!(
        "fieldreplay_tcp_srv_{}.fcap",
        std::process::id()
    ));
    let fcap2 = fcap.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);

    let recorder = thread::spawn(move || {
        let opts = TcpRecordOptions {
            config: TcpConfig::new(TcpRole::Server, rec_addr.ip().to_string(), rec_addr.port()),
            out_path: fcap2,
            channel_name: "TCP-srv".into(),
            session_title: Some("tcp-srv".into()),
            max_duration: Some(Duration::from_secs(3)),
            poll_timeout: Duration::from_millis(50),
        };
        record_tcp_to_file(&opts, &stop_rec)
    });

    thread::sleep(Duration::from_millis(120));
    {
        let mut peer = TcpStream::connect(rec_addr)?;
        peer.set_nodelay(true)?;
        peer.write_all(b"ping-1")?;
        thread::sleep(Duration::from_millis(40));
        peer.write_all(b"ping-2")?;
        thread::sleep(Duration::from_millis(80));
        // 关闭写端，录制侧读到 EOF
        drop(peer);
    }
    thread::sleep(Duration::from_millis(100));
    stop.store(true, Ordering::SeqCst);

    let n = recorder.join().map_err(|_| err_arg("TCP server 录制 panic"))??;
    // connect + ≥1 data + disconnect；TCP 可能把两包粘成一帧
    if n < 3 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("TCP server 录制帧数不足: {n}"),
        ));
    }

    let listen = TcpListener::bind("127.0.0.1:0")?;
    let play_addr = listen.local_addr()?;
    let acceptor = thread::spawn(move || -> io::Result<Vec<u8>> {
        listen.set_nonblocking(false)?;
        let (mut stream, _) = listen.accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        let deadline = Instant::now() + Duration::from_secs(3);
        while out.len() < 12 && Instant::now() < deadline {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
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

    thread::sleep(Duration::from_millis(50));
    let stop_play = Arc::new(AtomicBool::new(false));
    let sent = replay_tcp_from_file(
        &TcpReplayOptions {
            in_path: fcap.clone(),
            endpoint: play_addr,
            poll_timeout: Duration::from_millis(50),
        },
        &stop_play,
    )?;
    let got = acceptor
        .join()
        .map_err(|_| err_arg("TCP 接收线程 panic"))??;
    let _ = std::fs::remove_file(&fcap);

    if sent < 1 || got != b"ping-1ping-2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("TCP server→client 重放失败: sent={sent} got={got:?}"),
        ));
    }
    Ok(())
}

/// 录制角色 client → 重放取反为 server：等待上位机接入后写出 RX 载荷。
fn tcp_client_record_loopback() -> io::Result<()> {
    use std::thread;

    let device = TcpListener::bind("127.0.0.1:0")?;
    let device_addr = device.local_addr()?;

    // 设备侧：accept 后灌两包再关
    let feeder = thread::spawn(move || -> io::Result<()> {
        device.set_nonblocking(false)?;
        let (mut stream, _) = device.accept()?;
        stream.set_nodelay(true)?;
        stream.write_all(b"ping-1")?;
        thread::sleep(Duration::from_millis(40));
        stream.write_all(b"ping-2")?;
        thread::sleep(Duration::from_millis(40));
        Ok(())
    });

    thread::sleep(Duration::from_millis(40));

    let fcap = std::env::temp_dir().join(format!(
        "fieldreplay_tcp_cli_{}.fcap",
        std::process::id()
    ));
    let fcap2 = fcap.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);

    let recorder = thread::spawn(move || {
        let opts = TcpRecordOptions {
            config: TcpConfig::new(
                TcpRole::Client,
                device_addr.ip().to_string(),
                device_addr.port(),
            ),
            out_path: fcap2,
            channel_name: "TCP-cli".into(),
            session_title: Some("tcp-cli".into()),
            max_duration: Some(Duration::from_secs(3)),
            poll_timeout: Duration::from_millis(50),
        };
        record_tcp_to_file(&opts, &stop_rec)
    });

    feeder.join().map_err(|_| err_arg("TCP 设备灌包 panic"))??;
    thread::sleep(Duration::from_millis(150));
    stop.store(true, Ordering::SeqCst);

    let n = recorder.join().map_err(|_| err_arg("TCP client 录制 panic"))??;
    if n < 3 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("TCP client 录制帧数不足: {n}"),
        ));
    }

    // 重放为 server：先占端口，再起重放，上位机（本测）连入收包
    let play_probe = TcpListener::bind("127.0.0.1:0")?;
    let play_addr = play_probe.local_addr()?;
    drop(play_probe);

    let stop_play = Arc::new(AtomicBool::new(false));
    let fcap_play = fcap.clone();
    let replayer = thread::spawn(move || {
        replay_tcp_from_file(
            &TcpReplayOptions {
                in_path: fcap_play,
                endpoint: play_addr,
                poll_timeout: Duration::from_millis(50),
            },
            &stop_play,
        )
    });

    // 等重放 listen 起来
    let mut stream = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match TcpStream::connect(play_addr) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(30)),
        }
    }
    let mut stream = stream.ok_or_else(|| {
        io::Error::new(io::ErrorKind::TimedOut, "无法连上 TCP 重放 server")
    })?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;

    let mut got = Vec::new();
    let mut buf = [0u8; 2048];
    let read_deadline = Instant::now() + Duration::from_secs(3);
    while got.len() < 12 && Instant::now() < read_deadline {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    let sent = replayer.join().map_err(|_| err_arg("TCP server 重放 panic"))??;
    let _ = std::fs::remove_file(&fcap);

    if sent < 1 || got != b"ping-1ping-2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("TCP client→server 重放失败: sent={sent} got={got:?}"),
        ));
    }
    Ok(())
}

fn err_arg(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}
