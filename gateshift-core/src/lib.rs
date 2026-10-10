//! GateShift 核心库 —— 与界面无关的「默认网关切换 + 系统 DNS」逻辑，
//! macOS / Windows 通用，**不依赖任何 GUI / 托盘 / 异步运行时**。
//!
//! # 分层
//!
//! ```text
//! NetworkManager (manager.rs)   ← 唯一入口，界面层只用它
//!        │
//!        ├── Config / RouterProfile (config.rs)   配置 + 校验 + 持久化
//!        ├── 模型 (model.rs)                       serde 化，可直接给前端
//!        └── Box<dyn NetworkPlatform> (platform/)  平台差异全部隔离在这里
//!                 ├── macos.rs
//!                 ├── windows.rs
//!                 └── unsupported.rs
//! ```
//!
//! # 用法
//!
//! ```no_run
//! use gateshift_core::{Config, NetworkManager, RouterMode};
//!
//! # fn main() -> Result<(), gateshift_core::Error> {
//! let config = Config::load_from_file("config.json")?;
//! let manager = NetworkManager::new(config)?;
//!
//! let state = manager.probe()?;                       // 只读，不弹授权
//! println!("{} / {}", state.gateway.as_deref().unwrap_or("-"), state.dns.describe());
//!
//! let before = manager.snapshot()?;                   // 改之前先留个底
//! manager.switch_to(RouterMode::BypassRouter)?;       // 会弹系统授权
//! manager.restore(&before)?;                          // 需要的话还原
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod manager;
pub mod model;
pub mod platform;
pub mod util;

/// 测试工具：可脚本化、可回放的假平台实现（`MockPlatform` / `MockHandle`）。
/// 仅在 `cargo test` 或下游 `features = ["testkit"]` 时编入。
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

pub use config::{Config, RouterProfile};
pub use error::{Error, Result};
pub use manager::{NetworkManager, Snapshot};
pub use model::{DnsConfig, NetworkInterface, NetworkState, RouterMode};
pub use platform::NetworkPlatform;
