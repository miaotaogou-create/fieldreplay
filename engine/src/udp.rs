//! UDP 通道：bind 收包 / 向目标发包。

use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// 在 `bind` 上监听；`read_timeout` 便于录制循环可被打断。
pub fn bind_receiver(bind: impl ToSocketAddrs, read_timeout: Option<Duration>) -> io::Result<UdpSocket> {
    let sock = UdpSocket::bind(bind)?;
    sock.set_read_timeout(read_timeout)?;
    Ok(sock)
}

pub fn bind_sender(local: impl ToSocketAddrs) -> io::Result<UdpSocket> {
    let sock = UdpSocket::bind(local)?;
    Ok(sock)
}

pub fn resolve_one(addr: &str) -> io::Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("无法解析地址: {addr}")))
}

/// 收一包；超时返回 `Ok(None)`。
pub fn recv_one(sock: &UdpSocket, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
    match sock.recv_from(buf) {
        Ok((n, peer)) => Ok(Some((n, peer))),
        Err(e) if is_timeout(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn send_to(sock: &UdpSocket, payload: &[u8], target: SocketAddr) -> io::Result<usize> {
    sock.send_to(payload, target)
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

pub fn peer_aux(peer: SocketAddr) -> Vec<u8> {
    peer.to_string().into_bytes()
}
