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
//!
//! 提权脚本的两个硬性要求（都踩过坑）：
//! 1. 必须是 `set -e` 开头的脚本 —— `do shell script` 只看**最后一条**命令的退出码，
//!    否则中间的 `route` / `networksetup` 失败会被吞掉，变成「假成功」。
//! 2. 用户可控的字符串（服务名）一律用**单引号**包成 shell 字面量 —— 只会转义 `\` / `"`
//!    是不够的，`$` 和反引号在双引号里照样会在 root shell 里被展开。
//!
//! 还有一个容易翻车的点：**DNS 读不到不等于探测失败**。VPN（`utun*`）之类的虚拟接口
//! 没有对应的网络服务，`networksetup -getdnsservers utun3` 必然报
//! "not a recognized network service"。旧实现把它一路 `?` 出去，用户看到的是
//! 整次「探测失败」，连网关和档位都拿不到。现在只把 DNS 降级成空列表（见 [`dns_or_auto`]），
//! 网关 / 接口照常返回。

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use std::process::Command;

use super::NetworkPlatform;
use crate::error::{stderr_or_placeholder, Error, Result};
use crate::model::{DnsConfig, GatewayAndDns, NetworkInterface};
use crate::util::is_ipv4;

/// `networksetup -listnetworkserviceorder` 的缓存时长：网络服务列表极少变动，
/// 而一次 probe 里 `default_interface` / `list_interfaces` 都要读它。
const SERVICES_TTL: Duration = Duration::from_secs(3);
/// `route -n get default` 的输出复用时长：只为让同一次 probe 里的
/// `default_interface` 与 `get_gateway` 共用一次 `route`，不重复 spawn。
const ROUTE_TTL: Duration = Duration::from_secs(1);

/// 带时间戳的缓存项。
struct Cache<T> {
    at: Instant,
    value: T,
}

/// `networksetup` 的一行：`(服务名, 设备名)`。
type ServiceEntry = (String, Option<String>);

/// macOS 实现。带两个短 TTL 缓存（网络服务表、默认路由输出），
/// 让一次 `probe` 不再重复 spawn 同样的命令。
pub struct MacOsPlatform {
    services: Mutex<Option<Cache<Vec<ServiceEntry>>>>,
    default_route: Mutex<Option<Cache<String>>>,
}

impl Default for MacOsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOsPlatform {
    pub fn new() -> Self {
        MacOsPlatform {
            services: Mutex::new(None),
            default_route: Mutex::new(None),
        }
    }

    /// 锁一个缓存槽位；被毒化（某个线程 panic 时）也只重置一次，不把整条链路带崩。
    fn slot<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// TTL 缓存：命中直接返回，未命中执行 `load` 并记下来。
    fn cached<T: Clone>(
        target: &Mutex<Option<Cache<T>>>,
        ttl: Duration,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if let Some(hit) = Self::slot(target).as_ref() {
            if hit.at.elapsed() < ttl {
                return Ok(hit.value.clone());
            }
        }
        let value = load()?;
        let mut guard = Self::slot(target);
        *guard = Some(Cache {
            at: Instant::now(),
            value: value.clone(),
        });
        Ok(value)
    }

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

    /// 网络服务表（带 TTL 缓存，一次探测里会被问好几遍）。
    fn services(&self) -> Result<Vec<ServiceEntry>> {
        Self::cached(&self.services, SERVICES_TTL, || {
            Ok(parse_services(&Self::read(
                "networksetup",
                &["-listnetworkserviceorder"],
            )?))
        })
    }

    /// `route -n get default` 的原始输出（带 TTL 缓存：同一次探测里
    /// `default_interface` 和 `get_gateway` 共用一次 `route`，不再跑两遍）。
    fn default_route(&self) -> Result<String> {
        Self::cached(&self.default_route, ROUTE_TTL, || {
            Self::read("route", &["-n", "get", "default"])
        })
    }

