//! FieldReplay 薄壳：拉起引擎 `serve --ui-dir`，本机 WebView2 加载 UI。
//!
//! 用法：
//!   fieldreplay
//!   fieldreplay --smoke
//!   fieldreplay --engine <path> --ui-dir <ui/dist> --bind 127.0.0.1:17890

// 双击启动不弹控制台；从 cmd/bat 拉起时 stderr 仍可进父控制台（打包 smoke）
#![windows_subsystem = "windows"]

mod child;
mod paths;
mod window;

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use child::{smoke_fetch_ui, wait_http_ready, EngineChild};
use paths::{resolve_engine, resolve_ui_dir};

const DEFAULT_BIND: &str = "127.0.0.1:17890";

struct Opts {
    engine: Option<PathBuf>,
    ui_dir: Option<PathBuf>,
    bind: String,
    smoke: bool,
    help: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let msg = format!("fieldreplay: {e}");
            eprintln!("{msg}");
            show_fatal(&msg);
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let opts = parse_args(env::args().skip(1))?;
    if opts.help {
        // 无控制台时帮助不可见；仍保留给命令行调用
        print_help();
        return Ok(());
    }

    let engine = resolve_engine(opts.engine)?;
    let ui_dir = resolve_ui_dir(opts.ui_dir)?;
    let bind = opts.bind;
    let url = format!("http://{bind}/");

    if opts.smoke {
        eprintln!("引擎: {}", engine.display());
        eprintln!("UI  : {}", ui_dir.display());
        eprintln!("地址: {url}");
    }

    let mut child = EngineChild::spawn(&engine, &ui_dir, &bind)?;
    if let Some(code) = child.try_reap()? {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("引擎立刻退出，code={code}"),
        ));
    }

    wait_http_ready(&bind, Duration::from_secs(15))?;

    if opts.smoke {
        smoke_fetch_ui(&bind)?;
        eprintln!("smoke: OK（引擎托管 UI 可访问；未开窗口）");
        child.shutdown();
        return Ok(());
    }

    // EventLoop::run 不返回；关窗回调内 shutdown 引擎
    window::run(&url, child)
}

/// 双击启动失败时给可见提示（无控制台窗口）。
fn show_fatal(msg: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(msg)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let title: Vec<u16> = std::ffi::OsStr::new("FieldReplay")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        extern "system" {
            fn MessageBoxW(
                hwnd: *mut std::ffi::c_void,
                text: *const u16,
                caption: *const u16,
                utype: u32,
            ) -> i32;
        }
        const MB_OK: u32 = 0;
        const MB_ICONERROR: u32 = 0x10;
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = msg;
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> io::Result<Opts> {
    let mut opts = Opts {
        engine: None,
        ui_dir: None,
        bind: DEFAULT_BIND.to_string(),
        smoke: false,
        help: false,
    };
    let mut it = args.peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => opts.help = true,
            "--smoke" => opts.smoke = true,
            "--engine" => {
                let v = it
                    .next()
                    .ok_or_else(|| err_arg("--engine 需要路径"))?;
                opts.engine = Some(PathBuf::from(v));
            }
            "--ui-dir" => {
                let v = it
                    .next()
                    .ok_or_else(|| err_arg("--ui-dir 需要目录"))?;
                opts.ui_dir = Some(PathBuf::from(v));
            }
            "--bind" => {
                let v = it.next().ok_or_else(|| err_arg("--bind 需要地址"))?;
                opts.bind = v;
            }
            other => return Err(err_arg(format!("未知参数: {other}"))),
        }
    }
    Ok(opts)
}

fn err_arg(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

fn print_help() {
    println!(
        "\
FieldReplay 薄壳（WebView2）

用法:
  fieldreplay [--engine <引擎EXE>] [--ui-dir <ui/dist>] [--bind 127.0.0.1:17890]
  fieldreplay --smoke   # 拉起引擎并 HTTP 探活，不开窗口（自检）

环境变量:
  FIELDREPLAY_ENGINE   引擎可执行文件
  FIELDREPLAY_UI_DIR   静态 UI 目录（npm run build 产物）

说明:
  壳只负责窗口；录放热路径仍在 fieldreplay-engine。
  正式打包布局见进度本 E6c。"
    );
}
