//! 平台实现。业务逻辑只依赖 [`NetworkPlatform`]，各平台差异全部隔离在自己的文件里。

use crate::error::Result;
use crate::model::{DnsConfig, NetworkInterface};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacOsPlatform as Platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as Platform;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use unsupported::UnsupportedPlatform as Platform;

/// PowerShell 参数编码。放在这里（而不是 windows 模块里）是为了让
/// UTF-16LE + Base64 这段最容易写错的逻辑能在任意平台跑单元测试。
mod powershell;

/// Windows 侧的纯字符串逻辑（脚本、占位符替换、输出解析）。
/// 同样跨平台编译，好在 macOS / Linux 上也能测。
mod windows_scripts;

/// 当前平台的默认实现。
pub fn default_platform() -> Box<dyn NetworkPlatform> {
    Box::new(Platform)
}

/// 平台相关操作的统一抽象。
///
/// 约定：
/// - **读**操作（`list_interfaces` / `default_interface` / `get_gateway` / `get_dns`）不应触发提权。
/// - **写**操作（`set_gateway` / `set_dns` / `flush_dns` / `apply`）按需触发系统授权。
/// - 传入的 IP 一律先校验，非法值直接返回 [`crate::Error::InvalidIpv4`]，绝不拼进命令。
pub trait NetworkPlatform: Send + Sync {
    /// 列出可用接口，`is_default` 标记当前默认路由走的那个。
    fn list_interfaces(&self) -> Result<Vec<NetworkInterface>>;

    /// 当前默认路由走的接口。
    fn default_interface(&self) -> Result<NetworkInterface>;

    /// 读取该接口当前的默认网关。
    fn get_gateway(&self, interface: &NetworkInterface) -> Result<Option<String>>;

    /// 把默认网关设为 `gateway`。
    fn set_gateway(&self, interface: &NetworkInterface, gateway: &str) -> Result<()>;

    /// 读取该接口的 IPv4 DNS；空列表表示「自动（DHCP）」。
    fn get_dns(&self, interface: &NetworkInterface) -> Result<DnsConfig>;

    /// 设置该接口的 IPv4 DNS；`dns` 为空表示恢复「自动（DHCP）」。
    fn set_dns(&self, interface: &NetworkInterface, dns: &[String]) -> Result<()>;

    /// 刷新 DNS 解析缓存。
    fn flush_dns(&self) -> Result<()>;

    /// 一次把「网关 + DNS」都应用掉，并刷新缓存。
    ///
    /// 默认实现是分开调用；需要提权的平台应当覆写它，把多步合并成**一次**授权弹窗。
    fn apply(
        &self,
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
    ) -> Result<()> {
        if let Some(gateway) = gateway {
            self.set_gateway(interface, gateway)?;
        }
        if let Some(dns) = dns {
            self.set_dns(interface, dns)?;
        }
        self.flush_dns()
    }
}