    /// 把若干条 shell 命令合并成**一次**提权执行，只弹一次密码 / Touch ID。
    fn elevated(&self, commands: &[String]) -> Result<()> {
        let script = elevated_script(commands);
        let out = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| Error::Command {
                program: "osascript".to_string(),
                source: e,
            })?;
        if out.status.success() {
            // 刚改过系统状态，路由缓存立刻作废，下次探测必须重新读
            *Self::slot(&self.default_route) = None;
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

    /// 生成改默认网关的命令。
    ///
    /// 脚本连同 `set -e` 一起跑，所以这里要把「change 失败就 delete + add」
    /// 写成一条 `if` 复合命令：`if` 内部的 `delete` 失败不算致命（用 `|| :` 兜掉），
    /// 但最后的 `add` 失败会以非零退出码传出去，不会被当成成功。
    fn route_command(gateway: &str) -> Result<String> {
        if !is_ipv4(gateway) {
            return Err(Error::InvalidIpv4(gateway.to_string()));
        }
        // change 优先：不必先删路由，避免出现「短暂无默认路由」的窗口；
        // 路由不存在时再退回 delete + add。
        Ok(format!(
            "if ! route -n change default {gw} 2>/dev/null; then route -n delete default 2>/dev/null || :; route -n add default {gw}; fi",
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
            dns.iter()
                .map(|d| shell_quote(d))
                .collect::<Vec<_>>()
                .join(" ")
        };
        // 服务名来自系统输出（可能含空格），也可能被伪造：
        // 一律包成**单引号**字面量，$VAR / `...` / $() 在 root shell 里都不会展开。
        Ok(format!(
            "networksetup -setdnsservers {} {}",
            shell_quote(&interface.name),
            arg
        ))
    }

    /// 这个接口的 DNS 是否归 `networksetup` 管。
    ///
    /// VPN（`utun*`）之类的虚拟接口在 `-listnetworkserviceorder` 里没有条目，
    /// [`resolve_service`] 就把设备名当服务名返回；拿它去 `-setdnsservers` 必然报
    /// "not a recognized network service"，而这类接口的 DNS 本来就由 VPN 客户端自己管。
    ///
    /// 服务表本身读不到时不做判断、沿用旧行为（照常写 DNS）—— 真失败会由 `set -e`
    /// 报出来，不会静默吞掉。
    fn dns_manageable(&self, interface: &NetworkInterface) -> bool {
        match self.services() {
            Ok(services) => is_managed_service(&services, &interface.name),
            Err(_) => true,
        }
    }

    /// 组装这次 `apply` 要交给 `osascript` 的命令列表。
    ///
    /// 抽成纯函数是为了能断言「DNS 不可管理时 `-setdnsservers` 根本没进脚本」，
    /// 而不必真的跑 `networksetup`。注意 `dns_manageable = false` 时是**不生成**那条
    /// 命令，而不是给它加 `|| :` 兜底 —— 后者会把「真的设置失败」也一起吞掉。
    fn apply_commands(
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
        dns_manageable: bool,
    ) -> Result<Vec<String>> {
        let mut commands = Vec::new();
        if let Some(gateway) = gateway {
            commands.push(Self::route_command(gateway)?);
        }
        if let Some(dns) = dns {
            if dns_manageable {
                commands.push(Self::dns_command(interface, dns)?);
            }
        }
        commands.extend(Self::flush_commands());
        Ok(commands)
    }

    fn flush_commands() -> Vec<String> {
        vec![
            "dscacheutil -flushcache".to_string(),
            // killall 需要 root，所以它跟其它命令合并进同一次提权里跑。
            // mDNSResponder 不存在（老系统 / 被禁用）不算失败，用 `|| :` 兜掉；
            // 前面两条关键命令的失败仍然会由 set -e 传出来。
            "/usr/bin/killall -HUP mDNSResponder 2>/dev/null || :".to_string(),
        ]
    }
}

/// 把字符串包成 shell 单引号字面量：单引号内 shell 不做任何展开，
/// 内部的 `'` 用 `'\''`（收引号 → 转义单引号 → 重开引号）拼接。
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// AppleScript 字符串字面量转义：只需处理反斜杠与双引号。
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 生成交给 `osascript` 的提权脚本。
///
/// `set -e` 是关键：`do shell script` 只回报**最后一条**命令的退出码，
/// 少了它，中间的 `route` / `networksetup` 失败会被后面的 `killall || :`
/// 掩盖成「成功」，用户以为切换好了、其实没切。
fn elevated_script(commands: &[String]) -> String {
    let body = format!("set -e; {}", commands.join("; "));
    format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript(&body)
    )
}

/// 从 `route -n get default` 的输出里取默认路由的设备名（如 `en0`）。
fn parse_default_device(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix("interface:"))
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
}

/// 从 `route -n get default` 的输出里取默认网关。
fn parse_default_gateway(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix("gateway:"))
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_string)
}

