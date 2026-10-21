//! 测试工具：一个可脚本化、可回放的假平台实现。
//!
//! 业务测逻辑时不需要真实系统：用 [`MockPlatform`] 注入 [`NetworkManager`]，
//! 再用 [`MockHandle`] 检查「到底调了哪些操作、传了什么参数、系统状态变成了什么样」。
//!
//! 同时支持「脚本化错误」：可以指定某个操作返回特定 [`Error`]，用来覆盖
//! 授权取消、接口找不到等平时很难构造的异常路径。
//!
//! 该模块默认**不**编进发布产物——只在 `cargo test` 或下游 `features = ["testkit"]`
//! 时才出现，避免给线上库增加体积。

use std::sync::{Arc, Mutex};

use crate::error::Result;
use crate::model::{DnsConfig, GatewayAndDns, NetworkInterface};
use crate::platform::NetworkPlatform;

/// 一次平台操作的记录，用于断言。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    ListInterfaces,
    DefaultInterface,
    GetGateway { interface: String },
    SetGateway { interface: String, gateway: String },
    GetDns { interface: String },
    SetDns { interface: String, dns: Vec<String> },
    FlushDns,
    Apply {
        interface: String,
        gateway: Option<String>,
        dns: Option<Vec<String>>,
    },
}

impl Call {
    /// 只看「操作类型」、忽略参数的分类标签，便于按种类计数。
    pub fn kind(&self) -> &'static str {
        match self {
            Call::ListInterfaces => "list_interfaces",
            Call::DefaultInterface => "default_interface",
            Call::GetGateway { .. } => "get_gateway",
            Call::SetGateway { .. } => "set_gateway",
            Call::GetDns { .. } => "get_dns",
            Call::SetDns { .. } => "set_dns",
            Call::FlushDns => "flush_dns",
            Call::Apply { .. } => "apply",
        }
    }
}

/// 假平台的内部状态。读 / 写都被记录到 `calls`。
pub struct MockState {
    pub interface: NetworkInterface,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    /// 模拟「这个接口的 DNS 读不到」（VPN / `utun*` 场景）：探测时降级成空列表，
    /// `NetworkState::dns_readable = false`。默认 `true`。
    pub dns_readable: bool,
    pub calls: Vec<Call>,
    // 脚本化错误：返回一次后清空（one-shot）。`None` 表示正常执行。
    pub err_on_list: Option<crate::error::Error>,
    pub err_on_default: Option<crate::error::Error>,
    pub err_on_get_gateway: Option<crate::error::Error>,
    pub err_on_set_gateway: Option<crate::error::Error>,
    pub err_on_get_dns: Option<crate::error::Error>,
    pub err_on_set_dns: Option<crate::error::Error>,
    pub err_on_flush: Option<crate::error::Error>,
    pub err_on_apply: Option<crate::error::Error>,
}

impl Default for MockState {
    fn default() -> Self {
        MockState {
            interface: NetworkInterface {
                name: "Wi-Fi".into(),
                index: Some(7),
                device: None,
                is_default: true,
            },
            gateway: None,
            dns: Vec::new(),
            dns_readable: true,
            calls: Vec::new(),
            err_on_list: None,
            err_on_default: None,
            err_on_get_gateway: None,
            err_on_set_gateway: None,
            err_on_get_dns: None,
            err_on_set_dns: None,
            err_on_flush: None,
            err_on_apply: None,
        }
    }
}

/// 可脚本化、可回放的假平台。
pub struct MockPlatform {
    state: Arc<Mutex<MockState>>,
}

impl MockPlatform {
    /// 默认构造：一个叫 `Wi-Fi`、标记为默认接口的网卡，网关 / DNS 均为空。
    pub fn new() -> Self {
        MockPlatform {
            state: Arc::new(Mutex::new(MockState {
                interface: NetworkInterface {
                    name: "Wi-Fi".into(),
                    index: Some(7),
                    device: None,
                    is_default: true,
                },
                ..Default::default()
            })),
        }
    }

    /// 便捷构造：给定初始网关和 DNS（行为等同旧的内部 `FakePlatform`）。
    pub fn seeded(gateway: &str, dns: &[&str]) -> Self {
        let p = MockPlatform::new();
        {
            let mut st = p.state.lock().unwrap();
            st.gateway = Some(gateway.to_string());
            st.dns = dns.iter().map(|s| s.to_string()).collect();
        }
        p
    }

