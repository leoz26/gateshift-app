//! 配置模型与持久化。
//!
//! 默认格式是 JSON（`.json`），但 `load_from_file` / `save_to_file` 会按扩展名识别，
//! 传 `.toml` 路径也能用。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::RouterMode;
use crate::util::is_ipv4;

/// 一个档位：网关 + 该档位要用的 DNS。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterProfile {
    /// 展示名，可留空。
    #[serde(default)]
    pub name: String,
    pub gateway: String,
    /// 该档位要设置的系统 DNS；空数组表示恢复「自动（DHCP）」。
    #[serde(default)]
    pub dns: Vec<String>,
}

impl RouterProfile {
    pub fn new(name: impl Into<String>, gateway: impl Into<String>, dns: Vec<String>) -> Self {
        RouterProfile {
            name: name.into(),
            gateway: gateway.into(),
            dns,
        }
    }

    pub fn display_name<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.name.is_empty() {
            fallback
        } else {
            &self.name
        }
    }

    /// DNS 的展示文本；空列表 = 自动（DHCP）。
    pub fn dns_text(&self) -> String {
        if self.dns.is_empty() {
            "自动（DHCP）".to_string()
        } else {
            self.dns.join(", ")
        }
    }

    pub fn validate(&self, label: &str) -> Result<()> {
        if !is_ipv4(&self.gateway) {
            return Err(Error::InvalidConfig(format!(
                "{}网关「{}」不是合法的 IPv4 地址",
                label, self.gateway
            )));
        }
        for d in &self.dns {
            if !is_ipv4(d) {
                return Err(Error::InvalidConfig(format!(
                    "{}DNS「{}」不是合法的 IPv4 地址（当前只支持 IPv4）",
                    label, d
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// 切换网关时是否同时设置系统 DNS。
    #[serde(default = "default_true")]
    pub set_dns_on_switch: bool,
    /// 退出软件时是否把网关还原到「主路由」。默认关，需用户主动开启。
    #[serde(default)]
    pub restore_on_exit: bool,
    pub main_router: RouterProfile,
    pub bypass_router: RouterProfile,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            set_dns_on_switch: true,
            restore_on_exit: false,
            main_router: RouterProfile::new(
                "主路由",
                "192.168.1.1",
                vec!["223.5.5.5".into(), "8.8.8.8".into()],
            ),
            bypass_router: RouterProfile::new("旁路由", "192.168.1.11", vec!["192.168.1.11".into()]),
        }
    }
}

impl Config {
    /// 取某个档位的配置；`Unknown` 没有对应档位。
    pub fn profile(&self, mode: RouterMode) -> Option<&RouterProfile> {
        match mode {
            RouterMode::MainRouter => Some(&self.main_router),
            RouterMode::BypassRouter => Some(&self.bypass_router),
            RouterMode::Unknown => None,
        }
    }

    /// 纯函数版：根据网关判断当前档位，**不依赖平台**，便于脱离 `NetworkManager` 单独单测。
    pub fn mode_of(&self, gateway: Option<&str>) -> RouterMode {
        match gateway {
            Some(g) if g == self.main_router.gateway => RouterMode::MainRouter,
            Some(g) if g == self.bypass_router.gateway => RouterMode::BypassRouter,
            _ => RouterMode::Unknown,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.main_router.validate("主路由")?;
        self.bypass_router.validate("旁路由")?;
        if self.main_router.gateway == self.bypass_router.gateway {
            return Err(Error::InvalidConfig(
                "主路由与旁路由的网关不能相同，否则无法判断当前档位".into(),
            ));
        }
        Ok(())
    }

    /// 按扩展名读配置（`.toml` 走 TOML，其余走 JSON），读完顺手校验。
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::ConfigRead(format!("{}: {}", path.display(), e)))?;
        let config = Self::parse(path, &text)?;
        config.validate()?;
        Ok(config)
    }

    fn parse(path: &Path, text: &str) -> Result<Self> {
        let context = |e: String| Error::ConfigParse(format!("{}: {}", path.display(), e));
        if is_toml(path) {
            toml::from_str::<Config>(text).map_err(|e| context(e.to_string()))
        } else {
            serde_json::from_str::<Config>(text).map_err(|e| context(e.to_string()))
        }
    }

    /// 原子写入（先写临时文件再 rename），避免写一半失败把原配置毁掉。
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        self.validate()?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::ConfigWrite(format!("{}: {}", parent.display(), e)))?;
            }
        }

        let text = if is_toml(path) {
            toml::to_string_pretty(self).map_err(|e| Error::ConfigWrite(e.to_string()))?
        } else {
            serde_json::to_string_pretty(self).map_err(|e| Error::ConfigWrite(e.to_string()))?
        };

        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)
            .map_err(|e| Error::ConfigWrite(format!("{}: {}", tmp.display(), e)))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| Error::ConfigWrite(format!("{}: {}", path.display(), e)))?;
        Ok(())
    }

    /// 默认配置路径（仅作便利，不强制）：
    /// macOS `~/Library/Application Support/gateshift/`、Windows `%APPDATA%\gateshift\`。
    pub fn default_config_path() -> Option<PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("gateshift");
        p.push("config.json");
        Some(p)
    }
}

fn is_toml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn rejects_invalid_gateway() {
        let mut c = Config::default();
        c.bypass_router.gateway = "192.168.1".into();
        assert!(matches!(c.validate(), Err(Error::InvalidConfig(_))));
    }

    #[test]
    fn rejects_invalid_dns() {
        let mut c = Config::default();
        c.main_router.dns = vec!["不是IP".into()];
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_identical_gateways() {
        let mut c = Config::default();
        c.bypass_router.gateway = c.main_router.gateway.clone();
        assert!(c.validate().is_err());
    }

    #[test]
    fn json_roundtrip() {
        let c = Config::default();
        let text = serde_json::to_string_pretty(&c).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn toml_is_detected_by_extension() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back = Config::parse(Path::new("config.toml"), &text).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn json_is_default_format() {
        let c = Config::default();
        let text = serde_json::to_string_pretty(&c).unwrap();
        let back = Config::parse(Path::new("config.json"), &text).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn missing_optional_fields_fall_back_to_defaults() {
        let text = r#"{
            "main_router": { "gateway": "10.0.0.1" },
            "bypass_router": { "gateway": "10.0.0.2" }
        }"#;
        let c = Config::parse(Path::new("config.json"), text).unwrap();
        assert!(c.set_dns_on_switch);
        assert!(c.main_router.dns.is_empty());
        c.validate().unwrap();
    }

    #[test]
    fn file_roundtrip_through_disk() {
        let dir = std::env::temp_dir().join("gateshift-core-config-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let c = Config::default();
        c.save_to_file(&path).unwrap();
        let back = Config::load_from_file(&path).unwrap();
        assert_eq!(c, back);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mode_of_is_pure_and_matches_gateways() {
        let c = Config::default();
        assert_eq!(c.mode_of(Some(&c.main_router.gateway)), RouterMode::MainRouter);
        assert_eq!(
            c.mode_of(Some(&c.bypass_router.gateway)),
            RouterMode::BypassRouter
        );
        assert_eq!(c.mode_of(Some("10.0.0.1")), RouterMode::Unknown);
        assert_eq!(c.mode_of(None), RouterMode::Unknown);
    }
}