/// 设备名 → 网络服务名。VPN 之类的虚拟接口没有对应服务，退回用设备名。
fn resolve_service(services: &[ServiceEntry], device: &str) -> String {
    services
        .iter()
        .find(|(_, dev)| dev.as_deref() == Some(device))
        .map(|(service, _)| service.clone())
        .unwrap_or_else(|| device.to_string())
}

/// 把 `route -n get default` 的输出与 DNS 读取结果合成一次探测结果。
///
/// 抽成纯函数是为了能在测试里模拟 `networksetup -getdnsservers` 失败 ——
/// VPN 的 `utun*` 接口没有对应网络服务，真实命令必然返回非零。
fn combine_probe(
    route_out: &str,
    dns: Result<DnsConfig>,
    interface: &NetworkInterface,
) -> GatewayAndDns {
    let gateway = parse_default_gateway(route_out);
    let (dns, dns_readable) = dns_or_auto(interface, dns);
    GatewayAndDns {
        gateway,
        dns,
        dns_readable,
    }
}

/// DNS 读不到时降级成**空列表**（等价于「自动 / 未知」）+ `dns_readable = false`，
/// 而不是让整次探测失败。
///
/// 与「DNS 本来就是空的」在 [`DnsConfig`] 上同形，所以必须把 `dns_readable` 一起往上传：
/// - `dns_readable = true` + 空列表 → 真的自动，可以照常比对 / 改写；
/// - `dns_readable = false` → 读不到，上层应当**跳过** DNS 写入（`switch_to`）
///   或直接报错（`align_dns` / `reset_dns_to_auto`）。
fn dns_or_auto(interface: &NetworkInterface, dns: Result<DnsConfig>) -> (DnsConfig, bool) {
    match dns {
        Ok(dns) => (dns, true),
        Err(err) => {
            eprintln!(
                "gateshift: 读取接口「{}」的 DNS 失败，按「读不到」处理（网关与档位不受影响）：{err}",
                interface.name
            );
            (DnsConfig::new(Vec::new()), false)
        }
    }
}

/// 服务表里有没有同名条目 —— 有，才说明 `networksetup` 认这个接口名。
fn is_managed_service(services: &[ServiceEntry], name: &str) -> bool {
    services.iter().any(|(service, _)| service == name)
}

