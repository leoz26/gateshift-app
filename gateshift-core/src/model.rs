//! 数据模型。全部 `serde` 化，方便直接喂给未来的界面层（Tauri / 任意前端）。

use serde::{Deserialize, Serialize};

/// 一个网络接口 / 网络服务。
///
/// `name` 是**跨平台统一**的定位标识，也是写操作真正用到的那个名字：
/// - macOS：网络服务名（如 `Wi-Fi`）—— `networksetup` 认的就是它
/// - Windows：网卡别名（如 `以太网`）—— 仅用于展示，写操作靠 `index`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    /// Windows 的 `ifIndex`；macOS 为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    /// macOS 的 BSD 设备名（如 `en0`）；Windows 为 `None`。仅用于展示 / 诊断。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// 是否是当前默认路由走的那个接口。
    pub is_default: bool,
}

impl NetworkInterface {
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(index) = self.index {
            parts.push(format!("index={}", index));
        }
        if let Some(device) = &self.device {
            parts.push(format!("device={}", device));
        }
        if parts.is_empty() {
            "—".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// 某个接口上的 DNS 配置。当前只管理 IPv4。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsConfig {
    #[serde(default)]
    pub ipv4: Vec<String>,
}

impl DnsConfig {
    pub fn new(ipv4: Vec<String>) -> Self {
        DnsConfig { ipv4 }
    }

    /// 空列表 == 自动（DHCP）。
    pub fn is_auto(&self) -> bool {
        self.ipv4.is_empty()
    }

    pub fn describe(&self) -> String {
        if self.is_auto() {
            "自动（DHCP）".to_string()
        } else {
            self.ipv4.join(", ")
        }
    }
}

/// 当前处在哪个档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterMode {
    MainRouter,
    BypassRouter,
    Unknown,
}

impl RouterMode {
    pub fn label(self) -> &'static str {
        match self {
            RouterMode::MainRouter => "主路由",
            RouterMode::BypassRouter => "旁路由",
            RouterMode::Unknown => "未知",
        }
    }
}

/// 一次探测的完整快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkState {
    pub interface: NetworkInterface,
    pub gateway: Option<String>,
    pub dns: DnsConfig,
    pub mode: RouterMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_auto_when_empty() {
        assert!(DnsConfig::default().is_auto());
        assert_eq!(DnsConfig::default().describe(), "自动（DHCP）");
        assert_eq!(
            DnsConfig::new(vec!["1.1.1.1".into()]).describe(),
            "1.1.1.1"
        );
    }

    #[test]
    fn router_mode_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&RouterMode::BypassRouter).unwrap(),
            "\"bypass_router\""
        );
        let back: RouterMode = serde_json::from_str("\"main_router\"").unwrap();
        assert_eq!(back, RouterMode::MainRouter);
    }

    #[test]
    fn interface_describe_lists_platform_fields() {
        let mac = NetworkInterface {
            name: "Wi-Fi".into(),
            index: None,
            device: Some("en1".into()),
            is_default: true,
        };
        assert_eq!(mac.describe(), "device=en1");

        let win = NetworkInterface {
            name: "以太网".into(),
            index: Some(7),
            device: None,
            is_default: true,
        };
        assert_eq!(win.describe(), "index=7");
    }
}
