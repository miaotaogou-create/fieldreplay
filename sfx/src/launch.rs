//! 从解压目录拉起现有壳 `fieldreplay.exe`。

use std::io;
use std::path::Path;
use std::process::Command;

pub fn start_shell(extract_dir: &Path) -> io::Result<()> {
    let shell = extract_dir.join("fieldreplay.exe");
    if !shell.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("未找到壳程序: {}", shell.display()),
        ));
    }

    let mut cmd = Command::new(&shell);
    cmd.current_dir(extract_dir);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    cmd.spawn().map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("无法启动壳程序 {}: {e}", shell.display()),
        )
    })?;
    Ok(())
}
