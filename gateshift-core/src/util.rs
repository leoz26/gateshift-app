//! 小工具：IP 字面量校验。
//!
//! 所有平台实现在拼命令**之前**都会先过一遍这里，非法输入直接报错、绝不碰系统。

/// 严格校验 IPv4 字面量：四段十进制，每段 0-255。
pub fn is_ipv4(s: &str) -> bool {
    let mut segments = 0;
    for part in s.split('.') {
        segments += 1;
        if segments > 4 || part.is_empty() || part.len() > 3 {
            return false;
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        // 拒绝 "192.168.01.1" 这类前导零写法（在某些解析器里会被当成八进制）
        if part.len() > 1 && part.starts_with('0') {
            return false;
        }
        match part.parse::<u16>() {
            Ok(v) if v <= 255 => {}
            _ => return false,
        }
    }
    segments == 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_ipv4() {
        for s in ["0.0.0.0", "192.168.1.1", "255.255.255.255", "8.8.8.8"] {
            assert!(is_ipv4(s), "{} 应该合法", s);
        }
    }

    #[test]
    fn rejects_invalid_ipv4() {
        for s in [
            "", "192.168.1", "192.168.1.1.1", "192.168.1.256", "abc", "192.168.1.-1",
            "192.168.1.1 ", " 192.168.1.1", "fe80::1", "192.168.01.1",
        ] {
            assert!(!is_ipv4(s), "{} 不应该合法", s);
        }
    }
}
