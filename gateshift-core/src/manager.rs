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
///
/// 网关必须一起记：切到旁路由时**网关和 DNS 都变了**，只存 DNS 的还原等于
/// 半个还原（网关还留在旁路由上，整机流量照样走错）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub interface: NetworkInterface,
    /// 快照时的默认网关；读不到时为 `None`（还原时就不动网关）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
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
        // 网关 + DNS 合并成一次平台调用（Windows 一步 PowerShell，macOS 一次 route）
        let probe = self.platform.get_gateway_and_dns(&interface)?;
        let mode = self.mode_of(probe.gateway.as_deref());
        Ok(NetworkState {
            interface,
            gateway: probe.gateway,
            dns: probe.dns,
            mode,
            dns_readable: probe.dns_readable,
        })
    }

    /// 根据当前网关判断处在哪个档位（委托给 [`Config::mode_of`]，纯函数、可独立测试）。
    pub fn mode_of(&self, gateway: Option<&str>) -> RouterMode {
        self.config.mode_of(gateway)
    }

    /// 切到指定档位：改默认网关，并按配置决定是否同时设置该档位的 DNS。
    /// 返回切换后的最新状态。
    ///
    /// **幂等**：先探测一次，若网关与 DNS 都已经是目标值，就直接返回当前状态、
    /// 不动系统 —— 写操作会弹系统授权，重复点两下不该弹两次，切到「已经是的」
    /// 档位更不该弹。
    pub fn switch_to(&self, mode: RouterMode) -> Result<NetworkState> {
        let profile = self.config.profile(mode).ok_or(Error::ModeUnspecified)?;
        let current = self.detect()?;

        let gateway_matches = current.gateway.as_deref() == Some(profile.gateway.as_str());
        // DNS 读不到（VPN 等）时没法比对、也不该去碰它：这一步直接跳过，
        // 只把网关切过去 —— 切网关才是这个功能的主目的，DNS 是顺手带的。
        let want_dns = self.config.set_dns_on_switch && current.dns_readable;
        // 只在真的要写 DNS 时才比较 DNS；`set_dns_on_switch = false` 表示不管 DNS
        let dns_matches = !want_dns || current.dns.ipv4 == profile.dns;
        if gateway_matches && dns_matches {
            return Ok(current);
        }

        // 已经对的就不要再写一遍，减少提权脚本里的步骤
        let gateway = (!gateway_matches).then_some(profile.gateway.as_str());
        let dns = want_dns.then_some(profile.dns.as_slice());
        if self.config.set_dns_on_switch && !current.dns_readable {
            eprintln!(
                "gateshift: 接口「{}」的 DNS 不可读（VPN 等），已跳过 DNS 设置，只切网关",
                current.interface.name
            );
        }
        self.platform.apply(&current.interface, gateway, dns)?;
        self.detect()
    }

    /// 只对齐 DNS，**不动网关**：先看当前处在哪个档位，再把该档位的 DNS 设上去。
    ///
    /// 走**一次** `apply`（`gateway = None`）：macOS / Windows 上 `set_dns` 与 `flush_dns`
    /// 各自都是一次提权，分开调用等于让用户连输两次密码。三个平台的 `apply` 都会在
    /// `gateway = None` 时照常刷新 DNS 缓存，所以合并后语义不变。
    ///
    /// 错误：档位未知 → [`Error::ModeUnknown`]；当前接口的 DNS 不受系统管理
    /// （VPN 等，`dns_readable = false`）→ [`Error::DnsUnmanageable`]。
    pub fn align_dns(&self) -> Result<NetworkState> {
        let state = self.detect()?;
        // 「DNS 不可管理」比「档位未知」更具体、更可操作，先判它：
        // 这个按钮的语义就是「设 DNS」，设不了就得说出来 —— 静默成功会让用户
        // 以为已经对齐了。
        ensure_dns_manageable(&state)?;
        let profile = self.config.profile(state.mode).ok_or_else(|| {
            Error::ModeUnknown(
                state
                    .gateway
                    .clone()
                    .unwrap_or_else(|| "（读不到网关）".to_string()),
            )
        })?;
        self.platform
            .apply(&state.interface, None, Some(profile.dns.as_slice()))?;
        self.detect()
    }

    /// 把系统 DNS 恢复为自动（DHCP）。同样是**一次** `apply`（`gateway = None`），
    /// 只弹一次授权；空 DNS 列表在三个平台都表示「恢复自动」。
    ///
    /// 不看档位（`ModeUnknown` 也照做），但 DNS 读不到时同样报
    /// [`Error::DnsUnmanageable`]：管不了的接口谈不上「恢复自动」。
    pub fn reset_dns_to_auto(&self) -> Result<NetworkState> {
        // 这里不需要知道档位，但需要知道「DNS 读不读得到」：
        // 读不到就说明这个接口的 DNS 不归系统管，恢复自动也就无从谈起。
        let state = self.detect()?;
        ensure_dns_manageable(&state)?;
        self.platform
            .apply(&state.interface, None, Some(&[] as &[String]))?;
        self.detect()
    }

    /// 记录当前接口、网关与 DNS。
    pub fn snapshot(&self) -> Result<Snapshot> {
        let interface = self.target_interface()?;
        // 网关 + DNS 一次读完（平台支持时只 spawn 一次进程）
        let probe = self.platform.get_gateway_and_dns(&interface)?;
        Ok(Snapshot {
            interface,
            gateway: probe.gateway,
            dns: probe.dns,
        })
    }

    /// 把默认网关与 DNS 还原到快照时的状态。接口变了会拒绝执行，避免设错网卡。
    ///
    /// 顺序是**先网关、后 DNS**：只还原 DNS 会留下「网关仍指向旁路由」的半还原
    /// 状态，正好是本工具最该防住的那种情况。两步合成一次 `apply`，
    /// 用户只需要授权一次。快照里没有网关（读不到）时就不动网关。
    pub fn restore(&self, snapshot: &Snapshot) -> Result<()> {
        let current = self.target_interface()?;
        if current.name != snapshot.interface.name {
            return Err(Error::InterfaceChanged {
                expected: snapshot.interface.name.clone(),
                actual: current.name,
            });
        }
        self.platform.apply(
            &current,
            snapshot.gateway.as_deref(),
            Some(snapshot.dns.ipv4.as_slice()),
        )
    }
}

