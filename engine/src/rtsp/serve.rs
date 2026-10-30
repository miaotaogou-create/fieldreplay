//! RTSP 服务侧重放：listen → 握手至 PLAY → 返回可写交织流。

use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::proto::{handle_request, read_request};

/// 接受一路客户端，完成 OPTIONS→…→PLAY 后返回已就绪的 TCP 流。
pub fn accept_until_play(listener: &TcpListener, stop: &AtomicBool) -> io::Result<TcpStream> {
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
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "RTSP 等待接入已停止",
        ));
    };
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut buf = Vec::new();
    let mut session = String::from("1");
    while !stop.load(Ordering::SeqCst) {
        match read_request(&mut stream, &mut buf) {
            Ok(None) => continue,
            Ok(Some(req)) => {
                if handle_request(&mut stream, &req, &mut session)? {
                    // PLAY 后读超时拉长，便于重放间隙仍可收到 TEARDOWN
                    stream.set_read_timeout(Some(Duration::from_millis(20)))?;
                    return Ok(stream);
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::Interrupted,
        "RTSP 握手未完成已停止",
    ))
}

/// 非阻塞尝试读一条客户端信令（PLAY 后）；`TEARDOWN`/断开返回错误。
pub fn poll_client_signal(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<()> {
    let mut session = String::from("1");
    match read_request(stream, buf) {
        Ok(None) => Ok(()),
        Ok(Some(req)) => {
            let _ = handle_request(stream, &req, &mut session)?;
            Ok(())
        }
        Err(e)
            if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}
