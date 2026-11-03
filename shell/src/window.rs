//! 薄壳 WebView2 窗口：无系统边框，自定义标题栏经 IPC 控制。

use std::io;
use std::path::PathBuf;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon, WindowBuilder};
use wry::http::Request;
use wry::WebViewBuilder;

use crate::export_io::write_b64;
use crate::record_paths::{
    default_record_path, js_escape, path_to_string, pick_open_fcap, pick_save_fcap,
    suggested_record_file_name,
};

enum UserEvent {
    Minimize,
    Maximize,
    DragWindow,
    CloseWindow,
    /// UI 请求默认录制路径、另存为/打开或写入文件；经 evaluate_script 回传。
    ShellRpc {
        id: String,
        kind: ShellRpcKind,
    },
}

enum ShellRpcKind {
    DefaultRecordPath,
    PickSaveFcap { suggested: Option<String> },
    /// 原生打开 `.fcap`（U12）。
    PickOpenFcap,
    /// 将 base64 载荷写入绝对路径（U11）。
    WriteFile { path: String, b64: String },
}

/// 从 EXE 嵌入资源加载窗口图标；缺失时回退系统默认（不阻断开窗）。
fn load_window_icon() -> Option<Icon> {
    #[cfg(windows)]
    {
        use tao::dpi::PhysicalSize;
        use tao::platform::windows::IconExtWindows;
        // build.rs / winres 嵌入 ID=1；32px 适合常见任务栏缩放
        Icon::from_resource(1, Some(PhysicalSize::new(32, 32)))
            .or_else(|_| Icon::from_resource(1, None))
            .ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn parse_shell_rpc(body: &str) -> Option<UserEvent> {
    // 约定：default_record_path|<id>
    //       pick_save_fcap|<id>|<可选建议文件名>
    //       pick_open_fcap|<id>
    //       write_file|<id>|<绝对路径>|<base64>
    let mut parts = body.splitn(4, '|');
    let cmd = parts.next()?;
    let id = parts.next()?.to_string();
    if id.is_empty() {
        return None;
    }
    match cmd {
        "default_record_path" => Some(UserEvent::ShellRpc {
            id,
            kind: ShellRpcKind::DefaultRecordPath,
        }),
        "pick_save_fcap" => {
            let suggested = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Some(UserEvent::ShellRpc {
                id,
                kind: ShellRpcKind::PickSaveFcap { suggested },
            })
        }
        "pick_open_fcap" => Some(UserEvent::ShellRpc {
            id,
            kind: ShellRpcKind::PickOpenFcap,
        }),
        "write_file" => {
            let path = parts.next()?.to_string();
            let b64 = parts.next().unwrap_or("").to_string();
            if path.is_empty() {
                return None;
            }
            Some(UserEvent::ShellRpc {
                id,
                kind: ShellRpcKind::WriteFile { path, b64 },
            })
        }
        _ => None,
    }
}

fn resolve_script(id: &str, path: &str) -> String {
    format!(
        "window.__frShellResolve && window.__frShellResolve(\"{}\",\"{}\");",
        js_escape(id),
        js_escape(path)
    )
}

/// 打开本机无边框窗口并阻塞至用户关闭。关窗时先停引擎。
pub fn run(url: &str, engine: crate::child::EngineChild) -> io::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let mut builder = WindowBuilder::new()
        .with_title("FieldReplay")
        .with_decorations(false)
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0));
    if let Some(icon) = load_window_icon() {
        builder = builder.with_window_icon(Some(icon));
    }
    let window = builder
        .build(&event_loop)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("创建窗口失败: {e}")))?;

    let proxy = event_loop.create_proxy();
    let ipc = move |req: Request<String>| {
        let body = req.body().as_str();
        match body {
            "minimize" => {
                let _ = proxy.send_event(UserEvent::Minimize);
            }
            "maximize" => {
                let _ = proxy.send_event(UserEvent::Maximize);
            }
            "drag_window" => {
                let _ = proxy.send_event(UserEvent::DragWindow);
            }
            "close" => {
                let _ = proxy.send_event(UserEvent::CloseWindow);
            }
            other => {
                if let Some(ev) = parse_shell_rpc(other) {
                    let _ = proxy.send_event(ev);
                }
            }
        }
    };

    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_ipc_handler(ipc)
        .build(&window)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("创建 WebView2 失败: {e}")))?;

    let mut webview = Some(webview);
    let mut engine = Some(engine);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::CloseWindow) => {
                let _ = webview.take();
                if let Some(mut child) = engine.take() {
                    child.shutdown();
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Minimize) => {
                window.set_minimized(true);
            }
            Event::UserEvent(UserEvent::Maximize) => {
                window.set_maximized(!window.is_maximized());
            }
            Event::UserEvent(UserEvent::DragWindow) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(UserEvent::ShellRpc { id, kind }) => {
                let path = match kind {
                    ShellRpcKind::DefaultRecordPath => match default_record_path() {
                        Ok(p) => path_to_string(&p),
                        Err(_) => String::new(),
                    },
                    ShellRpcKind::PickSaveFcap { suggested } => {
                        let name = suggested.or_else(|| Some(suggested_record_file_name()));
                        match pick_save_fcap(name.as_deref()) {
                            Ok(Some(p)) => path_to_string(&p),
                            _ => String::new(),
                        }
                    }
                    ShellRpcKind::PickOpenFcap => match pick_open_fcap() {
                        Ok(Some(p)) => path_to_string(&p),
                        _ => String::new(),
                    },
                    ShellRpcKind::WriteFile {
                        path: dest,
                        b64: payload,
                    } => {
                        let p = PathBuf::from(&dest);
                        match write_b64(&p, &payload) {
                            Ok(()) => path_to_string(&p),
                            Err(_) => String::new(),
                        }
                    }
                };
                if let Some(wv) = webview.as_ref() {
                    let _ = wv.evaluate_script(&resolve_script(&id, &path));
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_write_file_rpc() {
        let ev = parse_shell_rpc("write_file|r1|C:\\tmp\\a.fcap|RkNBUA==").expect("parse");
        match ev {
            UserEvent::ShellRpc {
                id,
                kind: ShellRpcKind::WriteFile { path, b64 },
            } => {
                assert_eq!(id, "r1");
                assert_eq!(path, "C:\\tmp\\a.fcap");
                assert_eq!(b64, "RkNBUA==");
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn parse_pick_save_still_works() {
        let ev = parse_shell_rpc("pick_save_fcap|r2|demo.fcap").expect("parse");
        match ev {
            UserEvent::ShellRpc {
                id,
                kind: ShellRpcKind::PickSaveFcap { suggested },
            } => {
                assert_eq!(id, "r2");
                assert_eq!(suggested.as_deref(), Some("demo.fcap"));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn parse_pick_open_fcap_rpc() {
        let ev = parse_shell_rpc("pick_open_fcap|r3").expect("parse");
        match ev {
            UserEvent::ShellRpc {
                id,
                kind: ShellRpcKind::PickOpenFcap,
            } => {
                assert_eq!(id, "r3");
            }
            _ => panic!("unexpected"),
        }
    }
}
