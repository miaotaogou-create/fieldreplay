//! 控制面最小 JSON 拼装 / 字段抽取（已知请求形状，不引入 serde）。

pub fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn after_key<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    Some(after.trim_start_matches(|c: char| c == ':' || c.is_whitespace()))
}

pub fn extract_str(json: &str, key: &str) -> Option<String> {
    let after = after_key(json, key)?;
    if !after.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        hex.push(chars.next()?);
                    }
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                other => out.push(other),
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// 抽取 JSON 数字字段（整数）。
pub fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let after = after_key(json, key)?;
    let end = after
        .find(|c: char| !(c.is_ascii_digit()))
        .unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    after[..end].parse().ok()
}

/// 抽取 JSON 布尔字段。
pub fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let after = after_key(json, key)?;
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// 抽取 JSON 浮点/整数字段（如倍速 `speed`）。
pub fn extract_f64(json: &str, key: &str) -> Option<f64> {
    let after = after_key(json, key)?;
    let end = after
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))
        .unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    after[..end].parse().ok()
}

/// 抽取嵌套对象的原始子串（含首尾大括号）。
pub fn extract_object(json: &str, key: &str) -> Option<String> {
    let after = after_key(json, key)?;
    if !after.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in after.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 抽取 JSON 数组的原始子串（含首尾方括号）。
pub fn extract_array(json: &str, key: &str) -> Option<String> {
    let after = after_key(json, key)?;
    if !after.starts_with('[') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in after.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 将 JSON 数组拆成顶层元素原始子串（对象/数组/字面量）。
pub fn split_json_array_elements(array_json: &str) -> Vec<String> {
    let s = array_json.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Vec::new();
    }
    let inner = &s[1..s.len() - 1];
    let mut out = Vec::new();
    let mut start = None;
    let mut depth_obj = 0i32;
    let mut depth_arr = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in inner.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                if start.is_none() {
                    start = Some(i);
                }
                in_str = true;
            }
            '{' => {
                if start.is_none() {
                    start = Some(i);
                }
                depth_obj += 1;
            }
            '}' => depth_obj -= 1,
            '[' => {
                if start.is_none() {
                    start = Some(i);
                }
                depth_arr += 1;
            }
            ']' => depth_arr -= 1,
            ',' if depth_obj == 0 && depth_arr == 0 => {
                if let Some(st) = start {
                    let elem = inner[st..i].trim();
                    if !elem.is_empty() {
                        out.push(elem.to_string());
                    }
                }
                start = None;
            }
            c if !c.is_whitespace() => {
                if start.is_none() {
                    start = Some(i);
                }
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        let elem = inner[st..].trim();
        if !elem.is_empty() {
            out.push(elem.to_string());
        }
    }
    out
}

pub fn ok_result(id: &str, result_json: &str) -> String {
    format!(
        "{{\"id\":\"{}\",\"ok\":true,\"result\":{}}}",
        escape_str(id),
        result_json
    )
}

pub fn err_result(id: &str, message: &str) -> String {
    format!(
        "{{\"id\":\"{}\",\"ok\":false,\"error\":\"{}\"}}",
        escape_str(id),
        escape_str(message)
    )
}
