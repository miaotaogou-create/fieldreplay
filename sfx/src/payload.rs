//! 从本 EXE 尾部读取打包脚本追加的 zip 载荷与版本标记。
//!
//! 布局（文件末尾）：
//! `[stub][zip][version UTF-8][ver_len u32 LE][zip_len u64 LE][magic 8]`

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// 与 `tools/pack_sfx.ps1` 必须一致。
pub const MAGIC: &[u8; 8] = b"FRSFX001";

const TAIL_FIXED: u64 = 8 + 8 + 4; // magic + zip_len + ver_len

pub struct Payload {
    pub zip_bytes: Vec<u8>,
    pub version: String,
}

pub fn read_from_exe(exe: &Path) -> io::Result<Payload> {
    let mut f = File::open(exe)?;
    let file_len = f.seek(SeekFrom::End(0))?;
    if file_len < TAIL_FIXED {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "不是有效的 FieldReplay 自解压包（文件过短）",
        ));
    }

    f.seek(SeekFrom::End(-(TAIL_FIXED as i64)))?;
    let mut ver_len_buf = [0u8; 4];
    let mut zip_len_buf = [0u8; 8];
    let mut magic = [0u8; 8];
    f.read_exact(&mut ver_len_buf)?;
    f.read_exact(&mut zip_len_buf)?;
    f.read_exact(&mut magic)?;

    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "不是有效的 FieldReplay 自解压包（缺少载荷标记；请用 tools\\pack_sfx 生成）",
        ));
    }

    let ver_len = u32::from_le_bytes(ver_len_buf) as u64;
    let zip_len = u64::from_le_bytes(zip_len_buf);
    let need = TAIL_FIXED
        .checked_add(ver_len)
        .and_then(|n| n.checked_add(zip_len))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "载荷长度溢出"))?;
    if need > file_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "自解压载荷长度与文件不符",
        ));
    }

    let version_off = file_len - TAIL_FIXED - ver_len;
    let zip_off = version_off - zip_len;

    f.seek(SeekFrom::Start(zip_off))?;
    let mut zip_bytes = vec![0u8; zip_len as usize];
    f.read_exact(&mut zip_bytes)?;

    let mut ver_bytes = vec![0u8; ver_len as usize];
    f.read_exact(&mut ver_bytes)?;
    let version = String::from_utf8(ver_bytes).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "自解压版本标记不是合法 UTF-8")
    })?;

    if zip_bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "自解压 zip 载荷为空",
        ));
    }
    if version.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "自解压版本标记为空",
        ));
    }

    Ok(Payload { zip_bytes, version })
}
