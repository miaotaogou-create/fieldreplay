//! 自检用迷你 MQTT Broker（单机环回；非生产级）。

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::codec::{
    decode_packet, encode_connack, encode_pingresp, encode_puback, encode_pubcomp, encode_publish,
    encode_pubrec, encode_pubrel, encode_suback, topic_matches, Incoming, Publish,
};

struct Sub {
    filter: String,
    qos: u8,
}

struct ClientState {
    stream: TcpStream,
    subs: Vec<Sub>,
    buf: Vec<u8>,
}

struct Shared {
    clients: HashMap<u64, ClientState>,
    next_id: u64,
    next_packet_id: u16,
    /// `Some((user, pass))` 时校验 CONNECT；`None` 不鉴权。
    auth: Option<(String, String)>,
}

/// 本机迷你 Broker：accept + 轮询已连接客户端，按主题转发 PUBLISH。
pub struct MiniBroker {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MiniBroker {
    pub fn start() -> io::Result<Self> {
        Self::start_with_auth(None)
    }

    /// E9a 自检：要求 CONNECT 带匹配的 username/password。
    pub fn start_with_auth(auth: Option<(String, String)>) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);
        let shared = Arc::new(Mutex::new(Shared {
            clients: HashMap::new(),
            next_id: 1,
            next_packet_id: 1,
            auth,
        }));
        let shared_bg = Arc::clone(&shared);

        let join = thread::spawn(move || {
            if let Err(e) = run_broker(listener, shared_bg, &stop_bg) {
                eprintln!("迷你 Broker 退出: {e}");
            }
        });

        Ok(Self {
            addr,
            stop,
            join: Some(join),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for MiniBroker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run_broker(
    listener: TcpListener,
    shared: Arc<Mutex<Shared>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nodelay(true)?;
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_millis(20)))?;
                let mut g = shared.lock().unwrap();
                let id = g.next_id;
                g.next_id += 1;
                g.clients.insert(
                    id,
                    ClientState {
                        stream,
                        subs: Vec::new(),
                        buf: Vec::new(),
                    },
                );
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
        pump_clients(&shared, stop)?;
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn pump_clients(shared: &Arc<Mutex<Shared>>, stop: &AtomicBool) -> io::Result<()> {
    let ids: Vec<u64> = {
        let g = shared.lock().unwrap();
        g.clients.keys().copied().collect()
    };
    for id in ids {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        process_one(shared, id)?;
    }
    Ok(())
}

fn process_one(shared: &Arc<Mutex<Shared>>, id: u64) -> io::Result<()> {
    let mut tmp = [0u8; 4096];
    let closed = {
        let mut g = shared.lock().unwrap();
        let Some(client) = g.clients.get_mut(&id) else {
            return Ok(());
        };
        match client.stream.read(&mut tmp) {
            Ok(0) => true,
            Ok(n) => {
                client.buf.extend_from_slice(&tmp[..n]);
                false
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                false
            }
            Err(_) => true,
        }
    };
    if closed {
        shared.lock().unwrap().clients.remove(&id);
        return Ok(());
    }

    loop {
        let packet = {
            let mut g = shared.lock().unwrap();
            let Some(client) = g.clients.get_mut(&id) else {
                return Ok(());
            };
            match decode_packet(&client.buf)? {
                Some((n, pkt)) => {
                    client.buf.drain(..n);
                    Some(pkt)
                }
                None => None,
            }
        };
        let Some(pkt) = packet else {
            break;
        };
        handle_packet(shared, id, pkt)?;
    }
    Ok(())
}

fn handle_packet(shared: &Arc<Mutex<Shared>>, id: u64, pkt: Incoming) -> io::Result<()> {
    match pkt {
        Incoming::Connect {
            username,
            password,
            ..
        } => {
            let code = {
                let g = shared.lock().unwrap();
                match &g.auth {
                    None => 0u8,
                    Some((expect_user, expect_pass)) => {
                        let ok_user = username.as_deref() == Some(expect_user.as_str());
                        let ok_pass = match &password {
                            Some(p) => p.as_slice() == expect_pass.as_bytes(),
                            None => expect_pass.is_empty(),
                        };
                        if ok_user && ok_pass {
                            0
                        } else {
                            4 // bad username or password
                        }
                    }
                }
            };
            write_to(shared, id, &encode_connack(false, code))?;
            if code != 0 {
                // 拒绝后踢掉连接，避免半开客户端占槽
                let mut g = shared.lock().unwrap();
                g.clients.remove(&id);
            }
        }
        Incoming::Subscribe { packet_id, topics } => {
            let mut codes = Vec::new();
            {
                let mut g = shared.lock().unwrap();
                if let Some(client) = g.clients.get_mut(&id) {
                    for (filter, qos) in topics {
                        let q = qos.min(2);
                        client.subs.push(Sub { filter, qos: q });
                        codes.push(q);
                    }
                }
            }
            write_to(shared, id, &encode_suback(packet_id, &codes))?;
        }
        Incoming::Publish(msg) => {
            match msg.qos {
                1 => {
                    if let Some(pid) = msg.packet_id {
                        write_to(shared, id, &encode_puback(pid))?;
                    }
                }
                2 => {
                    if let Some(pid) = msg.packet_id {
                        write_to(shared, id, &encode_pubrec(pid))?;
                    }
                }
                _ => {}
            }
            fanout(shared, id, &msg)?;
        }
        Incoming::Pubrel(pid) => {
            write_to(shared, id, &encode_pubcomp(pid))?;
        }
        Incoming::Pubrec(pid) => {
            write_to(shared, id, &encode_pubrel(pid))?;
        }
        Incoming::Pingreq => {
            write_to(shared, id, &encode_pingresp())?;
        }
        Incoming::Disconnect => {
            shared.lock().unwrap().clients.remove(&id);
        }
        _ => {}
    }
    Ok(())
}

fn fanout(shared: &Arc<Mutex<Shared>>, publisher_id: u64, msg: &Publish) -> io::Result<()> {
    let targets: Vec<(u64, u8)> = {
        let g = shared.lock().unwrap();
        let mut out = Vec::new();
        for (cid, client) in g.clients.iter() {
            if *cid == publisher_id {
                continue;
            }
            for sub in &client.subs {
                if topic_matches(&sub.filter, &msg.topic) {
                    out.push((*cid, sub.qos.min(msg.qos)));
                    break;
                }
            }
        }
        out
    };

    for (cid, qos) in targets {
        let packet_id = if qos > 0 {
            Some({
                let mut g = shared.lock().unwrap();
                let id = g.next_packet_id;
                g.next_packet_id = g.next_packet_id.wrapping_add(1);
                if g.next_packet_id == 0 {
                    g.next_packet_id = 1;
                }
                id
            })
        } else {
            None
        };
        let deliver = Publish {
            dup: false,
            qos,
            retain: msg.retain,
            topic: msg.topic.clone(),
            packet_id,
            payload: msg.payload.clone(),
        };
        let bytes = encode_publish(&deliver)?;
        write_to(shared, cid, &bytes)?;
    }
    Ok(())
}

fn write_to(shared: &Arc<Mutex<Shared>>, id: u64, data: &[u8]) -> io::Result<()> {
    let mut g = shared.lock().unwrap();
    if let Some(client) = g.clients.get_mut(&id) {
        if client.stream.write_all(data).is_err() {
            g.clients.remove(&id);
        }
    }
    Ok(())
}