/// 解析 `networksetup -listnetworkserviceorder` → `[(服务名, 设备名)]`。
fn parse_services(output: &str) -> Vec<ServiceEntry> {
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
        Ok(self
            .services()?
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
        let out = self.default_route()?;
        let device = parse_default_device(&out).ok_or(Error::NoDefaultInterface)?;

        // 反查设备名对应的网络服务名；VPN 之类的虚拟接口没有对应服务，退回用设备名
        let service = resolve_service(&self.services()?, &device);

        Ok(NetworkInterface {
            name: service,
            index: None,
            device: Some(device),
            is_default: true,
        })
    }

    /// 一次 `route -n get default` 同时拿到接口和网关，避免按顺序两次 spawn。
    ///
    /// **只有网关 / 接口读不到才算失败**；DNS 读不到只降级（VPN 的 `utun*` 必然走到这条路）
    /// 并把 `dns_readable = false` 带出去，否则用户在没有对应网络服务的接口上连当前档位
    /// 都看不到。
    fn get_gateway_and_dns(&self, interface: &NetworkInterface) -> Result<GatewayAndDns> {
        let route = self.default_route()?;
        Ok(combine_probe(&route, self.get_dns(interface), interface))
    }

    fn get_gateway(&self, _interface: &NetworkInterface) -> Result<Option<String>> {
        Ok(parse_default_gateway(&self.default_route()?))
    }

    fn set_gateway(&self, _interface: &NetworkInterface, gateway: &str) -> Result<()> {
        self.elevated(&[Self::route_command(gateway)?])
    }

    /// 读 DNS。这里**保持严格**：读不到就报错 —— 是否降级由 [`Self::get_gateway_and_dns`]
    /// 决定，单独调用它的人应该知道命令确实失败了。
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
        // 直接调用 set_dns 的人应该知道「这个接口根本设不了 DNS」，
        // 而不是拿到一条注定失败的脚本 / 一个假装成功的返回。
        if !self.dns_manageable(interface) {
            return Err(Error::DnsUnmanageable(interface.name.clone()));
        }
        self.elevated(&[Self::dns_command(interface, dns)?])
    }

    fn flush_dns(&self) -> Result<()> {
        self.elevated(&Self::flush_commands())
    }

    fn apply(
        &self,
        interface: &NetworkInterface,
        gateway: Option<&str>,
        dns: Option<&[String]>,
    ) -> Result<()> {
        // 只有真要写 DNS 时才去查服务表（纯切网关不用多 spawn 一次 networksetup）
        let dns_manageable = match dns {
            Some(_) => self.dns_manageable(interface),
            None => true,
        };
        if dns.is_some() && !dns_manageable {
            eprintln!(
                "gateshift: 接口「{}」的 DNS 不受系统管理（VPN 等），已跳过 DNS 写入",
                interface.name
            );
        }
        // 合并成一次提权，用户只需要授权一次
        let commands = Self::apply_commands(interface, gateway, dns, dns_manageable)?;
        self.elevated(&commands)
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

    #[test]
    fn shell_quote_neutralizes_substitution() {
        // 服务名来自系统输出，但用户也能自己造一个含 $() / 反引号的服务名
        assert_eq!(shell_quote("Wi-Fi"), "'Wi-Fi'");
        assert_eq!(shell_quote("Wi Fi"), "'Wi Fi'");
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_quote("`id`"), "'`id`'");
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
        // 单引号包裹后，`'` 不会脱离引号、`$` 与反引号都不会被展开
        for evil in ["$(whoami)", "`whoami`", "a'b", "x\"y", "sp ace"] {
            let quoted = shell_quote(evil);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''), "{}", quoted);
            assert_eq!(unquote_single(&quoted), evil);
        }
    }

    #[test]
    fn dns_command_quotes_service_name_and_servers() {
        let cmd = MacOsPlatform::dns_command(
            &interface("Wi-Fi $HOME `id`"),
            &["223.5.5.5".to_string(), "8.8.8.8".to_string()],
        )
        .unwrap();
        // 服务名绝不出现在双引号里，也不会裸奔
        assert!(!cmd.contains('"'), "{}", cmd);
        assert!(cmd.contains("'Wi-Fi $HOME `id`'"), "{}", cmd);
        assert!(cmd.contains("'223.5.5.5' '8.8.8.8'"), "{}", cmd);

        // 含单引号的服务名也要安全：query 指令不能被注入
        let cmd = MacOsPlatform::dns_command(&interface("O'Brien"), &[]).unwrap();
        assert!(cmd.contains(r"'O'\''Brien'"), "{}", cmd);
        assert!(!cmd.contains("\"O'Brien\""), "{}", cmd);
    }

    #[test]
    fn elevated_script_starts_with_set_e() {
        // 回归测试：没有 set -e 时 do shell script 只看最后一条的退出码，
        // 中间的 route / networksetup 失败会被 killall 的兜底掩盖成「成功」
        let script = elevated_script(&MacOsPlatform::flush_commands());
        assert!(script.starts_with("do shell script \""), "{}", script);
        assert!(script.contains("\"set -e; "), "{}", script);
        assert!(
            script.ends_with("\" with administrator privileges"),
            "{}",
            script
        );
    }

    #[test]
    fn elevated_script_puts_failing_commands_before_guarded_ones() {
        let commands = vec![
            MacOsPlatform::route_command("192.168.1.11").unwrap(),
            MacOsPlatform::dns_command(&interface("Wi-Fi"), &["192.168.1.11".to_string()]).unwrap(),
            "/usr/bin/killall -HUP mDNSResponder 2>/dev/null || :".to_string(),
        ];
        let script = elevated_script(&commands);
        // 关键步骤不能被兜底写法掩盖
        let route_at = script.find("route -n").unwrap();
        let dns_at = script.find("networksetup -setdnsservers").unwrap();
        let kill_at = script.find("killall").unwrap();
        assert!(route_at < dns_at && dns_at < kill_at, "{}", script);
        assert!(script.contains("set -e;"), "{}", script);
        // 兜底只剩 killall 那条，而且是 `|| :` 不是 `|| true`
        assert_eq!(script.matches("|| true").count(), 0, "{}", script);
    }

    #[test]
    fn elevated_script_escapes_applescript_quotes() {
        let script = elevated_script(&[
            MacOsPlatform::dns_command(&interface("a\"b\\c"), &[]).unwrap(),
        ]);
        // AppleScript 层：内部双引号要转义成 \"、反斜杠要成 \\
        assert!(script.contains("\\\""), "{}", script);
        assert!(script.contains("\\\\"), "{}", script);
        assert_eq!(escape_applescript("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn route_command_falls_back_to_delete_then_add() {
        let cmd = MacOsPlatform::route_command("192.168.1.11").unwrap();
        // 关键步骤都在同一条 if 里，最后一条是 add：它失败就会让整条脚本失败
        assert!(cmd.starts_with("if ! route -n change default 192.168.1.11"), "{}", cmd);
        assert!(cmd.contains("route -n delete default"), "{}", cmd);
        assert!(cmd.ends_with("route -n add default 192.168.1.11; fi"), "{}", cmd);
    }

    #[test]
    fn flush_commands_do_not_swallow_failures() {
        let commands = MacOsPlatform::flush_commands();
        assert_eq!(commands[0], "dscacheutil -flushcache");
        for cmd in &commands {
            assert!(!cmd.ends_with("|| true"), "兜底写法会掩盖失败：{}", cmd);
        }
    }

    #[test]
    fn parses_route_get_default() {
        let out = "\
   route to: default
destination: default
       mask: default
    gateway: 192.168.1.11
  interface: en0
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>
";
        assert_eq!(parse_default_device(out).as_deref(), Some("en0"));
        assert_eq!(parse_default_gateway(out).as_deref(), Some("192.168.1.11"));
        assert_eq!(parse_default_device("garbage"), None);
        assert_eq!(parse_default_gateway("garbage"), None);
    }

    #[test]
    fn resolves_service_from_device_and_falls_back() {
        let services = parse_services(SAMPLE);
        assert_eq!(resolve_service(&services, "en0"), "Ethernet");
        assert_eq!(resolve_service(&services, "en1"), "Wi-Fi");
        // VPN / utun 之类没有对应服务，退回设备名
        assert_eq!(resolve_service(&services, "utun3"), "utun3");
    }

    /// VPN 场景下 `route -n get default` 的输出：走的是 `utun3`。
    const VPN_ROUTE: &str = "\
   route to: default
destination: default
       mask: default
    gateway: 10.8.0.1
  interface: utun3
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>
";

    #[test]
    fn detects_which_names_networksetup_manages() {
        let services = parse_services(SAMPLE);
        // 服务表里有同名条目 → 归 networksetup 管
        assert!(is_managed_service(&services, "Wi-Fi"));
        assert!(is_managed_service(&services, "Ethernet"));
        // 回退出来的设备名不在服务表里 → 不归它管（VPN 的典型形态）
        assert!(!is_managed_service(&services, "utun3"));
        assert!(!is_managed_service(&services, "en0"));
    }

    #[test]
    fn apply_skips_setdnsservers_for_unmanageable_interface() {
        // VPN（utun3）场景：DNS 不归 networksetup 管 → 脚本里不能有 -setdnsservers，
        // 也不能用 `|| :` 兜底；网关与 flush 照旧
        let commands = MacOsPlatform::apply_commands(
            &interface("utun3"),
            Some("192.168.1.11"),
            Some(&["1.1.1.1".to_string()]),
            false,
        )
        .unwrap();
        let script = elevated_script(&commands);
        assert!(
            !script.contains("setdnsservers"),
            "DNS 不可管理时不该生成 -setdnsservers：{script}"
        );
        assert!(script.contains("route -n change default 192.168.1.11"), "{script}");
        assert!(script.contains("dscacheutil -flushcache"), "flush 不能一起被跳过：{script}");
        assert!(script.contains("set -e"), "set -e 必须保留：{script}");
        assert!(!script.contains("|| true"), "{script}");
    }

    #[test]
    fn apply_keeps_setdnsservers_for_normal_interface() {
        // 回归：普通网卡照旧写 DNS
        let commands = MacOsPlatform::apply_commands(
            &interface("Wi-Fi"),
            Some("192.168.1.11"),
            Some(&["1.1.1.1".to_string()]),
            true,
        )
        .unwrap();
        let script = elevated_script(&commands);
        assert!(script.contains("networksetup -setdnsservers 'Wi-Fi' '1.1.1.1'"), "{script}");

        // 只切网关（dns = None）时也不该凭空多出 DNS 步骤
        let commands =
            MacOsPlatform::apply_commands(&interface("Wi-Fi"), Some("192.168.1.11"), None, true)
                .unwrap();
        let script = elevated_script(&commands);
        assert!(!script.contains("setdnsservers"), "{script}");
        assert!(script.contains("route -n change default"), "{script}");
    }

    #[test]
    fn vpn_interface_falls_back_to_device_name() {
        // utun 设备名在服务表里找不到 → 服务名退回设备名，
        // 这也是后面 `-getdnsservers utun3` 必然失败的原因
        let services = parse_services(SAMPLE);
        let device = parse_default_device(VPN_ROUTE).expect("VPN 路由必须有 interface");
        assert_eq!(device, "utun3");
        assert_eq!(resolve_service(&services, &device), "utun3");
    }

    #[test]
    fn probe_degrades_dns_failure_to_empty_list() {
        // 回归测试：`networksetup -getdnsservers utun3` 报
        // "not a recognized network service" 时，不能把整次探测带崩 ——
        // 网关必须照常返回，DNS 降级成空列表（自动 / 未知）
        let dns_err = Error::CommandFailed {
            program: "networksetup".to_string(),
            stderr: "utun3 is not a recognized network service.".to_string(),
        };
        let probe = combine_probe(VPN_ROUTE, Err(dns_err), &interface("utun3"));
        assert_eq!(probe.gateway.as_deref(), Some("10.8.0.1"));
        assert!(probe.dns.is_auto(), "DNS 读不到时应降级为空列表");
        // 关键：降级要标出来，否则上层分不清「读不到」和「本来就是空的」
        assert!(!probe.dns_readable, "降级后 dns_readable 必须是 false");
    }

    #[test]
    fn probe_passes_dns_through_when_read_succeeds() {
        let dns = DnsConfig::new(vec!["223.5.5.5".to_string(), "8.8.8.8".to_string()]);
        let probe = combine_probe(VPN_ROUTE, Ok(dns.clone()), &interface("Wi-Fi"));
        assert_eq!(probe.gateway.as_deref(), Some("10.8.0.1"));
        assert_eq!(probe.dns, dns, "读成功时原样透传，不该被降级覆盖");
        assert!(probe.dns_readable);

        // 「DNS 本来就是空的」与降级结果在数据上同形（都是空列表），
        // 但 dns_readable 必须区分开：这条是 true
        let probe = combine_probe(
            VPN_ROUTE,
            Ok(DnsConfig::new(Vec::new())),
            &interface("Wi-Fi"),
        );
        assert!(probe.dns.is_auto());
        assert!(probe.dns_readable, "读到空列表 = 真的是自动，不是读不到");
    }

    #[test]
    fn probe_keeps_gateway_none_when_route_has_no_gateway() {
        // 只有 interface、没有 gateway 的路由输出（部分 VPN）：
        // 网关为 None 不算错误，DNS 照常返回
        let route = "   route to: default\n  interface: utun3\n";
        let probe = combine_probe(
            route,
            Ok(DnsConfig::new(vec!["1.1.1.1".to_string()])),
            &interface("utun3"),
        );
        assert_eq!(probe.gateway, None);
        assert_eq!(probe.dns.ipv4, vec!["1.1.1.1".to_string()]);
        assert!(probe.dns_readable);
    }

    #[test]
    fn services_cache_hits_until_ttl_expires() {
        let calls = std::cell::Cell::new(0);
        let slot: Mutex<Option<Cache<Vec<String>>>> = Mutex::new(None);
        let load = || {
            calls.set(calls.get() + 1);
            Ok(vec!["v".to_string()])
        };
        assert_eq!(MacOsPlatform::cached(&slot, Duration::from_secs(30), load).unwrap(), vec!["v"]);
        assert_eq!(MacOsPlatform::cached(&slot, Duration::from_secs(30), load).unwrap(), vec!["v"]);
        assert_eq!(calls.get(), 1, "TTL 内应命中缓存");
        // TTL 为 0 时必然穿透，保证「刚改完系统」能读到新值
        assert_eq!(MacOsPlatform::cached(&slot, Duration::ZERO, load).unwrap(), vec!["v"]);
        assert_eq!(calls.get(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn set_e_stops_at_first_failure() {
        // 用 /bin/sh 本地验证脚本语义（不碰任何系统状态）
        let ok = Command::new("sh")
            .args(["-c", "set -e; true; false; true"])
            .status()
            .unwrap();
        assert!(!ok.success(), "set -e 下中间失败必须让整条脚本失败");
        let swallowed = Command::new("sh")
            .args(["-c", "true; false; true || true"])
            .status()
            .unwrap();
        assert!(swallowed.success(), "没有 set -e 时正是这种假成功");
    }

    /// 测试辅助：把 `'...'` 字面量还原成原文（校验转义可逆）。
    fn unquote_single(quoted: &str) -> String {
        let inner = quoted
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .expect("必须是单引号字面量");
        inner.replace("'\\''", "'")
    }
}
