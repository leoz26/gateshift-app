//! Tauri 命令层：每个命令都是对 `NetworkManager` 的薄封装。
//! 错误统一 `.map_err(|e| e.to_string())` 变成前端能直接展示的字符串；
//! 模型本身都是 `serde` 化的，可直接返回给前端。

use gateshift_core::{
    Config, NetworkInterface, NetworkState, RouterMode, Snapshot,
};
use tauri::State;

use crate::state::AppState;

fn as_string(e: gateshift_core::Error) -> String {
    e.to_string()
}

/// 只读探测：网卡 / 网关 / DNS / 档位。不弹授权。
#[tauri::command]
pub fn probe(state: State<AppState>) -> Result<NetworkState, String> {
    state.lock().probe().map_err(as_string)
}

/// 列出可用网络接口（配置里可选指定操作哪一个）。
#[tauri::command]
pub fn list_interfaces(state: State<AppState>) -> Result<Vec<NetworkInterface>, String> {
    state.lock().list_interfaces().map_err(as_string)
}

/// 读取当前生效配置。
#[tauri::command]
pub fn get_config(state: State<AppState>) -> Config {
    state.lock().config().clone()
}

/// 保存配置：先写进内存，再落盘到默认路径（若可写）。
#[tauri::command]
pub fn save_config(state: State<AppState>, config: Config) -> Result<(), String> {
    state.lock().set_config(config.clone()).map_err(as_string)?;
    if let Some(path) = Config::default_config_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        config.save_to_file(&path).map_err(as_string)?;
    }
    Ok(())
}

/// 切到指定档位（主路由 / 旁路由）。会触发系统授权。
pub(crate) fn switch_to_impl(
    state: &AppState,
    mode: RouterMode,
) -> Result<NetworkState, gateshift_core::Error> {
    state.lock().switch_to(mode)
}

#[tauri::command]
pub fn switch_to(state: State<AppState>, mode: RouterMode) -> Result<NetworkState, String> {
    switch_to_impl(&state, mode).map_err(as_string)
}

/// 只对齐 DNS，不动网关。
#[tauri::command]
pub fn align_dns(state: State<AppState>) -> Result<NetworkState, String> {
    state.lock().align_dns().map_err(as_string)
}

/// 把系统 DNS 恢复为自动（DHCP）。
#[tauri::command]
pub fn reset_dns_to_auto(state: State<AppState>) -> Result<NetworkState, String> {
    state.lock().reset_dns_to_auto().map_err(as_string)
}

/// 记录当前接口与 DNS（用于之后还原）。
#[tauri::command]
pub fn snapshot(state: State<AppState>) -> Result<Snapshot, String> {
    state.lock().snapshot().map_err(as_string)
}

/// 把 DNS 还原到快照状态。接口变了会被 core 拒绝。
#[tauri::command]
pub fn restore(state: State<AppState>, snapshot: Snapshot) -> Result<(), String> {
    state.lock().restore(&snapshot).map_err(as_string)
}

/// 纯函数版档位判断（委托给 `Config::mode_of`）。
#[tauri::command]
pub fn mode_of(state: State<AppState>, gateway: Option<String>) -> RouterMode {
    state.lock().mode_of(gateway.as_deref())
}

#[cfg(test)]
mod tests {
    // 命令层的集成测试：用 core 的 testkit 把 NetworkManager 换成假平台，
    // 断言「切网关这条 IPC 命令」确实触发了一次 apply。这部分需要 `testkit` feature：
    //   cargo test --features testkit
    #[cfg(feature = "testkit")]
    #[test]
    fn switch_to_command_drives_exactly_one_apply() {
        use gateshift_core::testkit::MockPlatform;
        use gateshift_core::{Config, NetworkManager, RouterMode};
        use std::sync::Mutex;

        use crate::state::AppState;

        // 直接构造一个带假平台的 AppState，绕过真实平台初始化。
        let (platform, handle) = MockPlatform::builder()
            .gateway(&Config::default().main_router.gateway)
            .dns(&["223.5.5.5"])
            .build();
        let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
        let state = AppState::with_manager(mgr);

        let result = super::switch_to_impl(&state, RouterMode::BypassRouter);
        assert!(result.is_ok());
        assert_eq!(handle.count_calls("apply"), 1);
    }
}
