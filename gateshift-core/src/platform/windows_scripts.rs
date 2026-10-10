//! Windows 侧**纯字符串**逻辑：PowerShell 脚本文本、占位符替换、输出解析。
//!
//! 刻意不放在 `windows.rs` 里（那个文件被 `#[cfg(windows)]` 挡着），
//! 这样占位符替换、加/删路由顺序、`Set-DnsClientServerAddress` 的参数禁忌这些
//! 真正容易写错的地方，在 macOS / Linux 上也能跑单元测试。

#![allow(dead_code)]

use crate::error::{Error, Result};
use crate::model::NetworkInterface;
use crate::util::is_ipv4;

/// 用占位符而不是 `format!`，免得跟 PowerShell 的花括号打架。
const INDEX_TOKEN: &str = "__INDEX__";
const GATEWAY_TOKEN: &str = "__GATEWAY__";
const SERVERS_TOKEN: &str = "__SERVERS__";

/// 当前默认路由走的网卡。
pub const DEFAULT_INTERFACE: &str = r#"
$r = Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' |
     Where-Object { $_.NextHop -ne '0.0.0.0' } |
     Sort-Object RouteMetric, Metric |
     Select-Object -First 1
if ($r) {
    $a = Get-NetAdapter -InterfaceIndex $r.ifIndex
    Write-Output ("{0}|{1}" -f $a.Name, $a.ifIndex)
}
"#;

/// 所有处在 Up 状态的网卡。
pub const LIST_INTERFACES: &str = r#"
Get-NetAdapter |
    Where-Object { $_.Status -eq 'Up' } |
    ForEach-Object { Write-Output ("{0}|{1}" -f $_.Name, $_.ifIndex) }
"#;

pub const GET_GATEWAY: &str = r#"
$r = Get-NetRoute -AddressFamily IPv4 -InterfaceIndex __INDEX__ -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue |
     Sort-Object RouteMetric, Metric |
     Select-Object -First 1
if ($r) { Write-Output $r.NextHop }
"#;

/// 改默认网关。
///
/// **先加新路由、再摘旧路由** —— 顺序反了会出现「短暂没有默认路由」的空窗，
/// 那一刻正好在跑的东西会直接断网。
///
/// `-PolicyStore ActiveStore` = 只在当前生效，重启后回到 DHCP 给的原状
/// （跟 macOS 的 `route` 行为对齐）。想常驻就去掉这个参数。
pub const SET_GATEWAY: &str = r#"
$gw  = '__GATEWAY__'
$idx = __INDEX__
$existing = @(Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue)
if (@($existing | Where-Object { $_.NextHop -eq $gw }).Count -eq 0) {
    New-NetRoute -DestinationPrefix '0.0.0.0/0' -InterfaceIndex $idx -NextHop $gw -PolicyStore ActiveStore | Out-Null
}
$existing | Where-Object { $_.NextHop -ne $gw } | Remove-NetRoute -Confirm:$false | Out-Null
"#;

pub const GET_DNS: &str = r#"
(Get-DnsClientServerAddress -AddressFamily IPv4 -InterfaceIndex __INDEX__).ServerAddresses
"#;

/// 注意：`Set-DnsClientServerAddress` **没有** `-AddressFamily` 参数，
/// 只有 `-ServerAddresses` / `-ResetServerAddresses` / `-InterfaceIndex` / `-InterfaceAlias`。
/// 多写一个 `-AddressFamily` 会直接报 "A parameter cannot be found"。
pub const SET_DNS: &str = r#"
Set-DnsClientServerAddress -InterfaceIndex __INDEX__ -ServerAddresses @(__SERVERS__)
"#;

/// 空列表 = 恢复 DHCP 自动。
pub const RESET_DNS: &str = r#"
Set-DnsClientServerAddress -InterfaceIndex __INDEX__ -ResetServerAddresses
"#;

/// `ipconfig /flushdns` 不需要管理员权限。
pub const FLUSH_DNS: &str = "ipconfig /flushdns | Out-Null";

pub fn set_gateway_script(index: u32, gateway: &str) -> Result<String> {
    if !is_ipv4(gateway) {
        return Err(Error::InvalidIpv4(gateway.to_string()));
    }
    Ok(SET_GATEWAY
        .replace(INDEX_TOKEN, &index.to_string())
        .replace(GATEWAY_TOKEN, gateway))
}

pub fn get_gateway_script(index: u32) -> String {
    GET_GATEWAY.replace(INDEX_TOKEN, &index.to_string())
}

