//! 统一入口。界面层只需要拿一个 [`NetworkManager`]，不需要知道下面跑的是哪个平台。
//!
//! 平台实现通过 `Box<dyn NetworkPlatform>` 注入（见 [`NetworkManager::with_platform`]），
//! 所以整套逻辑可以脱离真实系统做单元测试。

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::model::{DnsConfig, NetworkInterface, NetworkState, RouterMode};
use crate::platform::{self, NetworkPlatform};

/// 改动前的现场记录，用于之后还原。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub interface: NetworkInterface,
    pub dns: DnsConfig,
}

pub struct NetworkManager {
    config: Config,
    platform: Box<dyn NetworkPlatform>,
    /// 手动指定的接口名；`None` = 自动选当前默认接口。
    interface: Option<String>,
}

impl NetworkManager {
    /// 用当前平台的默认实现创建。
    pub fn new(config: Config) -> Result<Self> {
        Self::with_platform(config, platform::default_platform())
    }

    /// 注入自定义平台实现（测试 / 特殊环境）。
    pub fn with_platform(config: Config, platform: Box<dyn NetworkPlatform>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            platform,
            interface: None,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 换配置（会先校验）。
    pub fn set_config(&mut self, config: Config) -> Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    /// 指定操作哪个接口；传 `None` 恢复「自动选默认接口」。
    pub fn set_interface(&mut self, name: Option<String>) {
        self.interface = name;
    }

    /// 实际要操作的接口。
    fn target_interface(&self) -> Result<NetworkInterface> {
        match &self.interface {
            None => self.platform.default_interface(),
            Some(name) => self
                .platform
                .list_interfaces()?
                .into_iter()
                .find(|i| i.name == *name)
                .ok_or_else(|| Error::InterfaceNotFound(name.clone())),
        }
    }

    pub fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        self.platform.list_interfaces()
    }

    /// 只读探测：网卡 / 网关 / DNS / 档位。
    pub fn probe(&self) -> Result<NetworkState> {
        self.detect()
    }

    pub fn detect(&self) -> Result<NetworkState> {
        let interface = self.target_interface()?;
        let gateway = self.platform.get_gateway(&interface)?;
        let dns = self.platform.get_dns(&interface)?;
        let mode = self.mode_of(gateway.as_deref());
        Ok(NetworkState {
            interface,
            gateway,
            dns,
            mode,
        })
    }

    /// 根据当前网关判断处在哪个档位（委托给 [`Config::mode_of`]，纯函数、可独立测试）。
    pub fn mode_of(&self, gateway: Option<&str>) -> RouterMode {
        self.config.mode_of(gateway)
    }

    /// 切到指定档位：改默认网关，并按配置决定是否同时设置该档位的 DNS。
    /// 返回切换后的最新状态。
    pub fn switch_to(&self, mode: RouterMode) -> Result<NetworkState> {
        let profile = self.config.profile(mode).ok_or(Error::ModeUnspecified)?;
        let interface = self.target_interface()?;
        let dns = if self.config.set_dns_on_switch {
            Some(profile.dns.as_slice())
        } else {
            None
        };
        self.platform
            .apply(&interface, Some(&profile.gateway), dns)?;
        self.detect()
    }

    /// 只对齐 DNS，**不动网关**：先看当前处在哪个档位，再把该档位的 DNS 设上去。
    pub fn align_dns(&self) -> Result<NetworkState> {
        let state = self.detect()?;
        let profile = self.config.profile(state.mode).ok_or_else(|| {
            Error::ModeUnknown(
                state
                    .gateway
                    .clone()
                    .unwrap_or_else(|| "（读不到网关）".to_string()),
            )
        })?;
        self.platform.set_dns(&state.interface, &profile.dns)?;
        self.platform.flush_dns()?;
        self.detect()
    }

    /// 把系统 DNS 恢复为自动（DHCP）。
    pub fn reset_dns_to_auto(&self) -> Result<NetworkState> {
        let interface = self.target_interface()?;
        self.platform.set_dns(&interface, &[])?;
        self.platform.flush_dns()?;
        self.detect()
    }

    /// 记录当前接口与 DNS。
    pub fn snapshot(&self) -> Result<Snapshot> {
        let interface = self.target_interface()?;
        let dns = self.platform.get_dns(&interface)?;
        Ok(Snapshot { interface, dns })
    }

