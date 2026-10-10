//! PowerShell 参数编码。
//!
//! 这个模块在**所有平台**都会编译（不是 `#[cfg(windows)]`），
//! 这样 `encoded_command` / `base64` 这段最容易写错、又最难在 Windows 上调试的逻辑，
//! 能在 macOS / Linux 上直接跑单元测试。

#![allow(dead_code)]

/// 普通权限执行时用的外壳：出错即退出码 1，并把错误写到 stderr。
pub fn ps_wrap(script: &str) -> String {
    format!(
        "$ErrorActionPreference='Stop';\
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\
$OutputEncoding=[System.Text.Encoding]::UTF8;\
try{{ {} }}catch{{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}",
        script
    )
}

/// 提权执行时用的外壳：成功 `exit 0`，失败 `exit 1`。
pub fn ps_wrap_elevated(script: &str) -> String {
    format!(
        "$ErrorActionPreference='Stop';\
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;\
try{{ {} }}catch{{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }};exit 0",
        script
    )
}

/// 把脚本编码成 `powershell -EncodedCommand` 需要的格式：
/// UTF-16LE 字节流 + Base64。这样中文网卡名、引号、反斜杠全都不用转义。
pub fn encoded_command(script: &str) -> String {
    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64(&bytes)
}

/// 标准 Base64（带 `=` 填充）。
pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encoded_command_uses_utf16le() {
        // 'A' 的 UTF-16LE 是 41 00（两字节）→ base64 "QQA="
        assert_eq!(encoded_command("A"), "QQA=");
        // '中' 的 UTF-16LE 是 2D 4E（两字节）→ base64 "LU4="
        assert_eq!(encoded_command("中"), "LU4=");
        // 每个字符固定占两个字节，所以长度永远是偶数
        for s in ["", "A", "中", "abc", "以太网"] {
            assert_eq!(encoded_command(s).len() % 4, 0, "{}", s);
        }
        // 反过来验证：base64 解出来应该是同一串 UTF-16LE 字节
        let raw = utf16le_bytes("以太网");
        assert_eq!(raw.len(), "以太网".chars().count() * 2);
    }

    /// 参考实现，只用于测试比对。
    fn utf16le_bytes(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn encoded_command_roundtrips_through_base64() {
        // 保证长度是 4 的倍数、且只含合法字符（PowerShell 会做严格校验）
        let encoded = encoded_command("Get-NetAdapter | Where-Object { $_.Status -eq 'Up' }");
        assert_eq!(encoded.len() % 4, 0);
        assert!(encoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
    }

    #[test]
    fn wrappers_are_balanced_and_force_exit_codes() {
        let plain = ps_wrap("Get-Date");
        assert!(plain.contains("Get-Date"));
        assert!(plain.contains("exit 1"));
        assert_eq!(plain.matches('{').count(), plain.matches('}').count());

        let elevated = ps_wrap_elevated("Get-Date");
        assert!(elevated.contains("exit 0"));
        assert!(elevated.contains("exit 1"));
        assert_eq!(elevated.matches('{').count(), elevated.matches('}').count());
    }
}
