//! 解析引擎可执行文件与 UI 静态目录路径。

use std::env;
use std::io;
use std::path::{Path, PathBuf};

/// 壳自身所在目录（开发期通常为 `shell/target/debug`）。
pub fn exe_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 相对壳 EXE 的候选：旁路绿色目录、再回退到仓库开发布局。
fn candidates_from_exe(file_name: &str) -> Vec<PathBuf> {
    let root = exe_dir();
    vec![
        root.join(file_name),
        root.join("engine").join(file_name),
        // shell/target/{debug|release}/ → 仓库 engine/target/...
        root.join("../../../engine/target/debug").join(file_name),
        root.join("../../../engine/target/release").join(file_name),
        // 若 cwd 在仓库根且直接 cargo run
        PathBuf::from("engine/target/debug").join(file_name),
        PathBuf::from("engine/target/release").join(file_name),
    ]
}

fn ui_dir_candidates() -> Vec<PathBuf> {
    let root = exe_dir();
    vec![
        root.join("ui").join("dist"),
        root.join("dist"),
        root.join("../../../ui/dist"),
        PathBuf::from("ui/dist"),
        PathBuf::from("../ui/dist"),
    ]
}

fn first_existing_file(cands: &[PathBuf]) -> Option<PathBuf> {
    cands.iter().find(|p| p.is_file()).cloned()
}

fn first_existing_dir(cands: &[PathBuf]) -> Option<PathBuf> {
    cands.iter().find(|p| p.is_dir()).cloned()
}

pub fn resolve_engine(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(canonicalize_best(&p));
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("引擎可执行文件不存在: {}", p.display()),
        ));
    }
    if let Ok(env_p) = env::var("FIELDREPLAY_ENGINE") {
        let p = PathBuf::from(env_p);
        if p.is_file() {
            return Ok(canonicalize_best(&p));
        }
    }
    #[cfg(windows)]
    let name = "fieldreplay-engine.exe";
    #[cfg(not(windows))]
    let name = "fieldreplay-engine";

    first_existing_file(&candidates_from_exe(name))
        .map(|p| canonicalize_best(&p))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "找不到 fieldreplay-engine。请先 cargo build -p fieldreplay-engine，\
                 或用 --engine 指定路径，或设置 FIELDREPLAY_ENGINE。",
            )
        })
}

pub fn resolve_ui_dir(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_dir() {
            return Ok(canonicalize_best(&p));
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("UI 目录不存在: {}", p.display()),
        ));
    }
    if let Ok(env_p) = env::var("FIELDREPLAY_UI_DIR") {
        let p = PathBuf::from(env_p);
        if p.is_dir() {
            return Ok(canonicalize_best(&p));
        }
    }
    first_existing_dir(&ui_dir_candidates())
        .map(|p| canonicalize_best(&p))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "找不到 ui/dist。请先在 ui/ 执行 npm run build，\
                 或用 --ui-dir 指定，或设置 FIELDREPLAY_UI_DIR。",
            )
        })
}

fn canonicalize_best(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_nonempty() {
        assert!(!exe_dir().as_os_str().is_empty());
    }
}