    /// 生成一个「平台 + 句柄」对，句柄用于事后检查。
    pub fn pair() -> (Box<dyn NetworkPlatform>, MockHandle) {
        let p = MockPlatform::new();
        let h = MockHandle { state: p.state.clone() };
        (Box::new(p), h)
    }

    /// 取出句柄（用于检查调用记录 / 当前状态）。
    pub fn handle(&self) -> MockHandle {
        MockHandle { state: self.state.clone() }
    }

    /// 用构造器脚本化初始状态与错误。
    pub fn builder() -> MockBuilder {
        MockBuilder::default()
    }
}

impl Default for MockPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPlatform for MockPlatform {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_list.take() {
            return Err(e);
        }
        st.calls.push(Call::ListInterfaces);
        Ok(vec![st.interface.clone()])
    }

    fn default_interface(&self) -> Result<NetworkInterface> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_default.take() {
            return Err(e);
        }
        st.calls.push(Call::DefaultInterface);
        Ok(st.interface.clone())
    }

    fn get_gateway(&self, interface: &NetworkInterface) -> Result<Option<String>> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_get_gateway.take() {
            return Err(e);
        }
        st.calls.push(Call::GetGateway {
            interface: interface.name.clone(),
        });
        Ok(st.gateway.clone())
    }

    fn set_gateway(&self, interface: &NetworkInterface, gateway: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_set_gateway.take() {
            return Err(e);
        }
        st.calls.push(Call::SetGateway {
            interface: interface.name.clone(),
            gateway: gateway.to_string(),
        });
        st.gateway = Some(gateway.to_string());
        Ok(())
    }

    fn get_dns(&self, interface: &NetworkInterface) -> Result<DnsConfig> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_get_dns.take() {
            return Err(e);
        }
        st.calls.push(Call::GetDns {
            interface: interface.name.clone(),
        });
        Ok(DnsConfig::new(st.dns.clone()))
    }

    /// 默认实现会分别调用 `get_gateway` / `get_dns`；这里覆写只为带上 `dns_readable` 标记
    /// （模拟 VPN 接口「DNS 读不到」）。调用记录与默认实现完全一致。
    fn get_gateway_and_dns(&self, interface: &NetworkInterface) -> Result<GatewayAndDns> {
        let gateway = self.get_gateway(interface)?;
        let dns = self.get_dns(interface)?;
        let readable = self.state.lock().unwrap().dns_readable;
        Ok(if readable {
            GatewayAndDns::read(gateway, dns)
        } else {
            GatewayAndDns::dns_unreadable(gateway)
        })
    }

    fn set_dns(&self, interface: &NetworkInterface, dns: &[String]) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_set_dns.take() {
            return Err(e);
        }
        st.calls.push(Call::SetDns {
            interface: interface.name.clone(),
            dns: dns.to_vec(),
        });
        st.dns = dns.to_vec();
        Ok(())
    }

    fn flush_dns(&self) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        if let Some(e) = st.err_on_flush.take() {
            return Err(e);
        }
        st.calls.push(Call::FlushDns);
        Ok(())
    }

    fn apply(
        &self,
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
    ) -> Result<()> {
        // 先记录一次合并调用；脚本化错误也在这一层拦截。
        {
            let mut st = self.state.lock().unwrap();
            if let Some(e) = st.err_on_apply.take() {
                return Err(e);
            }
            st.calls.push(Call::Apply {
                interface: interface.name.clone(),
                gateway: gateway.map(|s| s.to_string()),
                dns: dns.map(|d| d.to_vec()),
            });
        }
        // 复用默认行为：分别写网关 / DNS 再刷新（这些调用也会被记录）。
        if let Some(g) = gateway {
            self.set_gateway(interface, g)?;
        }
        if let Some(d) = dns {
            self.set_dns(interface, d)?;
        }
        self.flush_dns()
    }
}

/// 事后检查句柄：跟 [`MockPlatform`] 共享同一份状态。
#[derive(Clone)]
pub struct MockHandle {
    state: Arc<Mutex<MockState>>,
}