pub fn get_dns_script(index: u32) -> String {
    GET_DNS.replace(INDEX_TOKEN, &index.to_string())
}

pub fn set_dns_script(index: u32, dns: &[String]) -> Result<String> {
    if dns.is_empty() {
        return Ok(RESET_DNS.replace(INDEX_TOKEN, &index.to_string()));
    }
    for d in dns {
        if !is_ipv4(d) {
            return Err(Error::InvalidIpv4(d.clone()));
        }
    }
    let servers = dns
        .iter()
        .map(|d| format!("'{}'", d))
        .collect::<Vec<_>>()
        .join(",");
    Ok(SET_DNS
        .replace(INDEX_TOKEN, &index.to_string())
        .replace(SERVERS_TOKEN, &servers))
}

/// 解析 `名称|ifIndex` 形式的一行。`ifIndex` 解析不出来时保留网卡名。
pub fn parse_adapter_line(line: &str) -> Option<NetworkInterface> {
    let (name, index) = line.split_once('|')?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(NetworkInterface {
        name,
        index: index.trim().parse::<u32>().ok(),
        device: None,
        is_default: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adapter_line() {
        let parsed = parse_adapter_line("以太网|7").unwrap();
        assert_eq!(parsed.name, "以太网");
        assert_eq!(parsed.index, Some(7));
        assert!(parsed.device.is_none());

        assert_eq!(parse_adapter_line("Wi-Fi|?").unwrap().index, None);
        assert!(parse_adapter_line("|7").is_none());
        assert!(parse_adapter_line("没有分隔符").is_none());
    }

    #[test]
    fn set_gateway_substitutes_tokens() {
        let script = set_gateway_script(12, "192.168.1.11").unwrap();
        assert!(!script.contains(INDEX_TOKEN));
        assert!(!script.contains(GATEWAY_TOKEN));
        assert!(script.contains("$idx = 12"), "{}", script);
        assert!(script.contains("$gw  = '192.168.1.11'"), "{}", script);
    }

    #[test]
    fn set_gateway_adds_before_removing() {
        let script = set_gateway_script(12, "192.168.1.11").unwrap();
        let add = script.find("New-NetRoute").unwrap();
        let remove = script.find("Remove-NetRoute").unwrap();
        assert!(add < remove, "必须先加新路由再删旧路由，否则有空窗期");
    }

    #[test]
    fn set_gateway_is_not_persistent() {
        let script = set_gateway_script(12, "192.168.1.11").unwrap();
        assert!(script.contains("-PolicyStore ActiveStore"));
    }

    #[test]
    fn set_gateway_rejects_bad_ipv4() {
        assert!(matches!(
            set_gateway_script(1, "192.168.1"),
            Err(Error::InvalidIpv4(_))
        ));
        assert!(matches!(
            set_gateway_script(1, "nope"),
            Err(Error::InvalidIpv4(_))
        ));
    }

    #[test]
    fn set_dns_never_uses_address_family() {
        // 回归测试：Set-DnsClientServerAddress 没有 -AddressFamily 参数
        let auto = set_dns_script(3, &[]).unwrap();
        let list = set_dns_script(3, &["1.1.1.1".to_string()]).unwrap();
        for script in [auto, list] {
            assert!(script.contains("Set-DnsClientServerAddress"));
            assert!(
                !script.contains("-AddressFamily"),
                "Set-DnsClientServerAddress 不接受 -AddressFamily：{}",
                script
            );
        }
    }

    #[test]
    fn set_dns_handles_auto_and_list() {
        let auto = set_dns_script(3, &[]).unwrap();
        assert!(auto.contains("-ResetServerAddresses"), "{}", auto);
        assert!(auto.contains("3"), "{}", auto);

        let list =
            set_dns_script(3, &["223.5.5.5".to_string(), "8.8.8.8".to_string()]).unwrap();
        assert!(list.contains("@('223.5.5.5','8.8.8.8')"), "{}", list);
    }

    #[test]
    fn set_dns_rejects_bad_ipv4() {
        assert!(matches!(
            set_dns_script(3, &["bad".to_string()]),
            Err(Error::InvalidIpv4(_))
        ));
    }

    #[test]
    fn get_scripts_substitute_index() {
        assert!(get_gateway_script(9).contains("-InterfaceIndex 9"));
        assert!(get_dns_script(9).contains("-InterfaceIndex 9"));
        assert!(!get_dns_script(9).contains(INDEX_TOKEN));
    }
}
