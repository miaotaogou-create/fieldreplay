//! 薄壳 WebView2 窗口：无系统边框，自定义标题栏经 IPC 控制。

use std::io;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon, WindowBuilder};
use wry::http::Request;
use wry::WebViewBuilder;

use crate::child::EngineChild;

enum UserEvent {
    Minimize,
    Maximize,
    DragWindow,
    CloseWindow,
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

/// 打开本机无边框窗口并阻塞至用户关闭。关窗时先停引擎。
pub fn run(url: &str, engine: EngineChild) -> io::Result<()> {
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
        match req.body().as_str() {
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
            _ => {}
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
            _ => {}
        }
    });
}
