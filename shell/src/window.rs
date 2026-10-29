//! 薄壳 WebView2 窗口：只加载本机引擎托管的 UI。

use std::io;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

use crate::child::EngineChild;

/// 打开本机窗口并阻塞至用户关闭。关窗时先停引擎（`EventLoop::run` 不返回）。
pub fn run(url: &str, engine: EngineChild) -> io::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("FieldReplay")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("创建窗口失败: {e}")))?;

    let _webview = WebViewBuilder::new()
        .with_url(url)
        .build(&window)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("创建 WebView2 失败: {e}")))?;

    let mut engine = Some(engine);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            if let Some(mut child) = engine.take() {
                child.shutdown();
            }
            *control_flow = ControlFlow::Exit;
        }
    });
}
