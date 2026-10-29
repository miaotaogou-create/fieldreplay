//! 控制面同端口 HTTP 静态资源（供交付壳 / WebView 加载 UI）。

use std::fs;
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};

/// 从已读到的 HTTP 请求行解析 GET 路径；非 GET 返回 `None`。
pub fn get_path(req: &str) -> Option<String> {
    let first = req.lines().next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?;
    if !method.eq_ignore_ascii_case("GET") && !method.eq_ignore_ascii_case("HEAD") {
        return None;
    }
    let raw = parts.next()?;
    let path = raw.split('?').next().unwrap_or(raw);
    Some(percent_decode(path))
}

pub fn is_head(req: &str) -> bool {
    req.lines()
        .next()
        .map(|l| l.starts_with("HEAD ") || l.starts_with("head "))
        .unwrap_or(false)
}

/// 在 `ui_dir` 下安全解析相对路径；拒绝跳出根目录。
pub fn resolve_safe(ui_dir: &Path, url_path: &str) -> io::Result<PathBuf> {
    let trimmed = url_path.trim_start_matches('/');
    let rel = if trimmed.is_empty() {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(trimmed)
    };
    for c in rel.components() {
        match c {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "非法静态路径",
                ));
            }
        }
    }
    let root = fs::canonicalize(ui_dir).unwrap_or_else(|_| ui_dir.to_path_buf());
    let candidate = root.join(&rel);
    let canon = match fs::canonicalize(&candidate) {
        Ok(p) => p,
        Err(_) => candidate,
    };
    if !canon.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "禁止访问 UI 根目录之外",
        ));
    }
    Ok(canon)
}

pub fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
Content-Type: {content_type}\r\n\
Content-Length: {}\r\n\
Cache-Control: no-cache\r\n\
Connection: close\r\n\
\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    Ok(())
}

pub fn serve_file(
    stream: &mut TcpStream,
    ui_dir: &Path,
    url_path: &str,
    head_only: bool,
) -> io::Result<()> {
    let path = match resolve_safe(ui_dir, url_path) {
        Ok(p) => p,
        Err(_) => {
            return write_response(
                stream,
                403,
                "Forbidden",
                "text/plain; charset=utf-8",
                b"forbidden",
                head_only,
            );
        }
    };
    if !path.is_file() {
        return write_response(
            stream,
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            head_only,
        );
    }
    let body = fs::read(&path)?;
    write_response(
        stream,
        200,
        "OK",
        content_type(&path),
        &body,
        head_only,
    )
}

pub fn serve_no_ui(stream: &mut TcpStream, head_only: bool) -> io::Result<()> {
    write_response(
        stream,
        404,
        "Not Found",
        "text/plain; charset=utf-8",
        "UI 未挂载。请用 serve --ui-dir <ui/dist>，或开发期 npm run dev。".as_bytes(),
        head_only,
    )
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn get_path_strips_query() {
        let req = "GET /assets/app.js?v=1 HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(get_path(req).as_deref(), Some("/assets/app.js"));
    }

    #[test]
    fn resolve_rejects_dotdot() {
        let dir = std::env::temp_dir().join("fieldreplay_http_static_dotdot");
        let _ = fs::create_dir_all(&dir);
        assert!(resolve_safe(&dir, "/../Cargo.toml").is_err());
    }

    #[test]
    fn resolve_index_default() {
        let dir = std::env::temp_dir().join("fieldreplay_http_static_idx");
        let _ = fs::create_dir_all(&dir);
        let p = resolve_safe(&dir, "/").unwrap();
        assert!(p.ends_with("index.html"));
    }
}
