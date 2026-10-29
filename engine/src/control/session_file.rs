//! 内存会话 ↔ 磁盘 `.fcap`（重放物化 / 导出共用）。

use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fcap::SessionWriter;

use super::state::LoadedSession;

/// 将内存会话写成 finalized `.fcap`；`meta_override` 非空时覆盖文件内 SessionMeta。
pub fn write_loaded_session(
    path: &Path,
    session: &LoadedSession,
    meta_override: Option<&str>,
) -> io::Result<()> {
    let meta = meta_override.or(session.meta_json.as_deref());
    let mut w = SessionWriter::create(
        path,
        session.start_wall_unix_us,
        &session.channels,
        meta,
    )?;
    for pkt in &session.packets {
        w.append_packet(pkt)?;
    }
    w.finalize()?;
    Ok(())
}

/// 导出到临时目录并读回字节（控制面 `session.export`）。
pub fn export_session_bytes(
    session: &LoadedSession,
    meta_override: Option<&str>,
) -> io::Result<(Vec<u8>, u64)> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join("fieldreplay_export");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("export_{ms}.fcap"));
    write_loaded_session(&path, session, meta_override)?;
    let bytes = std::fs::read(&path)?;
    let count = session.packets.len() as u64;
    let _ = std::fs::remove_file(&path);
    Ok((bytes, count))
}
