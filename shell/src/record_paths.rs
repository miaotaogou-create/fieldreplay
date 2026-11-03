//! 录制默认落盘：`{壳EXE目录}\record\<YYYY-MM-DD>\*.fcap`

use std::io;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths::exe_dir;

/// 本地日历日 `YYYY-MM-DD`（Windows 用 GetLocalTime；其它平台用 UTC 日）。
pub fn local_date_ymd() -> String {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        extern "system" {
            fn GetLocalTime(lp: *mut SystemTime);
        }
        let mut st = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut st) };
        return format!("{:04}-{:02}-{:02}", st.year, st.month, st.day);
    }
    #[cfg(not(windows))]
    {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // ponytail: 非 Windows 回退 UTC 日；正式交付是 Windows 壳
        let days = secs / 86400;
        let (y, m, d) = civil_from_days(days as i64);
        format!("{:04}-{:02}-{:02}", y, m, d)
    }
}

#[cfg(not(windows))]
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    // Howard Hinnant 算法：Unix 日 → 公历
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// `{exe}\record\YYYY-MM-DD`，必要时创建。
pub fn ensure_today_record_dir() -> io::Result<PathBuf> {
    let dir = exe_dir().join("record").join(local_date_ymd());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn local_hms() -> (u16, u16, u16) {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        extern "system" {
            fn GetLocalTime(lp: *mut SystemTime);
        }
        let mut st = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut st) };
        (st.hour, st.minute, st.second)
    }
    #[cfg(not(windows))]
    {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let tod = (secs % 86400) as u16;
        (tod / 3600, (tod % 3600) / 60, tod % 60)
    }
}

/// 建议文件名（不含目录）：`现场录制_HHMMSS.fcap`
pub fn suggested_record_file_name() -> String {
    let (h, m, s) = local_hms();
    format!("现场录制_{h:02}{m:02}{s:02}.fcap")
}

/// 默认完整绝对路径（建好日期目录）。
pub fn default_record_path() -> io::Result<PathBuf> {
    let dir = ensure_today_record_dir()?;
    Ok(dir.join(suggested_record_file_name()))
}

/// 弹出另存为；取消返回 `Ok(None)`。
pub fn pick_save_fcap(suggested_name: Option<&str>) -> io::Result<Option<PathBuf>> {
    let dir = ensure_today_record_dir()?;
    let name = suggested_name
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.to_ascii_lowercase().ends_with(".fcap") {
                s.to_string()
            } else {
                format!("{s}.fcap")
            }
        })
        .unwrap_or_else(suggested_record_file_name);

    let picked = rfd::FileDialog::new()
        .add_filter("FieldReplay 会话", &["fcap"])
        .set_directory(&dir)
        .set_file_name(&name)
        .save_file();

    Ok(picked.map(|p| ensure_fcap_extension(p)))
}

/// 弹出打开 `.fcap`；取消返回 `Ok(None)`。初始目录对齐当日 `record\日期`（U12）。
pub fn pick_open_fcap() -> io::Result<Option<PathBuf>> {
    let dir = ensure_today_record_dir()?;
    let picked = rfd::FileDialog::new()
        .add_filter("FieldReplay 会话", &["fcap"])
        .set_directory(&dir)
        .pick_file();
    Ok(picked)
}

fn ensure_fcap_extension(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("fcap") => path,
        _ => {
            let mut s = path.into_os_string();
            s.push(".fcap");
            PathBuf::from(s)
        }
    }
}

/// 供脚本回传：把路径写成 JS 字符串字面量内容（已转义，不含外层引号）。
pub fn js_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_escape_quotes_and_backslash() {
        assert_eq!(js_escape(r#"a\b"c"#), r#"a\\b\"c"#);
    }

    #[test]
    fn default_path_under_record_date_and_fcap() {
        let p = default_record_path().expect("应能创建默认录制目录");
        let s = path_to_string(&p);
        let date = local_date_ymd();
        assert!(
            s.contains(&format!("record{}{}", std::path::MAIN_SEPARATOR, date))
                || s.contains(&format!("record/{date}")),
            "路径应含 record\\日期: {s}"
        );
        assert!(
            s.to_ascii_lowercase().ends_with(".fcap"),
            "应以 .fcap 结尾: {s}"
        );
        assert!(p.parent().map(|d| d.is_dir()).unwrap_or(false));
    }

    #[test]
    fn ensure_fcap_extension_appends_when_missing() {
        let p = ensure_fcap_extension(PathBuf::from("C:\\tmp\\demo"));
        assert_eq!(p, PathBuf::from("C:\\tmp\\demo.fcap"));
        let keep = ensure_fcap_extension(PathBuf::from("C:\\tmp\\a.FCAP"));
        assert_eq!(keep, PathBuf::from("C:\\tmp\\a.FCAP"));
    }

    #[test]
    fn open_dialog_initial_dir_is_today_record() {
        // 不弹窗；只验 U12 打开对话框依赖的建议目录可创建且含 record\日期
        let dir = ensure_today_record_dir().expect("应能创建当日 record 目录");
        let s = path_to_string(&dir);
        let date = local_date_ymd();
        assert!(
            s.contains(&format!("record{}{}", std::path::MAIN_SEPARATOR, date))
                || s.contains(&format!("record/{date}")),
            "打开建议目录应含 record\\日期: {s}"
        );
        assert!(dir.is_dir());
    }
}
