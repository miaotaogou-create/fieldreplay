//! MVP 真实 IO 端到端验收自检（本机环回；不依赖 UI）。
//!
//! - UDP / TCP：操作系统真套接字
//! - 串口：内存环回（无 COM/虚口时仍可验录放路径；实口另手测）
//! - 高速率：验证写盘帧数、时间戳单调；重放抽查间隔与字节

use std::io::{self, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::fcap::SessionReader;
use crate::record::{
    record_serial_with_port, record_udp_to_file, RecordOptions, SerialRecordOptions,
};
use crate::replay::{
    replay_serial_with_port, replay_udp_from_file, FaultInjection, ReplayOptions,
    SerialReplayOptions,
};
use crate::serial::{SerialConfig, SerialPort};

const HIGH_RATE_N: u32 = 2_000;
const HIGH_RATE_PAYLOAD: usize = 64;

/// 跑完全部 IO 验收项；任一项失败即返回 Err。
pub fn run_io_acceptance() -> io::Result<()> {
    print!("io-accept: UDP+串口会话文件… ");
    let _ = io::stdout().flush();
    udp_and_serial_session_file()?;
    println!("ok");

    print!("io-accept: UDP 高速率灌入写盘… ");
    let _ = io::stdout().flush();
    udp_high_rate_check()?;
    println!("ok");

    print!("io-accept: UDP 重放间隔抽查… ");
    let _ = io::stdout().flush();
    udp_replay_interval_check()?;
    println!("ok");

    Ok(())
}

/// 验收：至少 1 路 UDP + 1 路串口均可录制并生成 `.fcap`（串口走内存环回）。
fn udp_and_serial_session_file() -> io::Result<()> {
    // —— UDP ——
    let rec_addr = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let fcap_udp = temp_fcap("io_udp");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);
    let out = fcap_udp.clone();
    let recorder = thread::spawn(move || {
        record_udp_to_file(
            &RecordOptions {
                bind: rec_addr,
                out_path: out,
                channel_name: "UDP-accept".into(),
                session_title: Some("io-accept-udp".into()),
                max_duration: Some(Duration::from_secs(2)),
                poll_timeout: Duration::from_millis(50),
            },
            &stop_rec,
        )
    });
    thread::sleep(Duration::from_millis(80));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.send_to(b"udp-accept-1", rec_addr)?;
    thread::sleep(Duration::from_millis(40));
    stop.store(true, Ordering::SeqCst);
    let n = recorder.join().map_err(|_| join_err("UDP 录制"))??;
    if n < 1 {
        return Err(other(format!("UDP 录制帧数不足: {n}")));
    }
    {
        let mut r = SessionReader::open(&fcap_udp)?;
        let pkts = r.read_all_packets()?;
        if pkts.is_empty() || pkts[0].payload != b"udp-accept-1" {
            return Err(other("UDP 会话载荷不符"));
        }
        if r.channels().len() != 1 || r.channels()[0].channel_type != crate::fcap::ChannelType::Udp
        {
            return Err(other("UDP 通道表不符"));
        }
    }
    let _ = std::fs::remove_file(&fcap_udp);

    // —— 串口（内存环回）——
    let (end_rec, end_inj) = SerialPort::open_loopback_pair(Duration::from_millis(50));
    let fcap_ser = temp_fcap("io_serial");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);
    let out = fcap_ser.clone();
    let recorder = thread::spawn(move || {
        record_serial_with_port(
            &SerialRecordOptions {
                config: SerialConfig::new("LOOP-accept", 115_200),
                out_path: out,
                channel_name: "SERIAL-accept".into(),
                session_title: Some("io-accept-serial".into()),
                max_duration: Some(Duration::from_secs(2)),
                poll_timeout: Duration::from_millis(50),
            },
            &stop_rec,
            &end_rec,
        )
    });
    thread::sleep(Duration::from_millis(60));
    end_inj.write_all(b"serial-accept-1")?;
    thread::sleep(Duration::from_millis(80));
    stop.store(true, Ordering::SeqCst);
    let n = recorder.join().map_err(|_| join_err("串口录制"))??;
    if n < 1 {
        return Err(other(format!("串口录制帧数不足: {n}")));
    }
    {
        let mut r = SessionReader::open(&fcap_ser)?;
        let pkts = r.read_all_packets()?;
        if pkts.is_empty() || pkts[0].payload != b"serial-accept-1" {
            return Err(other("串口会话载荷不符"));
        }
    }

    // 重放串口抽查字节
    let (end_play, end_listen) = SerialPort::open_loopback_pair(Duration::from_millis(200));
    let listener = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut buf = [0u8; 256];
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(n) = end_listen.read(&mut buf)? {
                if n > 0 {
                    return Ok(buf[..n].to_vec());
                }
            }
        }
        Err(other("串口重放超时无数据"))
    });
    let _ = replay_serial_with_port(
        &SerialReplayOptions {
            in_path: fcap_ser.clone(),
            config: SerialConfig::new("LOOP-accept-play", 115_200),
            speed: 1.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &end_play,
    )?;
    let got = listener.join().map_err(|_| join_err("串口接收"))??;
    let _ = std::fs::remove_file(&fcap_ser);
    if got != b"serial-accept-1" {
        return Err(other(format!("串口重放载荷不一致: {got:?}")));
    }
    Ok(())
}

