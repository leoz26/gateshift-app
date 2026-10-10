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

/// 命令报错时 stderr 可能为空，给个可读的占位，别让错误信息以冒号结尾。
pub(crate) fn stderr_or_placeholder(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).trim().to_string();
    if s.is_empty() {
        "(无错误输出)".to_string()
    } else {
        s
    }
}
