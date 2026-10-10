//! macOS 平台实现。
//!
//! 关键点：`networksetup` 认的是**网络服务名**（如 `Wi-Fi`），不是 BSD 设备名（`en0`）。
//! 两者靠 `networksetup -listnetworkserviceorder` 配对，它的输出是**两行一组**：
//!
//! ```text
//! (1) Wi-Fi
//! (Hardware Port: Wi-Fi, Device: en0)
//! ```
//!
//! 上一版实现误以为这是一行，`lines()` 逐行扫就会命中第二行，于是把整条
//! `Hardware Port: ...` 当成服务名喂给 `networksetup`，直接报
//! "is not a recognized network service"。这里按两行正确配对。

use std::process::Command;

use super::NetworkPlatform;
use crate::error::{stderr_or_placeholder, Error, Result};
use crate::model::{DnsConfig, NetworkInterface};
use crate::util::is_ipv4;

pub struct MacOsPlatform;

impl MacOsPlatform {
    /// 执行一个**只读**命令并返回 stdout。
    fn read(program: &str, args: &[&str]) -> Result<String> {
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                program: program.to_string(),
                source: e,
            })?;
        if !out.status.success() {
            return Err(Error::CommandFailed {
                program: program.to_string(),
                stderr: stderr_or_placeholder(&out.stderr),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn services() -> Result<Vec<(String, Option<String>)>> {
        Ok(parse_services(&Self::read(
            "networksetup",
            &["-listnetworkserviceorder"],
        )?))
    }

    /// 把若干条 shell 命令合并成**一次**提权执行，只弹一次密码 / Touch ID。
    fn elevated(commands: &[String]) -> Result<()> {
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            commands.join("; ").replace('\\', "\\\\").replace('"', "\\\"")
        );
        let out = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| Error::Command {
                program: "osascript".to_string(),
                source: e,
            })?;
        if out.status.success() {
            return Ok(());
        }
        let err = stderr_or_placeholder(&out.stderr);
        // 用户点「取消」时 osascript 报 User canceled. (-128)
        if err.contains("-128") || err.contains("User canceled") || err.contains("已取消") {
            Err(Error::ElevationCancelled)
        } else {
            Err(Error::Elevation(err))
        }
    }

    fn route_command(gateway: &str) -> Result<String> {
        if !is_ipv4(gateway) {
            return Err(Error::InvalidIpv4(gateway.to_string()));
        }
        // change 优先：不必先删路由，避免出现「短暂无默认路由」的窗口；
        // 路由不存在时再退回 delete + add。
        Ok(format!(
            "route -n change default {gw} 2>/dev/null || {{ route -n delete default 2>/dev/null; route -n add default {gw}; }}",
            gw = gateway
        ))
    }

    fn dns_command(interface: &NetworkInterface, dns: &[String]) -> Result<String> {
        let arg = if dns.is_empty() {
            // networksetup 用关键字 Empty 表示恢复 DHCP 自动
            "Empty".to_string()
        } else {
            for d in dns {
                if !is_ipv4(d) {
                    return Err(Error::InvalidIpv4(d.clone()));
                }
            }
            dns.join(" ")
        };
        Ok(format!(
            "networksetup -setdnsservers \"{}\" {}",
            interface.name, arg
        ))
    }

    fn flush_commands() -> Vec<String> {
        vec![
            "dscacheutil -flushcache".to_string(),
            // killall 需要 root，所以它跟其它命令合并进同一次提权里跑
            "/usr/bin/killall -HUP mDNSResponder 2>/dev/null || true".to_string(),
        ]
    }
}

/// 解析 `networksetup -listnetworkserviceorder` → `[(服务名, 设备名)]`。
fn parse_services(output: &str) -> Vec<(String, Option<String>)> {
    let mut result = Vec::new();
    let mut pending: Option<String> = None;

    for raw in output.lines() {
        let Some(rest) = raw.trim().strip_prefix('(') else {
            continue;
        };
        let Some((head, tail)) = rest.split_once(')') else {
            continue;
        };
        let head = head.trim();

        if head == "*" || head.chars().all(|c| c.is_ascii_digit()) {
            // "(1) Wi-Fi" / "(*) Bluetooth PAN"：服务名在右括号后面
            pending = Some(tail.trim().to_string());
        } else if let Some(service) = pending.take() {
            // "(Hardware Port: Wi-Fi, Device: en0)"：给上一个服务补设备名
            let device = head
                .split_once("Device:")
                .map(|(_, d)| d.trim().to_string())
                .filter(|d| !d.is_empty());
            result.push((service, device));
        }
    }
    result
}

