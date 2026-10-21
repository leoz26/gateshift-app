//! Windows 平台实现 —— 只负责「把脚本丢给 PowerShell / 用 UAC 提权」。
//! 脚本文本与字符串逻辑在 `windows_scripts`（那个模块跨平台编译，可以跑测试）。

use std::process::Command;
// Windows 子系统程序（Tauri 是 GUI）spawn 控制台子进程时，默认会临时分配一个黑窗口；
// 加 CREATE_NO_WINDOW 让 powershell.exe 在后台静默跑，不再抢出命令行窗口。
// windows.rs 只在 Windows 下编译，这里直接 use 不会有跨平台问题。
use std::os::windows::process::CommandExt;

use super::windows_scripts as scripts;
use super::{powershell, NetworkPlatform};
use crate::error::{stderr_or_placeholder, Error, Result};
use crate::model::{DnsConfig, GatewayAndDns, NetworkInterface};
use crate::util::is_ipv4;

pub struct WindowsPlatform;

impl WindowsPlatform {
    /// 普通权限执行 PowerShell 脚本，返回 stdout。
    ///
    /// 一律走 `-EncodedCommand`（UTF-16LE + Base64）：中文网卡名、引号、反斜杠全都不用转义；
    /// 并在脚本里把输出编码强制成 UTF-8，避免中文名回传时乱码。
    fn ps(script: &str) -> Result<String> {
        let encoded = powershell::encoded_command(&powershell::ps_wrap(script));
        let mut cmd = Command::new("powershell.exe");
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW：后台静默，不弹黑窗
        let out = cmd
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                &encoded,
            ])
            .output()
            .map_err(|e| Error::Command {
                program: "powershell.exe".to_string(),
                source: e,
            })?;

        if !out.status.success() {
            return Err(Error::CommandFailed {
                program: "powershell.exe".to_string(),
                stderr: stderr_or_placeholder(&out.stderr),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 弹 UAC 提权执行脚本。提权进程的 stdout 拿不到（`Start-Process` 会开新进程），
    /// 所以这里只看退出码。
    fn ps_elevated(script: &str) -> Result<()> {
        let inner = powershell::encoded_command(&powershell::ps_wrap_elevated(script));
        // 1223 = ERROR_CANCELLED，用户在 UAC 弹窗点了「否」
        // -WindowStyle Hidden：被提权拉起的那个 powershell 不再闪黑窗（UAC 弹窗本身不受影响）。
        let outer = format!(
            "$ErrorActionPreference='Stop';try{{$p=Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{}'}}catch{{exit 1223}};exit $p.ExitCode",
            inner
        );
        let encoded = powershell::encoded_command(&outer);
        let mut cmd = Command::new("powershell.exe");
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW：外层这个 powershell 也静默
        let out = cmd
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .output()
            .map_err(|e| Error::Command {
                program: "powershell.exe".to_string(),
                source: e,
            })?;

        match out.status.code() {
            Some(0) => Ok(()),
            Some(1223) => Err(Error::ElevationCancelled),
            Some(code) => Err(Error::Elevation(format!(
                "提权进程退出码 {}：{}",
                code,
                stderr_or_placeholder(&out.stderr)
            ))),
            None => Err(Error::Elevation("提权进程异常终止".to_string())),
        }
    }

    fn index_of(interface: &NetworkInterface) -> Result<u32> {
        interface
            .index
            .ok_or_else(|| Error::InterfaceNotFound(interface.name.clone()))
    }
}

impl NetworkPlatform for WindowsPlatform {
    fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        let default_index = self.default_interface().ok().and_then(|d| d.index);
        let out = Self::ps(scripts::LIST_INTERFACES)?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter_map(scripts::parse_adapter_line)
            .map(|mut iface| {
                iface.is_default = iface.index.is_some() && iface.index == default_index;
                iface
            })
            .collect())
    }

    fn default_interface(&self) -> Result<NetworkInterface> {
        let out = Self::ps(scripts::DEFAULT_INTERFACE)?;
        out.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .and_then(scripts::parse_adapter_line)
            .map(|mut iface| {
                iface.is_default = true;
                iface
            })
            .ok_or(Error::NoDefaultInterface)
    }

    fn get_gateway(&self, interface: &NetworkInterface) -> Result<Option<String>> {
        let index = Self::index_of(interface)?;
        let out = Self::ps(&scripts::get_gateway_script(index))?;
        let value = out.trim();
        Ok((!value.is_empty() && value != "0.0.0.0").then(|| value.to_string()))
    }

    fn set_gateway(&self, interface: &NetworkInterface, gateway: &str) -> Result<()> {
        if !is_ipv4(gateway) {
            return Err(Error::InvalidIpv4(gateway.to_string()));
        }
        let index = Self::index_of(interface)?;
        Self::ps_elevated(&scripts::set_gateway_script(index, gateway)?)
    }

    fn get_dns(&self, interface: &NetworkInterface) -> Result<DnsConfig> {
        let index = Self::index_of(interface)?;
        let out = Self::ps(&scripts::get_dns_script(index))?;
        Ok(DnsConfig::new(
            out.lines()
                .map(str::trim)
                .filter(|l| is_ipv4(l))
                .map(str::to_string)
                .collect(),
        ))
    }

    /// 覆写：网关 + DNS 合并成一次 PowerShell，省掉一次进程冷启动（~300ms+）。
    /// 用两个不可能出现在 IP 里的标记分隔输出，解析时按标记切分即可。
    fn get_gateway_and_dns(
        &self,
        interface: &NetworkInterface,
    ) -> Result<GatewayAndDns> {
        let index = Self::index_of(interface)?;
        let script = format!(
            "Write-Output '===GATEWAY==='\n{}\nWrite-Output '===DNS==='\n{}",
            scripts::get_gateway_script(index),
            scripts::get_dns_script(index),
        );
        let out = Self::ps(&script)?;

        let gateway = out
            .split("===DNS===")
            .next()
            .and_then(|g| g.split("===GATEWAY===").nth(1))
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "0.0.0.0")
            .map(str::to_string);

        let dns = out
            .split("===DNS===")
            .nth(1)
            .map(|d| {
                d.lines()
                    .map(str::trim)
                    .filter(|l| is_ipv4(l))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        // Windows 上取 DNS 是查 WMI/CIM，网卡存在就能读到，
        // 不存在的话上面 index_of 已经报错了 —— 所以这里恒为「读到了」。
        Ok(GatewayAndDns::read(gateway, DnsConfig::new(dns)))
    }

    fn set_dns(&self, interface: &NetworkInterface, dns: &[String]) -> Result<()> {
        let index = Self::index_of(interface)?;
        Self::ps_elevated(&scripts::set_dns_script(index, dns)?)
    }

    fn flush_dns(&self) -> Result<()> {
        Self::ps(scripts::FLUSH_DNS)?;
        Ok(())
    }

    fn apply(
        &self,
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
    ) -> Result<()> {
        // 合并成一次 UAC，用户只需要授权一次
        let index = Self::index_of(interface)?;
        let mut parts = Vec::new();
        if let Some(gateway) = gateway {
            parts.push(scripts::set_gateway_script(index, gateway)?);
        }
        if let Some(dns) = dns {
            parts.push(scripts::set_dns_script(index, dns)?);
        }
        parts.push(scripts::FLUSH_DNS.to_string());
        Self::ps_elevated(&parts.join("\n"))
    }
}
