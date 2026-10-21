use std::sync::Mutex;

use gateshift_core::{Config, NetworkManager};

/// 应用级单例：持有一个真实平台的 `NetworkManager`。
/// 所有命令都通过它操作，不直接碰平台细节，也方便将来做常驻 / 托盘。
pub struct AppState {
    manager: Mutex<NetworkManager>,
}

impl AppState {
    pub fn new() -> Self {
        let config = load_config();
        let manager = NetworkManager::new(config)
            .expect("无法初始化 NetworkManager（配置校验失败？）");
        AppState {
            manager: Mutex::new(manager),
        }
    }

    /// 拿到底层 manager 的已加锁句柄。
    pub fn lock(&self) -> std::sync::MutexGuard<'_, NetworkManager> {
        self.manager.lock().expect("NetworkManager 锁被毒化")
    }
}

#[cfg(any(test, feature = "testkit"))]
impl AppState {
    /// 测试 / 命令层集成测试专用：直接用给定 manager 构造，绕过真实平台初始化与配置加载。
    ///
    /// 只在带 `testkit` feature 的测试里真的被调用；其它 cfg 组合下它没有调用方，
    /// 所以显式允许 dead_code，避免 `cargo clippy --all-targets` 报假警告。
    #[cfg_attr(not(all(test, feature = "testkit")), allow(dead_code))]
    pub(crate) fn with_manager(manager: NetworkManager) -> Self {
        AppState {
            manager: Mutex::new(manager),
        }
    }
}

/// 优先读默认路径上的配置文件；读不到或读错就回退到内置默认配置。
fn load_config() -> Config {
    if let Some(path) = Config::default_config_path() {
        if path.exists() {
            match Config::load_from_file(&path) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    eprintln!(
                        "[gateshift] 配置文件 {} 读取失败：{e}，回退到内置默认配置",
                        path.display()
                    );
                }
            }
        }
    }
    Config::default()
}
