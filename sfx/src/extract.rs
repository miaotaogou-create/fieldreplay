//! 选择解压目录、版本复用、覆盖展开 zip。

use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

/// 解压目录内的版本标记文件名（与绿包并存，勿与业务文件冲突）。
pub const VERSION_FILE: &str = ".fr_sfx_version";

pub struct ExtractPlan {
    pub dir: PathBuf,
    pub skipped: bool,
}

/// 优先启动器同级 `FieldReplay\`；不可写则 `%LOCALAPPDATA%\FieldReplay\`。
pub fn resolve_extract_dir(sfx_exe: &Path) -> io::Result<PathBuf> {
    let exe_dir = sfx_exe
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "无法解析启动器所在目录"))?;
    let sibling = exe_dir.join("FieldReplay");
    if ensure_writable_dir(&sibling).is_ok() {
        return Ok(sibling);
    }

    let local = local_app_data()?.join("FieldReplay");
    ensure_writable_dir(&local)?;
    Ok(local)
}

fn local_app_data() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "未设置 LOCALAPPDATA，无法回退解压目录",
                )
            })
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "自解压仅支持 Windows",
        ))
    }
}

fn ensure_writable_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".fr_sfx_write_probe");
    {
        let mut f = File::create(&probe)?;
        f.write_all(b"ok")?;
    }
    fs::remove_file(&probe)?;
    Ok(())
}

pub fn prepare(dir: &Path, version: &str, zip_bytes: &[u8]) -> io::Result<ExtractPlan> {
    let mark = dir.join(VERSION_FILE);
    let shell = dir.join("fieldreplay.exe");
    if mark.is_file() && shell.is_file() {
        let existing = fs::read_to_string(&mark).unwrap_or_default();
        if existing.trim() == version {
            return Ok(ExtractPlan {
                dir: dir.to_path_buf(),
                skipped: true,
            });
        }
    }

    extract_zip_overwrite(dir, zip_bytes)?;
    fs::write(&mark, version.as_bytes())?;
    if !shell.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("解压后未找到壳程序: {}", shell.display()),
        ));
    }

    Ok(ExtractPlan {
        dir: dir.to_path_buf(),
        skipped: false,
    })
}

fn extract_zip_overwrite(dest: &Path, zip_bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无法打开内嵌 zip: {e}"),
        )
    })?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("读取 zip 条目失败: {e}"))
        })?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("拒绝不安全的 zip 路径: {}", file.name()),
                )
            })?
            .to_path_buf();

        let out_path = dest.join(&name);
        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)?;
        io::copy(&mut file, &mut out)?;
    }
    Ok(())
}
