//! Windows：把 `assets/fieldreplay.ico` 嵌入壳 EXE（资源管理器文件图标）。

fn main() {
    println!("cargo:rerun-if-changed=assets/fieldreplay.ico");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/fieldreplay.ico");
    // 资源 ID 1；运行时 `Icon::from_resource(1, …)` 与任务栏共用
    res.compile()
        .unwrap_or_else(|e| panic!("嵌入应用图标失败: {e}"));
}
