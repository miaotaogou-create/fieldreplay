//! 拉起 / 等待 / 回收引擎 `serve --ui-dir` 子进程。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// 持有引擎子进程；Drop / 关窗时 kill。Windows 上挂到 Job，壳异常退出时一并带走。
pub struct EngineChild {
    child: Child,
    #[cfg(windows)]
    _job: WinJob,
}

impl EngineChild {
    pub fn spawn(engine: &Path, ui_dir: &Path, bind: &str) -> io::Result<Self> {
        let mut cmd = Command::new(engine);
        cmd.arg("serve")
            .arg("--bind")
            .arg(bind)
            .arg("--ui-dir")
            .arg(ui_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("无法启动引擎 {}: {e}", engine.display()),
            )
        })?;

        #[cfg(windows)]
        let job = {
            let job = WinJob::new_kill_on_close()?;
            job.assign_pid(child.id())?;
            job
        };

        // 立刻失败时把 stderr 读出来，方便排查端口占用等
        thread::sleep(Duration::from_millis(50));
        if let Some(status) = child.try_wait()? {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "引擎立刻退出 code={}。{}",
                    status.code().unwrap_or(-1),
                    err.trim()
                ),
            ));
        }

        Ok(Self {
            child,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn try_reap(&mut self) -> io::Result<Option<i32>> {
        match self.child.try_wait()? {
            Some(status) => Ok(Some(status.code().unwrap_or(-1))),
            None => Ok(None),
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for EngineChild {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 等到控制面端口可连，并确认 HTTP GET / 返回 2xx。
pub fn wait_http_ready(bind: &str, timeout: Duration) -> io::Result<()> {
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("非法 --bind: {e}")))?;
    let deadline = Instant::now() + timeout;
    let mut last_err = None;

    while Instant::now() < deadline {
        match probe_http_root(&addr) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
        thread::sleep(Duration::from_millis(80));
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "等待引擎就绪超时（{bind}）。最后错误: {}",
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "无".into())
        ),
    ))
}

fn probe_http_root(addr: &SocketAddr) -> io::Result<()> {
    let mut stream = TcpStream::connect_timeout(addr, Duration::from_millis(300))?;
    stream.set_read_timeout(Some(Duration::from_millis(800)))?;
    stream.set_write_timeout(Some(Duration::from_millis(800)))?;
    let req = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        addr
    );
    stream.write_all(req.as_bytes())?;
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "空响应"));
    }
    let head = String::from_utf8_lossy(&buf[..n]);
    let ok = head
        .lines()
        .next()
        .map(|l| l.contains(" 2"))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("非成功 HTTP 状态: {}", head.lines().next().unwrap_or("")),
        ))
    }
}

/// 冒烟：确认静态 UI 已挂上（非「UI 未挂载」提示页）。
pub fn smoke_fetch_ui(bind: &str) -> io::Result<()> {
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("非法 --bind: {e}")))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let req = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        addr
    );
    stream.write_all(req.as_bytes())?;
    let mut body = Vec::new();
    stream.read_to_end(&mut body)?;
    let text = String::from_utf8_lossy(&body);
    if text.contains("UI 未挂载") {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "引擎已响应但未挂载 UI（检查 --ui-dir）",
        ));
    }
    if !text.contains("HTTP/1.") {
        return Err(io::Error::new(io::ErrorKind::Other, "未收到 HTTP 响应"));
    }
    let status_ok = text
        .lines()
        .next()
        .map(|l| l.contains(" 2"))
        .unwrap_or(false);
    if !status_ok {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "HTTP 状态异常: {}",
                text.lines().next().unwrap_or("(空)")
            ),
        ));
    }
    Ok(())
}

#[cfg(windows)]
struct WinJob {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe impl Send for WinJob {}

#[cfg(windows)]
impl WinJob {
    fn new_kill_on_close() -> io::Result<Self> {
        use std::mem::{size_of, zeroed};
        use std::ptr;

        #[repr(C)]
        struct Joli {
            per_process_user_time_limit: u64,
            per_job_user_time_limit: u64,
            limit_flags: u32,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
            active_process_limit: u32,
            affinity: usize,
            priority_class: u32,
            scheduling_class: u32,
        }

        extern "system" {
            fn CreateJobObjectW(
                lp_job_attributes: *mut std::ffi::c_void,
                lp_name: *const u16,
            ) -> *mut std::ffi::c_void;
            fn SetInformationJobObject(
                h_job: *mut std::ffi::c_void,
                job_object_info_class: i32,
                lp_job_object_info: *mut std::ffi::c_void,
                cb_job_object_info_length: u32,
            ) -> i32;
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
        }

        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;

        #[repr(C)]
        struct Jeeli {
            basic_limit_information: Joli,
            io_info: [u64; 6],
            process_memory_limit: usize,
            job_memory_limit: usize,
            peak_process_memory_used: usize,
            peak_job_memory_used: usize,
        }

        unsafe {
            let handle = CreateJobObjectW(ptr::null_mut(), ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: Jeeli = zeroed();
            info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                &mut info as *mut _ as *mut _,
                size_of::<Jeeli>() as u32,
            );
            if ok == 0 {
                let e = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(e);
            }
            Ok(Self { handle })
        }
    }

    fn assign_pid(&self, pid: u32) -> io::Result<()> {
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn AssignProcessToJobObject(
                h_job: *mut std::ffi::c_void,
                h_process: *mut std::ffi::c_void,
            ) -> i32;
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
        }

        const PROCESS_SET_QUOTA: u32 = 0x0100;
        const PROCESS_TERMINATE: u32 = 0x0001;
        const PROCESS_ALL: u32 = PROCESS_SET_QUOTA | PROCESS_TERMINATE;

        unsafe {
            let proc = OpenProcess(PROCESS_ALL, 0, pid);
            if proc.is_null() {
                return Err(io::Error::last_os_error());
            }
            let ok = AssignProcessToJobObject(self.handle, proc);
            let err = if ok == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            CloseHandle(proc);
            match err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
    }
}

#[cfg(windows)]
impl Drop for WinJob {
    fn drop(&mut self) {
        extern "system" {
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
        }
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
