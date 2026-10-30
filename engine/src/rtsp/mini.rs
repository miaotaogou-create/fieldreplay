//! 自检用迷你 RTSP 服务端：答 DESCRIBE/SETUP/PLAY 后交织吐块（非生产级）。

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::client::encode_interleaved;
use super::proto::{handle_request, read_request};

/// 本机迷你 RTSP：单连接；PLAY 后按序发送预设载荷（通道 0）。
pub struct MiniRtspServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl MiniRtspServer {
    pub fn start(payloads: Vec<Vec<u8>>) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);

        let join = thread::spawn(move || {
            if let Err(e) = run_once(listener, payloads, &stop_bg) {
                eprintln!("迷你 RTSP 退出: {e}");
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

    pub fn url(&self, path: &str) -> String {
        let p = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("rtsp://{}:{}{p}", self.addr.ip(), self.addr.port())
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for MiniRtspServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run_once(
    listener: TcpListener,
    payloads: Vec<Vec<u8>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut stream = None;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((s, _)) => {
                stream = Some(s);
                break;
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
    let Some(mut stream) = stream else {
        return Ok(());
    };
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut buf = Vec::new();
    let mut session = String::from("1");
    let mut playing = false;
    let mut sent = 0usize;

    while !stop.load(Ordering::SeqCst) {
        if playing && sent < payloads.len() {
            let frame = encode_interleaved(0, &payloads[sent]);
            stream.write_all(&frame)?;
            sent += 1;
            thread::sleep(Duration::from_millis(30));
            continue;
        }

        match read_request(&mut stream, &mut buf) {
            Ok(None) => continue,
            Ok(Some(req)) => match handle_request(&mut stream, &req, &mut session) {
                Ok(true) => playing = true,
                Ok(false) => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => break,
                Err(e) => return Err(e),
            },
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
