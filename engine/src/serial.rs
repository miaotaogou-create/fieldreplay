//! 串口通道：打开实口收发（Windows Win32）；自检用内存环回。

use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 可选实口/虚口对自检：`FIELDREPLAY_SERIAL_PAIR=COMa,COMb`（未设置则跳过，不导致 CI 失败）。
pub const SERIAL_PAIR_ENV: &str = "FIELDREPLAY_SERIAL_PAIR";

/// 解析 `COMa,COMb`；两端须非空且不同。
pub fn parse_serial_pair(spec: &str) -> io::Result<(String, String)> {
    let parts: Vec<&str> = spec
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{SERIAL_PAIR_ENV} 应为 COMa,COMb（两个不同口名），当前: {spec:?}"
            ),
        ));
    }
    if parts[0].eq_ignore_ascii_case(parts[1]) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{SERIAL_PAIR_ENV} 两端口名不能相同: {}", parts[0]),
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// 读取环境变量；未设置或空串 → `Ok(None)`（自检跳过实口对）。
pub fn serial_pair_from_env() -> io::Result<Option<(String, String)>> {
    match std::env::var(SERIAL_PAIR_ENV) {
        Ok(s) if !s.trim().is_empty() => parse_serial_pair(&s).map(Some),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{SERIAL_PAIR_ENV} 含非 Unicode"),
        )),
    }
}

/// 校验位（写入 `.fcap` config_json 的 `parity`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl Parity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Odd => "odd",
            Self::Even => "even",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "n" => Some(Self::None),
            "odd" | "o" => Some(Self::Odd),
            "even" | "e" => Some(Self::Even),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub port_name: String,
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: Parity,
}

impl SerialConfig {
    pub fn new(port_name: impl Into<String>, baud: u32) -> Self {
        Self {
            port_name: port_name.into(),
            baud,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
        }
    }

    /// 与 `docs/design/fcap-format.md` 串口 `config_json` 对齐。
    pub fn to_config_json(&self) -> String {
        format!(
            r#"{{"port_name":"{}","baud":{},"data_bits":{},"stop_bits":{},"parity":"{}"}}"#,
            escape_json_str(&self.port_name),
            self.baud,
            self.data_bits,
            self.stop_bits,
            self.parity.as_str()
        )
    }
}

enum PortInner {
    #[cfg(windows)]
    Win(win::WinPort),
    /// 自检环回：非真实 COM
    Mem(MemEnd),
}

/// 串口句柄（实口或自检环回端）。
pub struct SerialPort {
    inner: PortInner,
}

impl SerialPort {
    /// 打开本机串口（Windows）。`port_name` 如 `COM3` / `COM10`。
    pub fn open(cfg: &SerialConfig, read_timeout: Duration) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let win = win::WinPort::open(cfg, read_timeout)?;
            return Ok(Self {
                inner: PortInner::Win(win),
            });
        }
        #[cfg(not(windows))]
        {
            let _ = (cfg, read_timeout);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "串口实口目前仅支持 Windows",
            ))
        }
    }

    /// 自检用交叉环回：写入 A 可从 B 读，反之亦然。
    pub fn open_loopback_pair(read_timeout: Duration) -> (Self, Self) {
        let (tx_ab, rx_ab) = mpsc::channel::<Vec<u8>>();
        let (tx_ba, rx_ba) = mpsc::channel::<Vec<u8>>();
        let a = Self {
            inner: PortInner::Mem(MemEnd {
                tx: tx_ab,
                rx: Arc::new(Mutex::new(rx_ba)),
                pending: Mutex::new(Vec::new()),
                read_timeout,
            }),
        };
        let b = Self {
            inner: PortInner::Mem(MemEnd {
                tx: tx_ba,
                rx: Arc::new(Mutex::new(rx_ab)),
                pending: Mutex::new(Vec::new()),
                read_timeout,
            }),
        };
        (a, b)
    }

    /// 读若干字节；超时返回 `Ok(None)`。
    pub fn read(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        match &self.inner {
            #[cfg(windows)]
            PortInner::Win(p) => p.read(buf),
            PortInner::Mem(p) => p.read(buf),
        }
    }

    pub fn write_all(&self, data: &[u8]) -> io::Result<()> {
        match &self.inner {
            #[cfg(windows)]
            PortInner::Win(p) => p.write_all(data),
            PortInner::Mem(p) => p.write_all(data),
        }
    }
}

