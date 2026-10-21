//! Tauri 命令层：每个命令都是对 `NetworkManager` 的薄封装。
//! 错误统一序列化成 [`CommandError`]（`{ code, message }`）：
//! `code` 是稳定的机器可读标识（见 `gateshift_core::Error::code()`），供前端做分支；
//! `message` 是直接给人看的中文文案。
//!
//! 所有命令都是 `async` + `spawn_blocking`：平台操作会卡在系统授权弹窗
//! （UAC / 密码框 / Touch ID）上几十秒，而 IPC 回调跑在 UI 线程，
//! 原地同步执行会让窗口直接假死（转菊花、点不动）。

use gateshift_core::{Config, NetworkInterface, NetworkState, RouterMode, Snapshot};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// IPC 层统一错误载荷：`{ "code": "...", "message": "..." }`。
///
/// 前端**只应该依赖 `code`** 做分支（例如 `elevation_cancelled` → 记一条「已取消，未做修改」
/// 而不是报错）。`message` 是给人看的中文文案，随时可能调整措辞，拿它做字符串匹配
/// （`message.includes("取消")`）会在改文案时悄悄失效。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    /// 机器可读错误码：取值来自 `gateshift_core::Error::code()`，另加本层自有的
    /// `internal`（阻塞线程池 join 失败等命令层自身的问题）。
    pub code: String,
    /// 给人看的错误详情。
    pub message: String,
}

impl CommandError {
    /// 把 core 的错误按「稳定码 + 中文文案」拆开。
    fn from_core(e: gateshift_core::Error) -> Self {
        CommandError {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }

    /// 命令层自身的问题（阻塞线程池 join 失败等），不属于 core 的错误域。
    fn internal(message: impl Into<String>) -> Self {
        CommandError {
            code: "internal".to_string(),
            message: message.into(),
        }
    }
}

/// 在阻塞线程池里跑一段需要 `AppState` 的同步逻辑。
///
/// 注意 `AppState` 内部是 `std::sync::Mutex`：锁**只在闭包内部**获取，
/// 并在闭包返回前释放，不会跨 `.await` 持有 `MutexGuard`
/// （跨 await 持锁既不是 `Send`，也容易跟托盘/退出路径互相卡死）。
async fn blocking<T, F>(app: AppHandle, f: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, gateshift_core::Error> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        f(state.inner()).map_err(CommandError::from_core)
    })
    .await
    .map_err(|e| CommandError::internal(format!("后台任务执行失败：{e}")))?
}

/// 只读探测：网卡 / 网关 / DNS / 档位。不弹授权。
#[tauri::command]
pub async fn probe(app: AppHandle) -> Result<NetworkState, CommandError> {
    blocking(app, |state| state.lock().probe()).await
}

/// 列出可用网络接口（配置里可选指定操作哪一个）。
#[tauri::command]
pub async fn list_interfaces(app: AppHandle) -> Result<Vec<NetworkInterface>, CommandError> {
    blocking(app, |state| state.lock().list_interfaces()).await
}

/// 读取当前生效配置。
#[tauri::command]
pub async fn get_config(app: AppHandle) -> Result<Config, CommandError> {
    blocking(app, |state| Ok::<_, gateshift_core::Error>(state.lock().config().clone())).await
}

/// 保存配置：先写进内存，再落盘到默认路径（若可写）。
#[tauri::command]
pub async fn save_config(app: AppHandle, config: Config) -> Result<(), CommandError> {
    blocking(app, move |state| {
        state.lock().set_config(config.clone())?;
        if let Some(path) = Config::default_config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            config.save_to_file(&path)?;
        }
        Ok(())
    })
    .await
}

/// 切到指定档位（主路由 / 旁路由）。会触发系统授权。
///
/// 保持同步签名：命令层与 `#[cfg(test)]` 的集成测试都直接调它，
/// 阻塞的部分交给上层 `switch_to` 丢进线程池。
pub(crate) fn switch_to_impl(
    state: &AppState,
    mode: RouterMode,
) -> Result<NetworkState, gateshift_core::Error> {
    state.lock().switch_to(mode)
}

#[tauri::command]
pub async fn switch_to(app: AppHandle, mode: RouterMode) -> Result<NetworkState, CommandError> {
    blocking(app, move |state| switch_to_impl(state, mode)).await
}

