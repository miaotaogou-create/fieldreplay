//! FieldReplay 自解压启动器：展开旁路后拉起壳。
//!
//! 用法：
//!   FieldReplay.exe           — 解压（或复用）并启动
//!   FieldReplay.exe --self-check — 校验载荷并可解压到临时目录（打包脚本用）

#![windows_subsystem = "windows"]

mod extract;
mod launch;
mod msg;
mod payload;

use std::env;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let self_check = env::args().skip(1).any(|a| a == "--self-check");
    match run(self_check) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let text = format!("FieldReplay 自解压启动失败：{e}");
            eprintln!("{text}");
            if !self_check {
                msg::show_fatal(&text);
            }
            ExitCode::FAILURE
        }
    }
}

fn run(self_check: bool) -> io::Result<()> {
    let exe = env::current_exe()?;
    let payload = payload::read_from_exe(&exe)?;

    if self_check {
        return run_self_check(&payload);
    }

    let dir = extract::resolve_extract_dir(&exe)?;
    let plan = extract::prepare(&dir, &payload.version, &payload.zip_bytes)?;
    if plan.skipped {
        eprintln!("自解压：版本一致，跳过解压 → {}", plan.dir.display());
    } else {
        eprintln!("自解压：已展开 → {}", plan.dir.display());
    }
    launch::start_shell(&plan.dir)
}

fn run_self_check(payload: &payload::Payload) -> io::Result<()> {
    let tmp = env::temp_dir().join(format!(
        "fieldreplay-sfx-check-{}",
        std::process::id()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    let plan = extract::prepare(&tmp, &payload.version, &payload.zip_bytes)?;
    let shell = plan.dir.join("fieldreplay.exe");
    let engine = plan.dir.join("fieldreplay-engine.exe");
    let index = plan.dir.join("ui").join("dist").join("index.html");
    for p in [&shell, &engine, &index] {
        if !p.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("self-check 缺少: {}", p.display()),
            ));
        }
    }
    eprintln!(
        "sfx self-check: OK（version={}, extract={}）",
        payload.version,
        plan.dir.display()
    );
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}