/// 枚举本机串口名（如 `COM3`）。Windows 用 `QueryDosDeviceW`；其它平台返回空列表。
/// 无口时返回空 `Vec`（不视为错误）。
pub fn list_ports() -> io::Result<Vec<String>> {
    #[cfg(windows)]
    {
        win::list_ports()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

struct MemEnd {
    tx: mpsc::Sender<Vec<u8>>,
    rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    pending: Mutex<Vec<u8>>,
    read_timeout: Duration,
}

impl MemEnd {
    fn read(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        if buf.is_empty() {
            return Ok(Some(0));
        }
        {
            let mut pend = self.pending.lock().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "串口环回锁损坏")
            })?;
            if !pend.is_empty() {
                let n = buf.len().min(pend.len());
                buf[..n].copy_from_slice(&pend[..n]);
                pend.drain(..n);
                return Ok(Some(n));
            }
        }
        let rx = self.rx.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "串口环回锁损坏")
        })?;
        match rx.recv_timeout(self.read_timeout) {
            Ok(chunk) => {
                let n = buf.len().min(chunk.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                if chunk.len() > n {
                    let mut pend = self.pending.lock().map_err(|_| {
                        io::Error::new(io::ErrorKind::Other, "串口环回锁损坏")
                    })?;
                    pend.extend_from_slice(&chunk[n..]);
                }
                Ok(Some(n))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "串口环回对端已关闭",
            )),
        }
    }

    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        self.tx.send(data.to_vec()).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "串口环回对端已关闭")
        })
    }
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(windows)]
mod win {
    use super::{Parity, SerialConfig};
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{FromRawHandle, OwnedHandle, AsRawHandle};
    use std::ptr;
    use std::time::Duration;

    type HANDLE = *mut std::ffi::c_void;
    type BOOL = i32;
    type DWORD = u32;
    type WCHAR = u16;

    const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
    const GENERIC_READ: DWORD = 0x8000_0000;
    const GENERIC_WRITE: DWORD = 0x4000_0000;
    const OPEN_EXISTING: DWORD = 3;
    const FILE_ATTRIBUTE_NORMAL: DWORD = 0x80;
    const NOPARITY: u8 = 0;
    const ODDPARITY: u8 = 1;
    const EVENPARITY: u8 = 2;
    const ONESTOPBIT: u8 = 0;
    const TWOSTOPBITS: u8 = 2;

    #[repr(C)]
    struct Dcb {
        dc_length: DWORD,
        baud_rate: DWORD,
        flags: DWORD,
        w_reserved: u16,
        xon_lim: u16,
        xoff_lim: u16,
        byte_size: u8,
        parity: u8,
        stop_bits: u8,
        xon_char: i8,
        xoff_char: i8,
        error_char: i8,
        eof_char: i8,
        evt_char: i8,
        w_reserved1: u16,
    }