/// DNS 读不到 = 这个接口的 DNS 不归系统管（典型是 VPN 的 `utun*`），
/// 此时「设 DNS」类操作必须明确失败，不能假装成功。
fn ensure_dns_manageable(state: &NetworkState) -> Result<()> {
    if state.dns_readable {
        Ok(())
    } else {
        Err(Error::DnsUnmanageable(state.interface.name.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testkit::{Call, MockPlatform};

    fn manager(gateway: &str, dns: &[&str]) -> NetworkManager {
        NetworkManager::with_platform(Config::default(), Box::new(MockPlatform::seeded(gateway, dns)))
            .unwrap()
    }

    /// `apply` **之前**的调用记录。
    ///
    /// 判「Manager 发起了几次提权」要看这个：`MockPlatform::apply` 内部会复用
    /// `set_dns` / `flush_dns`（真平台上这几步合成在同一次授权里），
    /// 所以 apply 之后的这些记录是「一次授权里做了什么」，不是额外又弹了几次密码。
    fn calls_before_apply(calls: &[Call]) -> &[Call] {
        let at = calls
            .iter()
            .position(|c| c.kind() == "apply")
            .unwrap_or(calls.len());
        &calls[..at]
    }

    /// 取出那一次 `apply` 的 `(gateway, dns)` 参数。
    fn apply_payload(calls: &[Call]) -> (Option<String>, Option<Vec<String>>) {
        calls
            .iter()
            .find_map(|c| match c {
                Call::Apply { gateway, dns, .. } => Some((gateway.clone(), dns.clone())),
                _ => None,
            })
            .expect("应当发生过一次 apply 调用")
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
    fn snapshot_records_gateway_and_dns() {
        let cfg = Config::default();
        let mgr = manager(&cfg.main_router.gateway, &["223.5.5.5"]);
        let snap = mgr.snapshot().unwrap();
        assert_eq!(snap.gateway.as_deref(), Some(cfg.main_router.gateway.as_str()));
        assert_eq!(snap.dns.ipv4, vec!["223.5.5.5".to_string()]);
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
        let restored = mgr.detect().unwrap();
        // 网关与 DNS 都必须回到留底时的状态
        assert_eq!(
            restored.gateway.as_deref(),
            Some(cfg.main_router.gateway.as_str())
        );
        assert_eq!(restored.dns.ipv4, vec!["223.5.5.5".to_string()]);
        assert_eq!(restored.mode, RouterMode::MainRouter);
    }

    #[test]
    fn restore_puts_gateway_back_before_dns() {
        // 回归测试：以前 restore() 只还原 DNS，网关仍指向旁路由（半个还原）
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        let snap = mgr.snapshot().unwrap();
        mgr.switch_to(RouterMode::BypassRouter).unwrap();
        let applies_before = handle.count_calls("apply");

        mgr.restore(&snap).unwrap();
        assert_eq!(handle.gateway().as_deref(), Some(cfg.main_router.gateway.as_str()));
        assert_eq!(handle.dns(), vec!["223.5.5.5".to_string()]);
        // 合并成一次 apply（一次授权），且其中网关写在 DNS 写之前
        assert_eq!(handle.count_calls("apply"), applies_before + 1);
        let calls = handle.calls();
        let apply = calls.iter().rposition(|c| c.kind() == "apply").unwrap();
        let gateway = calls.iter().rposition(|c| c.kind() == "set_gateway").unwrap();
        let dns = calls.iter().rposition(|c| c.kind() == "set_dns").unwrap();
        assert!(apply < gateway && gateway < dns, "调用顺序不对：{calls:?}");
    }

    #[test]
    fn restore_skips_gateway_when_snapshot_has_none() {
        // 老快照 / 读不到网关时不能瞎写网关，只还原 DNS
        let mgr = manager("192.168.1.11", &["223.5.5.5"]);
        let stale = Snapshot {
            interface: mgr.detect().unwrap().interface,
            gateway: None,
            dns: DnsConfig::new(vec!["1.1.1.1".into()]),
        };
        mgr.restore(&stale).unwrap();
        assert_eq!(mgr.detect().unwrap().gateway.as_deref(), Some("192.168.1.11"));
        assert_eq!(mgr.detect().unwrap().dns.ipv4, vec!["1.1.1.1".to_string()]);
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
            gateway: Some("192.168.1.1".into()),
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
        // 注：MockPlatform 的 apply 内部会复用 set_dns / flush_dns，所以这里各记 1 次
        // 是「合成的那一次 apply 里做了什么」；Manager 是否只发起一次授权看下面的用例。
        assert_eq!(handle.count_calls("set_dns"), 1);
        assert_eq!(handle.count_calls("flush_dns"), 1);
        assert!(handle.dns().is_empty());
    }

    #[test]
    fn align_dns_uses_single_apply_call() {
        // 回归测试：以前是 set_dns + flush_dns 两次调用，
        // 在 macOS 上等于两次 `do shell script`，用户要输两次密码
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns(&["9.9.9.9"])
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        let state = mgr.align_dns().unwrap();

        assert_eq!(handle.count_calls("apply"), 1, "只该发起一次提权");
        let calls = handle.calls();
        // apply 之前的记录 = Manager 自己额外发起的调用，必须一次都没有
        let before = calls_before_apply(&calls);
        for kind in ["set_gateway", "set_dns", "flush_dns"] {
            assert!(
                !before.iter().any(|c| c.kind() == kind),
                "apply 之前不该再直接调用 {kind}：{calls:?}"
            );
        }
        // 参数：不动网关，DNS 设成当前档位的值
        let (gateway, dns) = apply_payload(&calls);
        assert_eq!(gateway, None, "align_dns 不能碰网关：{calls:?}");
        assert_eq!(dns.as_deref(), Some(cfg.bypass_router.dns.as_slice()));
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
    }

    #[test]
    fn reset_dns_to_auto_uses_single_apply_call() {
        let (platform, handle) = MockPlatform::builder()
            .gateway("192.168.1.11")
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        let state = mgr.reset_dns_to_auto().unwrap();

        assert_eq!(handle.count_calls("apply"), 1, "只该发起一次提权");
        let calls = handle.calls();
        let before = calls_before_apply(&calls);
        for kind in ["set_gateway", "set_dns", "flush_dns"] {
            assert!(
                !before.iter().any(|c| c.kind() == kind),
                "apply 之前不该再直接调用 {kind}：{calls:?}"
            );
        }
        // 参数：网关不动，DNS 传空切片（= 恢复自动）
        let (gateway, dns) = apply_payload(&calls);
        assert_eq!(gateway, None, "reset_dns_to_auto 不能碰网关：{calls:?}");
        assert_eq!(dns, Some(Vec::<String>::new()), "空列表表示恢复自动");
        assert!(state.dns.is_auto());
        assert_eq!(handle.gateway().as_deref(), Some("192.168.1.11"));
    }

    #[test]
    fn switch_to_writes_dns_when_dns_read_failed_looks_auto() {
        // DNS 读不到时平台会降级成空列表（见 macos::dns_or_auto），
        // 于是这里判为「不一致」并重写一次 DNS —— 这是期望行为：
        // 状态未知时宁可重写，也不要默认它已经对了。网关已匹配则不再碰网关。
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();

        assert_eq!(handle.count_calls("apply"), 1);
        let (gateway, dns) = apply_payload(&handle.calls());
        assert_eq!(gateway, None, "网关已经对了就不该重写");
        assert_eq!(dns.as_deref(), Some(cfg.bypass_router.dns.as_slice()));
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
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

    // ---- VPN / DNS 读不到（dns_readable = false）时的语义分叉 ----

    #[test]
    fn switch_to_still_switches_gateway_when_dns_is_unreadable() {
        // VPN 场景：DNS 读不到也不能耽误切网关 —— 切网关才是主目的。
        // 返回 Ok（不报错），DNS 这一步跳过。
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .dns_unreadable()
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();

        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        assert_eq!(state.mode, RouterMode::BypassRouter);
        assert!(!state.dns_readable);

        // 只切了网关：apply 里 dns = None，也没写 DNS
        let (gateway, dns) = apply_payload(&handle.calls());
        assert_eq!(gateway.as_deref(), Some(cfg.bypass_router.gateway.as_str()));
        assert_eq!(dns, None, "DNS 读不到时不该写 DNS");
        assert_eq!(handle.count_calls("apply"), 1);
        assert_eq!(handle.count_calls("set_dns"), 0);
        assert_eq!(handle.gateway().as_deref(), Some(cfg.bypass_router.gateway.as_str()));
    }

    #[test]
    fn switch_to_is_a_no_op_when_gateway_matches_and_dns_is_unreadable() {
        // 网关已经是目标值 + DNS 读不到：无事可做，不该弹授权
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns_unreadable()
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();

        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(state.mode, RouterMode::BypassRouter);
        assert_eq!(handle.count_calls("apply"), 0);
        assert_eq!(handle.count_calls("set_gateway"), 0);
        assert_eq!(handle.count_calls("set_dns"), 0);
    }

    #[test]
    fn align_dns_fails_when_dns_is_unreadable() {
        // 「对齐 DNS」的语义就是设 DNS，设不了必须明确报错，不能静默成功
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns_unreadable()
            .build();
        let mgr = NetworkManager::with_platform(cfg, platform).unwrap();

        assert!(matches!(
            mgr.align_dns(),
            Err(Error::DnsUnmanageable(name)) if name == "Wi-Fi"
        ));
        assert_eq!(handle.count_calls("apply"), 0, "报错前不该动系统");
        assert_eq!(handle.count_calls("set_dns"), 0);
    }

    #[test]
    fn align_dns_reports_dns_unmanageable_before_mode_unknown() {
        // VPN 下这两个条件通常同时成立（网关是 10.x + DNS 读不到）。
        // 报「DNS 不受系统管理」比报「档位未知」更具体、更能指导用户行动。
        let (platform, _h) = MockPlatform::builder()
            .gateway("10.8.0.1")
            .dns_unreadable()
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        assert!(matches!(
            mgr.align_dns(),
            Err(Error::DnsUnmanageable(_))
        ));
    }

    #[test]
    fn reset_dns_to_auto_fails_when_dns_is_unreadable() {
        let (platform, handle) = MockPlatform::builder()
            .gateway("192.168.1.1")
            .dns_unreadable()
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();

        assert!(matches!(
            mgr.reset_dns_to_auto(),
            Err(Error::DnsUnmanageable(_))
        ));
        assert_eq!(handle.count_calls("apply"), 0, "报错前不该动系统");
    }

    #[test]
    fn dns_operations_still_work_when_dns_is_readable() {
        // 回归：DNS 读得到时三个方法行为不变（尤其 align_dns 仍是一次 apply）
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns(&["9.9.9.9"])
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();

        let state = mgr.align_dns().unwrap();
        assert!(state.dns_readable);
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
        assert_eq!(handle.count_calls("apply"), 1);
        assert_eq!(apply_payload(&handle.calls()).1.as_deref(), Some(cfg.bypass_router.dns.as_slice()));
    }

    #[test]
    fn switch_to_is_a_no_op_when_already_in_target_mode() {
        // 回归测试：切到「已经是的」档位不该再弹一次系统授权
        let cfg = Config::default();
        let dns: Vec<&str> = cfg.bypass_router.dns.iter().map(String::as_str).collect();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns(&dns)
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();

        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(state.mode, RouterMode::BypassRouter);
        assert_eq!(
            state.gateway.as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
        // 一次写操作都没发生
        assert_eq!(handle.count_calls("apply"), 0);
        assert_eq!(handle.count_calls("set_gateway"), 0);
        assert_eq!(handle.count_calls("set_dns"), 0);
        assert_eq!(handle.count_calls("flush_dns"), 0);
    }

    #[test]
    fn switch_to_is_idempotent_across_two_calls() {
        // 连点两下：第二次不应再改系统
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .dns(&cfg.main_router.dns.iter().map(String::as_str).collect::<Vec<_>>())
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(handle.count_calls("apply"), 1);
        mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(handle.count_calls("apply"), 1, "第二次切换不该再动系统");
    }

    #[test]
    fn switch_to_only_writes_dns_when_gateway_already_matches() {
        // 网关对了、DNS 还是旧值：只补 DNS，不再碰网关
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.bypass_router.gateway)
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        let state = mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(handle.count_calls("apply"), 1);
        assert_eq!(handle.count_calls("set_gateway"), 0);
        assert_eq!(handle.count_calls("set_dns"), 1);
        assert_eq!(state.dns.ipv4, cfg.bypass_router.dns);
    }

    #[test]
    fn switch_to_writes_gateway_even_when_dns_already_matches() {
        // DNS 已经对、网关不对：仍然要改网关（不能因为 DNS 相同就整体跳过）
        let cfg = Config::default();
        let (platform, handle) = MockPlatform::builder()
            .gateway(&cfg.main_router.gateway)
            .dns(&cfg.bypass_router.dns.iter().map(String::as_str).collect::<Vec<_>>())
            .build();
        let mgr = NetworkManager::with_platform(cfg.clone(), platform).unwrap();
        mgr.switch_to(RouterMode::BypassRouter).unwrap();
        assert_eq!(handle.count_calls("apply"), 1);
        assert_eq!(handle.count_calls("set_gateway"), 1);
        assert_eq!(
            handle.gateway().as_deref(),
            Some(cfg.bypass_router.gateway.as_str())
        );
    }
}
