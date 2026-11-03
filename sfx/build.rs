//! Windows：嵌入与壳相同的产品图标（资源管理器可辨）。

fn main() {
    println!("cargo:rerun-if-changed=../shell/assets/fieldreplay.ico");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon("../shell/assets/fieldreplay.ico");
    res.compile()
        .unwrap_or_else(|e| panic!("嵌入应用图标失败: {e}"));
}
