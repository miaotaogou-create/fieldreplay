//! UDP 通道：bind 收包 / 向目标发包。

use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// 录制侧默认接收缓冲（降低高速率突发时内核丢包）。
const RECEIVER_RCVBUF: usize = 4 * 1024 * 1024;

/// 在 `bind` 上监听；`read_timeout` 便于录制循环可被打断。
pub fn bind_receiver(bind: impl ToSocketAddrs, read_timeout: Option<Duration>) -> io::Result<UdpSocket> {
    let sock = UdpSocket::bind(bind)?;
    sock.set_read_timeout(read_timeout)?;
    // 失败不阻断：系统可能钳制实际大小
    let _ = set_recv_buffer(&sock, RECEIVER_RCVBUF);
    Ok(sock)
}

#[cfg(windows)]
fn set_recv_buffer(sock: &UdpSocket, bytes: usize) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    // winsock：SOL_SOCKET / SO_RCVBUF
    const SOL_SOCKET: i32 = 0xffff;
    const SO_RCVBUF: i32 = 0x1002;
    extern "system" {
        fn setsockopt(
            s: usize,
            level: i32,
            optname: i32,
            optval: *const u8,
            optlen: i32,
        ) -> i32;
    }
    let val = (bytes as u32).to_ne_bytes();
    let rc = unsafe {
        setsockopt(
            sock.as_raw_socket() as usize,
            SOL_SOCKET,
            SO_RCVBUF,
            val.as_ptr(),
            val.len() as i32,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_recv_buffer(_sock: &UdpSocket, _bytes: usize) -> io::Result<()> {
    // 非 Windows 暂不调缓冲；本项目交付以 Win 为主
    Ok(())
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