/// 验收：持续高速率 UDP 灌入时写盘帧数完整、时间戳单调不减。
fn udp_high_rate_check() -> io::Result<()> {
    let rec_addr = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let fcap = temp_fcap("io_hirate");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);
    let out = fcap.clone();
    let recorder = thread::spawn(move || {
        record_udp_to_file(
            &RecordOptions {
                bind: rec_addr,
                out_path: out,
                channel_name: "UDP-hirate".into(),
                session_title: Some("io-accept-hirate".into()),
                max_duration: Some(Duration::from_secs(8)),
                poll_timeout: Duration::from_millis(20),
            },
            &stop_rec,
        )
    });

    thread::sleep(Duration::from_millis(100));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    let mut payload = vec![0u8; HIGH_RATE_PAYLOAD];
    for i in 0..HIGH_RATE_N {
        payload[0..4].copy_from_slice(&i.to_le_bytes());
        sender.send_to(&payload, rec_addr)?;
        // 持续高速率（约 1 万+ 包/秒），避免无节制突发打满内核队列
        if i & 15 == 15 {
            thread::sleep(Duration::from_millis(1));
        }
    }
    thread::sleep(Duration::from_millis(400));
    stop.store(true, Ordering::SeqCst);
    let n = recorder.join().map_err(|_| join_err("高速率录制"))??;
    if n != u64::from(HIGH_RATE_N) {
        let _ = std::fs::remove_file(&fcap);
        return Err(other(format!(
            "高速率丢帧: 期望 {HIGH_RATE_N} 实得 {n}"
        )));
    }

    let mut r = SessionReader::open(&fcap)?;
    let pkts = r.read_all_packets()?;
    let _ = std::fs::remove_file(&fcap);
    if pkts.len() != HIGH_RATE_N as usize {
        return Err(other(format!("读回帧数不符: {}", pkts.len())));
    }
    let mut prev_us = 0u64;
    for (i, p) in pkts.iter().enumerate() {
        if p.relative_us < prev_us {
            return Err(other(format!(
                "时间戳非单调: i={i} prev={prev_us} cur={}",
                p.relative_us
            )));
        }
        prev_us = p.relative_us;
        let seq = u32::from_le_bytes(p.payload[0..4].try_into().unwrap());
        if seq as usize != i {
            return Err(other(format!("序号乱序: i={i} seq={seq}")));
        }
    }
    Ok(())
}