impl MockHandle {
    /// 当前接口名。
    pub fn interface_name(&self) -> String {
        self.state.lock().unwrap().interface.name.clone()
    }

    /// 当前网关。
    pub fn gateway(&self) -> Option<String> {
        self.state.lock().unwrap().gateway.clone()
    }

    /// 当前 DNS（IPv4 列表）。
    pub fn dns(&self) -> Vec<String> {
        self.state.lock().unwrap().dns.clone()
    }

    /// 模拟的「DNS 是否可读」标记。
    pub fn dns_readable(&self) -> bool {
        self.state.lock().unwrap().dns_readable
    }

    /// 全部调用记录（按时间顺序）。
    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    /// 是否调用过某个种类的操作（只看类型，忽略参数）。
    pub fn was_called(&self, kind: &str) -> bool {
        self.calls().iter().any(|c| c.kind() == kind)
    }

    /// 某个种类的操作被调用了多少次（只看类型，忽略参数）。
    pub fn count_calls(&self, kind: &str) -> usize {
        self.calls().iter().filter(|c| c.kind() == kind).count()
    }
}

/// 构造器：脚本化初始状态与一次性错误。
pub struct MockBuilder {
    state: MockState,
}

impl Default for MockBuilder {
    fn default() -> Self {
        MockBuilder {
            state: MockState {
                interface: NetworkInterface {
                    name: "Wi-Fi".into(),
                    index: Some(7),
                    device: None,
                    is_default: true,
                },
                ..Default::default()
            },
        }
    }
}

impl MockBuilder {
    pub fn gateway(mut self, gateway: &str) -> Self {
        self.state.gateway = Some(gateway.to_string());
        self
    }

    pub fn dns(mut self, dns: &[&str]) -> Self {
        self.state.dns = dns.iter().map(|s| s.to_string()).collect();
        self
    }

    /// 模拟「这个接口的 DNS 读不到」（VPN / `utun*`）：探测返回空 DNS + `dns_readable = false`。
    pub fn dns_unreadable(mut self) -> Self {
        self.state.dns_readable = false;
        self.state.dns.clear();
        self
    }

    pub fn interface_name(mut self, name: &str) -> Self {
        self.state.interface.name = name.to_string();
        self
    }

    pub fn fail_list(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_list = Some(e);
        self
    }

    pub fn fail_default(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_default = Some(e);
        self
    }

    pub fn fail_get_gateway(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_get_gateway = Some(e);
        self
    }

    pub fn fail_set_gateway(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_set_gateway = Some(e);
        self
    }

    pub fn fail_get_dns(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_get_dns = Some(e);
        self
    }

    pub fn fail_set_dns(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_set_dns = Some(e);
        self
    }

    pub fn fail_flush(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_flush = Some(e);
        self
    }

    pub fn fail_apply(mut self, e: crate::error::Error) -> Self {
        self.state.err_on_apply = Some(e);
        self
    }

    /// 生成「平台 + 句柄」对。
    pub fn build(self) -> (Box<dyn NetworkPlatform>, MockHandle) {
        let p = MockPlatform {
            state: Arc::new(Mutex::new(self.state)),
        };
        let h = MockHandle { state: p.state.clone() };
        (Box::new(p), h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn seeded_platform_reports_initial_state() {
        let p = MockPlatform::seeded("192.168.1.1", &["223.5.5.5"]);
        let h = p.handle();
        let mgr = crate::NetworkManager::with_platform(
            crate::Config::default(),
            Box::new(p),
        )
        .unwrap();
        let state = mgr.detect().unwrap();
        assert_eq!(state.gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(state.dns.ipv4, vec!["223.5.5.5".to_string()]);
        // 只读探测不应触发任何写操作
        assert!(!h.was_called("set_gateway"));
        assert!(!h.was_called("apply"));
    }

    #[test]
    fn scripted_apply_error_propagates() {
        let (platform, _h) = MockPlatform::builder()
            .gateway("192.168.1.1")
            .fail_apply(Error::ElevationCancelled)
            .build();
        let mgr =
            crate::NetworkManager::with_platform(crate::Config::default(), platform).unwrap();
        assert!(matches!(
            mgr.switch_to(crate::RouterMode::BypassRouter),
            Err(Error::ElevationCancelled)
        ));
    }
}
