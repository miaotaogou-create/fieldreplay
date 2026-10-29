//! 标准 Base64（RFC 4648），控制面载荷编解码用。

const ENC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(ENC[((n >> 18) & 0x3f) as usize] as char);
        out.push(ENC[((n >> 12) & 0x3f) as usize] as char);
        out.push(ENC[((n >> 6) & 0x3f) as usize] as char);
        out.push(ENC[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(ENC[((n >> 18) & 0x3f) as usize] as char);
        out.push(ENC[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(ENC[((n >> 18) & 0x3f) as usize] as char);
        out.push(ENC[((n >> 12) & 0x3f) as usize] as char);
        out.push(ENC[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

fn dec_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if bytes.len() % 4 != 0 {
        return Err("base64 长度非法".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                if i < 2 {
                    return Err("base64 填充非法".into());
                }
                continue;
            }
            let v = dec_val(c).ok_or_else(|| "base64 字符非法".to_string())?;
            n |= (v as u32) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let samples: &[&[u8]] = &[b"", b"f", b"fo", b"foo", b"FCAP", &[0, 1, 2, 255]];
        for s in samples {
            let e = encode(s);
            let d = decode(&e).expect("decode");
            assert_eq!(&d, s);
        }
    }
}