/// 验收：按原间隔重放 UDP，抽查墙钟间隔与字节。
fn udp_replay_interval_check() -> io::Result<()> {
    let rec_addr = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let fcap = temp_fcap("io_interval");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_rec = Arc::clone(&stop);
    let out = fcap.clone();
    let recorder = thread::spawn(move || {
        record_udp_to_file(
            &RecordOptions {
                bind: rec_addr,
                out_path: out,
                channel_name: "UDP-interval".into(),
                session_title: Some("io-accept-interval".into()),
                max_duration: Some(Duration::from_secs(3)),
                poll_timeout: Duration::from_millis(50),
            },
            &stop_rec,
        )
    });

    thread::sleep(Duration::from_millis(80));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.send_to(b"t0", rec_addr)?;
    thread::sleep(Duration::from_millis(100));
    sender.send_to(b"t1", rec_addr)?;
    thread::sleep(Duration::from_millis(100));
    sender.send_to(b"t2", rec_addr)?;
    thread::sleep(Duration::from_millis(120));
    stop.store(true, Ordering::SeqCst);
    let n = recorder.join().map_err(|_| join_err("间隔录制"))??;
    if n < 3 {
        let _ = std::fs::remove_file(&fcap);
        return Err(other(format!("间隔录制帧数不足: {n}")));
    }

    let mut r = SessionReader::open(&fcap)?;
    let pkts = r.read_all_packets()?;
    if pkts.len() < 3 {
        let _ = std::fs::remove_file(&fcap);
        return Err(other("间隔会话帧不足"));
    }
    let gap01 = pkts[1].relative_us.saturating_sub(pkts[0].relative_us);
    let gap12 = pkts[2].relative_us.saturating_sub(pkts[1].relative_us);
    // 录制侧约 100ms；允许较大抖动（调度 + 超时轮询）
    if gap01 < 40_000 || gap01 > 250_000 || gap12 < 40_000 || gap12 > 250_000 {
        let _ = std::fs::remove_file(&fcap);
        return Err(other(format!(
            "录制间隔异常: gap01={gap01}us gap12={gap12}us"
        )));
    }

    let recv_sock = UdpSocket::bind("127.0.0.1:0")?;
    let recv_addr = recv_sock.local_addr()?;
    recv_sock.set_read_timeout(Some(Duration::from_millis(50)))?;

    let listener = thread::spawn(move || -> io::Result<Vec<(Instant, Vec<u8>)>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        let deadline = Instant::now() + Duration::from_secs(4);
        while out.len() < 3 && Instant::now() < deadline {
            match recv_sock.recv_from(&mut buf) {
                Ok((n, _)) => out.push((Instant::now(), buf[..n].to_vec())),
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
        speed: 1.0,
        fault: FaultInjection::OFF,
        looping: false,
    })?;
    let got = listener.join().map_err(|_| join_err("间隔接收"))??;
    let _ = std::fs::remove_file(&fcap);

    if sent < 3 || got.len() < 3 {
        return Err(other(format!(
            "间隔重放不足: sent={sent} got={}",
            got.len()
        )));
    }
    if got[0].1 != b"t0" || got[1].1 != b"t1" || got[2].1 != b"t2" {
        return Err(other(format!("间隔重放载荷不一致: {:?}", got)));
    }

    let wall01 = got[1].0.duration_since(got[0].0).as_micros() as u64;
    let wall12 = got[2].0.duration_since(got[1].0).as_micros() as u64;
    // 相对录制间隔：允许 ±60%（调度噪声）
    let ok_gap = |wall: u64, rec: u64| {
        let lo = rec * 40 / 100;
        let hi = rec * 160 / 100;
        wall >= lo && wall <= hi
    };
    if !ok_gap(wall01, gap01) || !ok_gap(wall12, gap12) {
        return Err(other(format!(
            "重放间隔偏离过大: wall01={wall01} rec01={gap01} wall12={wall12} rec12={gap12}"
        )));
    }
    Ok(())
}

fn temp_fcap(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "fieldreplay_{}_{}.fcap",
        tag,
        std::process::id()
    ))
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.into())
}

fn join_err(who: &str) -> io::Error {
    other(format!("{who} 线程 panic"))
}
