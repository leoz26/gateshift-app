//! 统一的错误类型。核心库只暴露这一个 `Error`，不把 `anyhow` 之类的选择强加给调用方。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("当前平台不受支持（只实现了 macOS 与 Windows）")]
    UnsupportedPlatform,

    #[error("找不到默认网络接口")]
    NoDefaultInterface,

    #[error("找不到网络接口「{0}」")]
    InterfaceNotFound(String),

    #[error("网络接口已变化：快照记录的是「{expected}」，当前是「{actual}」")]
    InterfaceChanged { expected: String, actual: String },

    #[error("无法判断当前档位：网关「{0}」既不匹配主路由也不匹配旁路由")]
    ModeUnknown(String),

    #[error("不能切到「未知」档位")]
    ModeUnspecified,

    #[error("当前接口（{0}）的 DNS 不受系统管理，常见于 VPN 连接期间，无法对齐 DNS")]
    DnsUnmanageable(String),

    #[error("配置无效：{0}")]
    InvalidConfig(String),

    #[error("「{0}」不是合法的 IPv4 地址")]
    InvalidIpv4(String),

    #[error("读取配置失败：{0}")]
    ConfigRead(String),

    #[error("解析配置失败：{0}")]
    ConfigParse(String),

    #[error("写入配置失败：{0}")]
    ConfigWrite(String),

    #[error("无法启动 {program}：{source}")]
    Command {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{program} 执行失败：{stderr}")]
    CommandFailed { program: String, stderr: String },

    #[error("已取消系统授权")]
    ElevationCancelled,

    #[error("提权执行失败：{0}")]
    Elevation(String),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// 稳定的机器可读错误码。
    ///
    /// `Display` 出来的中文文案是给人看的，措辞、语言随时会改；调用方（Tauri 命令层、
    /// 前端）要做分支判断时只应该依赖这里的码，例如判断「用户取消了系统授权」用
    /// [`Error::ElevationCancelled`] → `"elevation_cancelled"`，而不是去
    /// `message.includes("取消")`（文案一改就悄悄失效）。
    ///
    /// 这些字符串属于对外契约：只增不改（改名等于破坏兼容），新增变体时补一个码。
    pub fn code(&self) -> &'static str {
        match self {
            Error::UnsupportedPlatform => "unsupported_platform",
            Error::NoDefaultInterface => "no_default_interface",
            Error::InterfaceNotFound(_) => "interface_not_found",
            Error::InterfaceChanged { .. } => "interface_changed",
            Error::ModeUnknown(_) => "mode_unknown",
            Error::ModeUnspecified => "mode_unspecified",
            // 「这个接口的 DNS 我们管不了」（VPN 虚拟接口）：前端该提示用户
            // 「当前连接下无法对齐 DNS」，而不是当成普通执行失败重试。
            Error::DnsUnmanageable(_) => "dns_unmanageable",
            Error::InvalidConfig(_) => "invalid_config",
            Error::InvalidIpv4(_) => "invalid_ipv4",
            Error::ConfigRead(_) => "config_read",
            Error::ConfigParse(_) => "config_parse",
            Error::ConfigWrite(_) => "config_write",
            // 区分「命令根本起不来」（环境问题）和「命令跑了但失败」（权限 / 参数），
            // 前端的提示语可以不一样。
            Error::Command { .. } => "command_spawn_failed",
            Error::CommandFailed { .. } => "command_failed",
            Error::ElevationCancelled => "elevation_cancelled",
            Error::Elevation(_) => "elevation_failed",
            Error::Io(_) => "io",
        }
    }
}

/// 命令报错时 stderr 可能为空，给个可读的占位，别让错误信息以冒号结尾。
pub(crate) fn stderr_or_placeholder(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).trim().to_string();
    if s.is_empty() {
        "(无错误输出)".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_cancelled_has_stable_code() {
        // 前端据这个码决定「已取消，未做修改」而不是报错
        assert_eq!(Error::ElevationCancelled.code(), "elevation_cancelled");
    }

    #[test]
    fn dns_unmanageable_has_stable_code() {
        // 前端据这个码提示「当前连接下无法对齐 DNS」（VPN 场景），而不是当成普通失败
        assert_eq!(
            Error::DnsUnmanageable("utun3".into()).code(),
            "dns_unmanageable"
        );
    }

    #[test]
    fn codes_are_snake_case_ascii() {
        for e in [
            Error::UnsupportedPlatform,
            Error::NoDefaultInterface,
            Error::InterfaceNotFound("x".into()),
            Error::InterfaceChanged {
                expected: "a".into(),
                actual: "b".into(),
            },
            Error::ModeUnknown("g".into()),
            Error::ModeUnspecified,
            Error::DnsUnmanageable("utun3".into()),
            Error::InvalidConfig("c".into()),
            Error::InvalidIpv4("1".into()),
            Error::ConfigRead("r".into()),
            Error::ConfigParse("p".into()),
            Error::ConfigWrite("w".into()),
            Error::CommandFailed {
                program: "p".into(),
                stderr: "s".into(),
            },
            Error::ElevationCancelled,
            Error::Elevation("e".into()),
        ] {
            let code = e.code();
            assert!(!code.is_empty(), "{e} 没有错误码");
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "错误码「{code}」应该是 snake_case ascii"
            );
            assert!(!code.contains("__") && !code.ends_with('_'), "{code}");
        }
    }

    #[test]
    fn distinct_variants_get_distinct_codes() {
        let variants = [
            Error::UnsupportedPlatform,
            Error::NoDefaultInterface,
            Error::InterfaceNotFound("x".into()),
            Error::InterfaceChanged {
                expected: "a".into(),
                actual: "b".into(),
            },
            Error::ModeUnknown("g".into()),
            Error::ModeUnspecified,
            Error::DnsUnmanageable("utun3".into()),
            Error::InvalidConfig("c".into()),
            Error::InvalidIpv4("1".into()),
            Error::ConfigRead("r".into()),
            Error::ConfigParse("p".into()),
            Error::ConfigWrite("w".into()),
            Error::CommandFailed {
                program: "p".into(),
                stderr: "s".into(),
            },
            Error::ElevationCancelled,
            Error::Elevation("e".into()),
        ];
        let mut codes: Vec<&str> = variants.iter().map(|e| e.code()).collect();
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total, "错误码不能撞车：{codes:?}");
    }

    #[test]
    fn spawn_failure_and_run_failure_are_distinguishable() {
        let spawn = Error::Command {
            program: "osascript".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        let run = Error::CommandFailed {
            program: "route".into(),
            stderr: "boom".into(),
        };
        assert_ne!(spawn.code(), run.code());
    }
}