/// 只对齐 DNS，不动网关。
#[tauri::command]
pub async fn align_dns(app: AppHandle) -> Result<NetworkState, CommandError> {
    blocking(app, |state| state.lock().align_dns()).await
}

/// 把系统 DNS 恢复为自动（DHCP）。
#[tauri::command]
pub async fn reset_dns_to_auto(app: AppHandle) -> Result<NetworkState, CommandError> {
    blocking(app, |state| state.lock().reset_dns_to_auto()).await
}

/// 记录当前接口、网关与 DNS（用于之后还原）。
#[tauri::command]
pub async fn snapshot(app: AppHandle) -> Result<Snapshot, CommandError> {
    blocking(app, |state| state.lock().snapshot()).await
}

/// 把默认网关与 DNS 还原到快照状态。接口变了会被 core 拒绝。
#[tauri::command]
pub async fn restore(app: AppHandle, snapshot: Snapshot) -> Result<(), CommandError> {
    blocking(app, move |state| state.lock().restore(&snapshot)).await
}

/// 纯函数版档位判断（委托给 `Config::mode_of`）。只是一次内存计算，不弹授权。
#[tauri::command]
pub async fn mode_of(app: AppHandle, gateway: Option<String>) -> Result<RouterMode, CommandError> {
    blocking(app, move |state| {
        Ok::<_, gateshift_core::Error>(state.lock().mode_of(gateway.as_deref()))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::CommandError;

    #[test]
    fn error_payload_exposes_code_and_message() {
        // 前端按 code 分支：取消授权必须能只靠码识别出来，不依赖中文文案。
        let err = CommandError::from_core(gateshift_core::Error::ElevationCancelled);
        assert_eq!(err.code, "elevation_cancelled");

        let json = serde_json::to_value(&err).expect("CommandError 必须可序列化");
        assert_eq!(json["code"], "elevation_cancelled");
        assert_eq!(json["message"], "已取消系统授权");
        // 结构固定为 { code, message } 两个字段，前端解析不会踩到意外字段
        let obj = json.as_object().expect("序列化结果应为 JSON 对象");
        assert_eq!(obj.len(), 2, "载荷只应包含 code 与 message：{obj:?}");
    }

    #[test]
    fn every_core_error_maps_to_a_nonempty_code() {
        // 逐条覆盖 core 的错误变体，确保没有哪个分支漏掉错误码
        let cases = [
            gateshift_core::Error::UnsupportedPlatform,
            gateshift_core::Error::NoDefaultInterface,
            gateshift_core::Error::InterfaceNotFound("en0".into()),
            gateshift_core::Error::InterfaceChanged {
                expected: "en0".into(),
                actual: "en1".into(),
            },
            gateshift_core::Error::ModeUnknown("10.0.0.1".into()),
            gateshift_core::Error::ModeUnspecified,
            gateshift_core::Error::DnsUnmanageable("utun3".into()),
            gateshift_core::Error::InvalidConfig("坏配置".into()),
            gateshift_core::Error::InvalidIpv4("192.168.01.1".into()),
            gateshift_core::Error::ConfigRead("读不到".into()),
            gateshift_core::Error::ConfigParse("解析失败".into()),
            gateshift_core::Error::ConfigWrite("写不进".into()),
            gateshift_core::Error::CommandFailed {
                program: "route".into(),
                stderr: "boom".into(),
            },
            gateshift_core::Error::ElevationCancelled,
            gateshift_core::Error::Elevation("osascript 失败".into()),
        ];
        for e in cases {
            let err = CommandError::from_core(e);
            assert!(!err.code.is_empty(), "错误码不能为空：{err:?}");
            assert_ne!(err.code, "internal", "core 的错误不该落到 internal");
            assert!(!err.message.is_empty(), "文案不能为空：{err:?}");
        }
    }

    #[test]
    fn internal_errors_use_internal_code() {
        let err = CommandError::internal("后台任务执行失败：task panicked");
        assert_eq!(err.code, "internal");
        assert!(err.message.contains("后台任务执行失败"));
    }

    // 命令层的集成测试：用 core 的 testkit 把 NetworkManager 换成假平台，
    // 断言「切网关这条 IPC 命令」确实触发了一次 apply。这部分需要 `testkit` feature：
    //   cargo test --features testkit
    #[cfg(feature = "testkit")]
    #[test]
    fn switch_to_command_drives_exactly_one_apply() {
        use gateshift_core::testkit::MockPlatform;
        use gateshift_core::{Config, NetworkManager, RouterMode};

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