impl NetworkPlatform for MacOsPlatform {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        let default_name = self.default_interface().ok().map(|d| d.name);
        Ok(Self::services()?
            .into_iter()
            .map(|(name, device)| NetworkInterface {
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                index: None,
                device,
            })
            .collect())
    }

    fn default_interface(&self) -> Result<NetworkInterface> {
        let out = Self::read("route", &["-n", "get", "default"])?;
        let device = out
            .lines()
            .find_map(|l| l.trim().strip_prefix("interface:"))
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .ok_or(Error::NoDefaultInterface)?
            .to_string();

        // 反查设备名对应的网络服务名；VPN 之类的虚拟接口没有对应服务，退回用设备名
        let service = Self::services()?
            .into_iter()
            .find(|(_, dev)| dev.as_deref() == Some(device.as_str()))
            .map(|(service, _)| service)
            .unwrap_or_else(|| device.clone());

        Ok(NetworkInterface {
            name: service,
            index: None,
            device: Some(device),
            is_default: true,
        })
    }

    fn get_gateway(&self, _interface: &NetworkInterface) -> Result<Option<String>> {
        let out = Self::read("route", &["-n", "get", "default"])?;
        Ok(out
            .lines()
            .find_map(|l| l.trim().strip_prefix("gateway:"))
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string))
    }

    fn set_gateway(&self, _interface: &NetworkInterface, gateway: &str) -> Result<()> {
        Self::elevated(&[Self::route_command(gateway)?])
    }

    fn get_dns(&self, interface: &NetworkInterface) -> Result<DnsConfig> {
        let out = Self::read("networksetup", &["-getdnsservers", &interface.name])?;
        Ok(DnsConfig::new(
            out.lines()
                .map(str::trim)
                .filter(|l| is_ipv4(l))
                .map(str::to_string)
                .collect(),
        ))
    }

    fn set_dns(&self, interface: &NetworkInterface, dns: &[String]) -> Result<()> {
        Self::elevated(&[Self::dns_command(interface, dns)?])
    }

    fn flush_dns(&self) -> Result<()> {
        Self::elevated(&Self::flush_commands())
    }

    fn apply(
        &self,
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
    ) -> Result<()> {
        // 合并成一次提权，用户只需要授权一次
        let mut commands = Vec::new();
        if let Some(gateway) = gateway {
            commands.push(Self::route_command(gateway)?);
        }
        if let Some(dns) = dns {
            commands.push(Self::dns_command(interface, dns)?);
        }
        commands.extend(Self::flush_commands());
        Self::elevated(&commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
An asterisk (*) denotes that a network service is disabled.
(1) Ethernet
(Hardware Port: Ethernet, Device: en0)

(2) Wi-Fi
(Hardware Port: Wi-Fi, Device: en1)

(*) Bluetooth PAN
(Hardware Port: Bluetooth PAN, Device: en5)

(4) Tailscale
(Hardware Port: io.tailscale.ipn.macsys, Device: )
";

    fn interface(name: &str) -> NetworkInterface {
        NetworkInterface {
            name: name.to_string(),
            index: None,
            device: None,
            is_default: true,
        }
    }

    #[test]
    fn pairs_service_with_device() {
        assert_eq!(
            parse_services(SAMPLE),
            vec![
                ("Ethernet".to_string(), Some("en0".to_string())),
                ("Wi-Fi".to_string(), Some("en1".to_string())),
                ("Bluetooth PAN".to_string(), Some("en5".to_string())),
                ("Tailscale".to_string(), None),
            ]
        );
    }

    #[test]
    fn service_name_never_contains_hardware_port() {
        // 回归测试：上一版会把 "(Hardware Port: ...)" 整条当成服务名
        for (name, _) in parse_services(SAMPLE) {
            assert!(
                !name.contains("Hardware Port") && !name.contains('('),
                "服务名解析错了：{}",
                name
            );
        }
    }

    #[test]
    fn route_command_rejects_bad_ipv4() {
        assert!(matches!(
            MacOsPlatform::route_command("nope"),
            Err(Error::InvalidIpv4(_))
        ));
        assert!(MacOsPlatform::route_command("192.168.1").is_err());
        assert!(MacOsPlatform::route_command("192.168.1.1").is_ok());
    }

    #[test]
    fn dns_command_uses_empty_keyword_for_auto() {
        let cmd = MacOsPlatform::dns_command(&interface("Wi-Fi"), &[]).unwrap();
        assert!(cmd.contains("Empty"), "{}", cmd);

        let cmd =
            MacOsPlatform::dns_command(&interface("Wi-Fi"), &["223.5.5.5".to_string()]).unwrap();
        assert!(cmd.contains("223.5.5.5"), "{}", cmd);

        assert!(MacOsPlatform::dns_command(&interface("Wi-Fi"), &["bad".to_string()]).is_err());
    }
}