    /// 把 DNS 还原到快照时的状态。接口变了会拒绝执行，避免设错网卡。
    pub fn restore(&self, snapshot: &Snapshot) -> Result<()> {
        let current = self.target_interface()?;
        if current.name != snapshot.interface.name {
            return Err(Error::InterfaceChanged {
                expected: snapshot.interface.name.clone(),
                actual: current.name,
            });
        }
        self.platform.set_dns(&current, &snapshot.dns.ipv4)?;
        self.platform.flush_dns()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testkit::MockPlatform;

    fn manager(gateway: &str, dns: &[&str]) -> NetworkManager {
        NetworkManager::with_platform(Config::default(), Box::new(MockPlatform::seeded(gateway, dns)))
            .unwrap()
    }

    #[test]
    fn detect_matches_mode_by_gateway() {
        let cfg = Config::default();
        let mgr = manager(&cfg.main_router.gateway, &["223.5.5.5"]);
        let state = mgr.detect().unwrap();
        assert_eq!(state.mode, RouterMode::MainRouter);
        assert_eq!(state.interface.name, "Wi-Fi");
        assert_eq!(state.dns.ipv4, vec!["223.5.5.5".to_string()]);
    }

    #[test]
    fn detect_reports_unknown_for_foreign_gateway() {
        let mgr = manager("10.0.0.1", &[]);
        assert_eq!(mgr.detect().unwrap().mode, RouterMode::Unknown);
    }

    #[test]
    fn switch_to_moves_gateway_and_dns() {
        let cfg = Config::default();
        let mgr = manager(&cfg.main_router.gateway, &[]);
        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
        assert_eq!(state.mode, RouterMode::BypassRouter);
    }

    #[test]
    fn switch_to_keeps_dns_when_disabled() {
        let cfg = Config {
            set_dns_on_switch: false,
            ..Config::default()
        };
        let gw = cfg.main_router.gateway.clone();
        let mgr = NetworkManager::with_platform(cfg.clone(), Box::new(MockPlatform::seeded(&gw, &["9.9.9.9"])))
            .unwrap();
        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        assert_eq!(state.dns.ipv4, vec!["9.9.9.9".to_string()]);
    }

    #[test]
    fn switch_to_rejects_unknown_mode() {
        let mgr = manager("192.168.1.1", &[]);
        assert!(matches!(
            mgr.switch_to(RouterMode::Unknown),
            Err(Error::ModeUnspecified)
        ));
    }

    #[test]
    fn align_dns_leaves_gateway_untouched() {
        let cfg = Config::default();
        let mgr = manager(&cfg.bypass_router.gateway, &[]);
        let state = mgr.align_dns().unwrap();
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
    }

    #[test]
    fn align_dns_fails_when_mode_unknown() {
        let mgr = manager("10.0.0.1", &[]);
        assert!(matches!(mgr.align_dns(), Err(Error::ModeUnknown(_))));
    }

    #[test]
    fn reset_dns_to_auto_clears_list() {
        let cfg = Config::default();
        let mgr = manager(&cfg.main_router.gateway, &["223.5.5.5"]);
        let state = mgr.reset_dns_to_auto().unwrap();
        assert!(state.dns.is_auto());
    }

    #[test]
    fn snapshot_then_restore_roundtrip() {
        let cfg = Config::default();
        let mgr = manager(&cfg.main_router.gateway, &["223.5.5.5"]);
        let snap = mgr.snapshot().unwrap();
        assert_eq!(snap.dns.ipv4, vec!["223.5.5.5".to_string()]);

        mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(mgr.detect().unwrap().dns.ipv4, cfg.bypass_router.dns);

        mgr.restore(&snap).unwrap();
        assert_eq!(
            mgr.detect().unwrap().dns.ipv4,
            vec!["223.5.5.5".to_string()]
        );
    }

    #[test]
    fn restore_refuses_when_interface_changed() {
        let mgr = manager("192.168.1.1", &[]);
        let stale = Snapshot {
            interface: NetworkInterface {
                name: "Ethernet".into(),
                index: None,
                device: Some("en0".into()),
                is_default: true,
            },
            dns: DnsConfig::new(vec!["1.1.1.1".into()]),
        };
        assert!(matches!(
            mgr.restore(&stale),
            Err(Error::InterfaceChanged { .. })
        ));
    }

    // ---- 以下用例用 testkit 的 MockPlatform / MockHandle 做「调用序列」与「错误传播」断言 ----

    #[test]
    fn switch_to_records_exact_sequence() {
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        mgr.switch_to(RouterMode::BypassRouter).unwrap();

        // 一次 switch 合并成一次 apply（网关 + DNS + flush 各被记一次）
        assert_eq!(handle.count_calls("apply"), 1);
        assert_eq!(handle.count_calls("set_gateway"), 1);
        assert_eq!(handle.count_calls("set_dns"), 1);
        assert_eq!(handle.count_calls("flush_dns"), 1);
        // 写进去的就是旁路由档位的值
        assert_eq!(
            handle.gateway().as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        assert_eq!(handle.dns(), cfg.bypass_router.dns);
    }

    #[test]
    fn switch_to_propagates_elevation_cancel() {
        let cfg = Config::default();
        let (platform, _handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .fail_apply(Error::ElevationCancelled)
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        assert!(matches!(
            mgr.switch_to(RouterMode::BypassRouter),
            Err(Error::ElevationCancelled)
        ));
    }

    #[test]
    fn reset_dns_records_empty_set_and_flush() {
        let (platform, handle) = MockPlatform::builder()
            .gateway("192.168.1.1")
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        mgr.reset_dns_to_auto().unwrap();
        assert_eq!(handle.count_calls("set_dns"), 1);
        assert_eq!(handle.count_calls("flush_dns"), 1);
        assert!(handle.dns().is_empty());
    }

    #[test]
    fn align_dns_does_not_touch_gateway() {
        let (platform, handle) = MockPlatform::builder()
            .gateway("192.168.1.11")
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        mgr.align_dns().unwrap();
        // 只对齐 DNS，不动网关
        assert_eq!(handle.count_calls("set_gateway"), 0);
        assert_eq!(handle.count_calls("set_dns"), 1);
        assert_eq!(handle.gateway().as_deref(), Some("192.168.1.11"));
    }
}
