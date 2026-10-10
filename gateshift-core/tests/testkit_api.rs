#![cfg(feature = "testkit")]

//! 演示「下游消费者」（如未来的 Tauri 应用）如何用 testkit 做集成测试：
//! 注入假平台、触发业务动作、再断言网络操作被正确调用。

use gateshift_core::testkit::MockPlatform;
use gateshift_core::{Config, NetworkManager, RouterMode};

#[test]
fn downstream_can_assert_on_recorded_calls() {
    let (platform, handle) = MockPlatform::builder()
        .gateway("192.168.1.1")
        .dns(&["223.5.5.5"])
        .build();
    let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();

    mgr.switch_to(RouterMode::BypassRouter).unwrap();

    assert!(handle.was_called("apply"));
    assert_eq!(handle.gateway().as_deref(), Some("192.168.1.11"));
    assert_eq!(handle.dns(), vec!["192.168.1.11".to_string()]);
}
