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

/// 严格校验 IPv6 字面量：8 组十六进制完整形式，或含一次 `::` 的压缩形式。
///
/// 拒绝多写 `::`（`::1::2`）、多写冒号（`2400:3200:::1`）、组数超限、
/// 单组超过 4 位等非法写法。IPv4 映射形式（如 `::ffff:1.2.3.4`）不在支持范围内。
pub fn is_ipv6(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // `::` 只能出现一次；出现即压缩形式（省略了一串全零组）
    let (head, tail) = match s.split_once("::") {
        Some((h, t)) => {
            if t.contains("::") {
                return false;
            }
            (h, Some(t))
        }
        None => (s, None),
    };

    // 数一段里用 `:` 分隔的合法组数；任何一组非法就整体返回 None
    let count_groups = |part: &str| -> Option<usize> {
        if part.is_empty() {
            return Some(0);
        }
        let mut n = 0;
        for group in part.split(':') {
            n += 1;
            // 每组 1-4 位十六进制；空组 / 超 4 位都非法
            if group.is_empty() || group.len() > 4 {
                return None;
            }
            if !group.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
        }
        Some(n)
    };

    let head_groups = match count_groups(head) {
        Some(n) => n,
        None => return false,
    };
    let tail_groups = match tail.map(count_groups) {
        Some(Some(n)) => n,
        Some(None) => return false,
        None => 0,
    };

    match tail {
        // 完整形式：必须恰好 8 组
        None => head_groups == 8,
        // 压缩形式：`::` 至少省略一组，所以非零组总数最多 7
        Some(_) => head_groups + tail_groups <= 7,
    }
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

    #[test]
    fn accepts_valid_ipv6() {
        for s in [
            "::", "::1", "2400:3200::1", "fe80::1", "2001:db8:0:0:0:0:0:2", // 完整形式允许无前导零的缩写组
            "2001:0db8:0000:0000:0000:0000:0000:0002", // 8 组完整形式
            "1:2:3:4:5:6:7:8",
        ] {
            assert!(is_ipv6(s), "{} 应该合法", s);
        }
    }

    #[test]
    fn rejects_invalid_ipv6() {
        for s in [
            "", "1.2.3.4", "2400:3200:::1", "::1::2", "1:2:3:4:5:6:7:8:9", "12345::", // 组超 4 位
            "012345::", ":1::2", "1:2:3", // 组数不足且无 ::
            "gg::1", "fe80::1 ", " fe80::1", // 含空格
            "fe80:0:0:0:0:0:0:0:1", // 9 组
        ] {
            assert!(!is_ipv6(s), "{} 不应该合法", s);
        }
    }
}
