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
    /// DNS 是否**真的读到了**。
    ///
    /// `false` 表示 `dns` 只是占位空列表（读不到），典型场景是 VPN 的 `utun*` 接口：
    /// 它没有对应的网络服务，`networksetup -getdnsservers` 必然失败，而这类接口的 DNS
    /// 也不受系统管理。此时「DNS 是空的（= 自动）」和「DNS 读不到」在数据上同形，
    /// 靠这个标志区分：前者可以照常比对 / 改写，后者不该去碰。
    ///
    /// 旧数据 / 旧前端没有这个字段时按 `true` 处理（见 `default_dns_readable`）。
    #[serde(default = "default_dns_readable")]
    pub dns_readable: bool,
}

/// `dns_readable` 的默认值：老数据里没有这个字段，按「读到了」处理，保持旧行为。
fn default_dns_readable() -> bool {
    true
}

/// 一次平台探测的原始结果：网关 + DNS + DNS 是否读到。
///
/// 单独用个结构体（而不是 `(Option<String>, DnsConfig)` 二元组）是为了让
/// 「DNS 读不到 → 降级成空列表」这件事能一路传到 [`NetworkState`]，
/// 上层据此决定「跳过 DNS 写入」还是「明确报错」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAndDns {
    pub gateway: Option<String>,
    pub dns: DnsConfig,
    /// DNS 是否真的读到了。`false` 时 `dns` 是不可信的占位空列表。
    pub dns_readable: bool,
}

impl GatewayAndDns {
    /// 正常读到网关与 DNS。
    pub fn read(gateway: Option<String>, dns: DnsConfig) -> Self {
        GatewayAndDns {
            gateway,
            dns,
            dns_readable: true,
        }
    }

    /// 网关读到了，但 DNS 读不到（已降级成空列表）。
    pub fn dns_unreadable(gateway: Option<String>) -> Self {
        GatewayAndDns {
            gateway,
            dns: DnsConfig::default(),
            dns_readable: false,
        }
    }
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

    #[test]
    fn dns_readable_defaults_to_true_for_old_payloads() {
        // 老版本序列化出来的 NetworkState 没有 dns_readable 字段，
        // 反序列化时不能炸，也不能默认成「读不到」（那会让旧数据被当成不可管理）
        let old = r#"{
            "interface": {"name": "Wi-Fi", "is_default": true},
            "gateway": "192.168.1.1",
            "dns": {"ipv4": ["223.5.5.5"]},
            "mode": "main_router"
        }"#;
        let state: NetworkState = serde_json::from_str(old).unwrap();
        assert!(state.dns_readable);
        assert_eq!(state.dns.ipv4, vec!["223.5.5.5".to_string()]);

        // 新数据明确带上这个字段时按实际值来
        let state = NetworkState {
            interface: NetworkInterface {
                name: "utun3".into(),
                index: None,
                device: Some("utun3".into()),
                is_default: true,
            },
            gateway: Some("10.8.0.1".into()),
            dns: DnsConfig::default(),
            mode: RouterMode::Unknown,
            dns_readable: false,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"dns_readable\":false"), "{}", json);
        let round: NetworkState = serde_json::from_str(&json).unwrap();
        assert!(!round.dns_readable);
    }
}
