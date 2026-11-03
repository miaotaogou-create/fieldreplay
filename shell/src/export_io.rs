//! 导出落盘：把字节写入用户另存为选定的绝对路径。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::b64;

/// 由 `.fcap` 路径得到同目录同基名的 `.pcap`（供测试与后续接线）。
#[cfg_attr(not(test), allow(dead_code))]
pub fn pcap_path_beside_fcap(fcap: &Path) -> PathBuf {
    fcap.with_extension("pcap")
}

/// 将原始字节写入 `path`（必要时创建父目录；先写临时文件再替换）。
pub fn write_bytes(path: &Path, data: &[u8]) -> io::Result<()> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "写入路径为空"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let tmp = {
        let mut t = path.as_os_str().to_owned();
        t.push(".fr_writing");
        PathBuf::from(t)
    };
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    // Windows 上目标已存在时 rename 可能失败，先删再移
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e
    })?;
    Ok(())
}

/// 解码 base64 后写入；供壳 IPC。
pub fn write_b64(path: &Path, b64s: &str) -> io::Result<()> {
    let data = b64::decode(b64s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_bytes(path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "fieldreplay-export-io-{}-{}",
            std::process::id(),
            ns
        ));
        fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn write_bytes_roundtrip_and_pcap_beside() {
        let dir = temp_dir();
        let fcap = dir.join("demo").join("pack.fcap");
        write_bytes(&fcap, b"FCAP-demo").expect("write");
        assert_eq!(fs::read(&fcap).unwrap(), b"FCAP-demo");

        let pcap = pcap_path_beside_fcap(&fcap);
        assert_eq!(pcap, dir.join("demo").join("pack.pcap"));
        // \xd4\xc3\xb2\xa1 → base64
        write_b64(&pcap, "1MOyoQ==").expect("pcap");
        assert_eq!(fs::read(&pcap).unwrap(), b"\xd4\xc3\xb2\xa1");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn overwrite_existing() {
        let dir = temp_dir();
        let p = dir.join("a.fcap");
        write_bytes(&p, b"old").unwrap();
        write_bytes(&p, b"new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }
}