    #[repr(C)]
    struct CommTimeouts {
        read_interval_timeout: DWORD,
        read_total_timeout_multiplier: DWORD,
        read_total_timeout_constant: DWORD,
        write_total_timeout_multiplier: DWORD,
        write_total_timeout_constant: DWORD,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const WCHAR,
            access: DWORD,
            share: DWORD,
            sec: *mut std::ffi::c_void,
            disposition: DWORD,
            flags: DWORD,
            template: HANDLE,
        ) -> HANDLE;
        fn ReadFile(
            h: HANDLE,
            buf: *mut u8,
            to_read: DWORD,
            read: *mut DWORD,
            overlapped: *mut std::ffi::c_void,
        ) -> BOOL;
        fn WriteFile(
            h: HANDLE,
            buf: *const u8,
            to_write: DWORD,
            written: *mut DWORD,
            overlapped: *mut std::ffi::c_void,
        ) -> BOOL;
        fn GetCommState(h: HANDLE, dcb: *mut Dcb) -> BOOL;
        fn SetCommState(h: HANDLE, dcb: *const Dcb) -> BOOL;
        fn SetCommTimeouts(h: HANDLE, t: *const CommTimeouts) -> BOOL;
        fn GetLastError() -> DWORD;
        fn PurgeComm(h: HANDLE, flags: DWORD) -> BOOL;
        fn QueryDosDeviceW(
            name: *const WCHAR,
            target: *mut WCHAR,
            size: DWORD,
        ) -> DWORD;
    }

    const PURGE_RXCLEAR: DWORD = 0x0008;
    const PURGE_TXCLEAR: DWORD = 0x0004;
    const ERROR_INSUFFICIENT_BUFFER: DWORD = 122;

    pub struct WinPort {
        handle: OwnedHandle,
    }

    // OwnedHandle 关闭即 CloseHandle；勿再手动关。
    impl WinPort {
        pub fn open(cfg: &SerialConfig, read_timeout: Duration) -> io::Result<Self> {
            let path = device_path(&cfg.port_name);
            let wide: Vec<u16> = path
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let raw = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                )
            };
            if raw == INVALID_HANDLE_VALUE || raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(raw as _) };

            unsafe {
                let h = handle.as_raw_handle() as HANDLE;
                let mut dcb: Dcb = zeroed();
                dcb.dc_length = size_of::<Dcb>() as DWORD;
                if GetCommState(h, &mut dcb) == 0 {
                    return Err(io::Error::last_os_error());
                }
                dcb.baud_rate = cfg.baud;
                dcb.byte_size = cfg.data_bits;
                dcb.parity = match cfg.parity {
                    Parity::None => NOPARITY,
                    Parity::Odd => ODDPARITY,
                    Parity::Even => EVENPARITY,
                };
                dcb.stop_bits = match cfg.stop_bits {
                    2 => TWOSTOPBITS,
                    _ => ONESTOPBIT,
                };
                // fBinary | (fParity if needed)
                dcb.flags = 0x0000_0001; // fBinary
                if cfg.parity != Parity::None {
                    dcb.flags |= 0x0000_0002; // fParity
                }
                if SetCommState(h, &dcb) == 0 {
                    return Err(io::Error::last_os_error());
                }

                let ms = read_timeout.as_millis().min(u32::MAX as u128) as DWORD;
                let timeouts = CommTimeouts {
                    // 有数据则尽快返回；无数据则等到 constant
                    read_interval_timeout: 10,
                    read_total_timeout_multiplier: 0,
                    read_total_timeout_constant: ms.max(1),
                    write_total_timeout_multiplier: 0,
                    write_total_timeout_constant: ms.max(1).saturating_mul(5),
                };
                if SetCommTimeouts(h, &timeouts) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let _ = PurgeComm(h, PURGE_RXCLEAR | PURGE_TXCLEAR);
            }

            Ok(Self { handle })
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
            if buf.is_empty() {
                return Ok(Some(0));
            }
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    self.handle.as_raw_handle() as HANDLE,
                    buf.as_mut_ptr(),
                    buf.len() as DWORD,
                    &mut read,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                // WAIT_TIMEOUT 等；多数情况下读超时返回成功且 n=0
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            if read == 0 {
                Ok(None)
            } else {
                Ok(Some(read as usize))
            }
        }

        pub fn write_all(&self, data: &[u8]) -> io::Result<()> {
            let mut offset = 0;
            while offset < data.len() {
                let mut written = 0u32;
                let ok = unsafe {
                    WriteFile(
                        self.handle.as_raw_handle() as HANDLE,
                        data[offset..].as_ptr(),
                        (data.len() - offset) as DWORD,
                        &mut written,
                        ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                if written == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "串口写入返回 0 字节",
                    ));
                }
                offset += written as usize;
            }
            Ok(())
        }
    }

    fn device_path(name: &str) -> String {
        let n = name.trim();
        if n.starts_with(r"\\.\") || n.starts_with("//./") {
            n.to_string()
        } else {
            format!(r"\\.\{n}")
        }
    }

    /// 枚举 DOS 设备名中的 `COMn`（含 COM10+），按编号排序。
    pub fn list_ports() -> io::Result<Vec<String>> {
        // 缓冲不够时按 ERROR_INSUFFICIENT_BUFFER 扩容重试
        let mut cap: usize = 16 * 1024;
        let mut buf: Vec<u16> = vec![0; cap];
        loop {
            let n = unsafe { QueryDosDeviceW(ptr::null(), buf.as_mut_ptr(), cap as DWORD) };
            if n != 0 {
                buf.truncate(n as usize);
                break;
            }
            let err = unsafe { GetLastError() };
            if err != ERROR_INSUFFICIENT_BUFFER {
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            cap = cap.saturating_mul(2).max(cap + 1);
            if cap > 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "QueryDosDeviceW 缓冲过大",
                ));
            }
            buf.resize(cap, 0);
        }

        let mut ports = Vec::new();
        let mut start = 0usize;
        while start < buf.len() {
            if let Some(rel) = buf[start..].iter().position(|&c| c == 0) {
                if rel == 0 {
                    break; // 双 NUL 结束
                }
                let name = String::from_utf16_lossy(&buf[start..start + rel]);
                if is_com_name(&name) {
                    ports.push(name);
                }
                start += rel + 1;
            } else {
                break;
            }
        }
        ports.sort_by_key(|p| com_sort_key(p));
        ports.dedup();
        Ok(ports)
    }

    fn is_com_name(name: &str) -> bool {
        let u = name.to_ascii_uppercase();
        let rest = match u.strip_prefix("COM") {
            Some(r) => r,
            None => return false,
        };
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    }

    fn com_sort_key(name: &str) -> u32 {
        name.to_ascii_uppercase()
            .strip_prefix("COM")
            .and_then(|n| n.parse().ok())
            .unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_json_shape() {
        let c = SerialConfig::new("COM3", 115_200);
        assert_eq!(
            c.to_config_json(),
            r#"{"port_name":"COM3","baud":115200,"data_bits":8,"stop_bits":1,"parity":"none"}"#
        );
    }

    #[test]
    fn mem_loopback_bytes() {
        let (a, b) = SerialPort::open_loopback_pair(Duration::from_millis(200));
        a.write_all(b"hello").unwrap();
        let mut buf = [0u8; 16];
        let n = b.read(&mut buf).unwrap().expect("应读到数据");
        assert_eq!(&buf[..n], b"hello");
    }

    #[test]
    fn list_ports_does_not_fail() {
        let ports = list_ports().expect("枚举串口不应失败");
        for p in &ports {
            assert!(
                p.to_ascii_uppercase().starts_with("COM"),
                "意外端口名: {p}"
            );
        }
    }

    #[test]
    fn parse_serial_pair_ok() {
        let (a, b) = parse_serial_pair(" COM3 , COM4 ").unwrap();
        assert_eq!(a, "COM3");
        assert_eq!(b, "COM4");
    }

    #[test]
    fn parse_serial_pair_rejects_same_or_bad() {
        assert!(parse_serial_pair("COM3").is_err());
        assert!(parse_serial_pair("COM3,COM3").is_err());
        assert!(parse_serial_pair("COM3, COM3 ").is_err());
        assert!(parse_serial_pair("").is_err());
    }
}
