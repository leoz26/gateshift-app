# gateshift-core

旁路由网关切换 + 系统 DNS 的**核心逻辑库**，macOS / Windows 通用。
**不含任何界面**——GUI / Tauri / CLI 都由上层自己接。

```
NetworkManager (manager.rs)      ← 唯一入口，界面层只用它
       │
       ├── Config / RouterProfile (config.rs)     配置 + 校验 + 持久化
       ├── 模型 (model.rs)                          serde 化，可直接给前端
       └── Box<dyn NetworkPlatform> (platform/)    平台差异全部隔离在这里
                ├── macos.rs
                ├── windows.rs
                └── unsupported.rs
```

## 设计原则

1. **业务逻辑只依赖 `NetworkPlatform` trait**。`NetworkManager` 持有的是
   `Box<dyn NetworkPlatform>`（不是具体类型），所以可以注入假实现跑单元测试——
   整套逻辑在 macOS 上就能测完，不用等 Windows。
2. **平台差异全部隔离在 `platform/*.rs`**，加 Linux 只要加一个文件 + 一行 `cfg`。
3. **传入的 IP 一律先校验再拼命令**。非法值直接返回 `InvalidIpv4`，绝不碰系统、绝不弹授权。
4. **一次授权**。`apply()` 把「网关 + DNS + 刷新缓存」合并成一次提权调用，
   macOS 只弹一次密码 / Touch ID，Windows 只弹一次 UAC。
5. **模型全部 `serde` 化**，`NetworkState` / `NetworkInterface` / `RouterMode` 可以直接
   序列化给前端。
6. 错误统一收敛到一个 `Error`（`thiserror`），不把 `anyhow` 之类的选择强加给调用方。

## 用起来长这样

```rust
use gateshift_core::{Config, NetworkManager, RouterMode};

let config = Config::load_from_file("config.json")?;
let manager = NetworkManager::new(config)?;

let state = manager.probe()?;                    // 只读，不弹授权
println!("{} / {}", state.gateway.unwrap_or_default(), state.dns.describe());

let before = manager.snapshot()?;                // 改之前先留个底
manager.switch_to(RouterMode::BypassRouter)?;    // 切网关（+ 按配置改 DNS），弹一次授权
// manager.align_dns()?;                         // 只对齐 DNS，不动网关
// manager.reset_dns_to_auto()?;                 // 系统 DNS 恢复自动（DHCP）
manager.restore(&before)?;                       // 需要的话还原
```

接界面时，界面层只负责调 `NetworkManager`，别把网络逻辑写进前端。

### 测试工具（testkit）

核心库内置了一个**可脚本化、可回放**的假平台 `MockPlatform`，业务测逻辑时
不用碰真实系统，还能精确断言「到底调了哪些操作、传了什么参数、系统状态变成了什么样」：

```rust
use gateshift_core::testkit::MockPlatform;
use gateshift_core::{Config, NetworkManager, RouterMode};

let (platform, handle) = MockPlatform::builder()
    .gateway("192.168.1.1")
    .dns(&["223.5.5.5"])
    .build();
let mgr = NetworkManager::with_platform(Config::default(), platform).unwrap();
mgr.switch_to(RouterMode::BypassRouter).unwrap();

assert!(handle.was_called("apply"));                 // 一次合并调用
assert_eq!(handle.gateway().as_deref(), Some("192.168.1.11"));
assert_eq!(handle.count_calls("flush_dns"), 1);
```

`MockPlatform::builder()` 还能用 `fail_apply(Error::ElevationCancelled)` 之类
脚本化**一次性错误**，覆盖「授权取消 / 接口找不到」等平时极难构造的异常路径。

`testkit` 默认**不**编进发布产物：仅在本库 `cargo test` 或下游用
`features = ["testkit"]` 开启时才出现。

## 配置

默认 JSON（`.toml` 路径也认，按扩展名判断）：

```json
{
  "set_dns_on_switch": true,
  "main_router":   { "name": "主路由", "gateway": "192.168.1.1",  "dns": ["223.5.5.5", "8.8.8.8"] },
  "bypass_router": { "name": "旁路由", "gateway": "192.168.1.11", "dns": ["192.168.1.11"] }
}
```

- `dns: []` 表示恢复「自动（DHCP）」
- `set_dns_on_switch: false` 表示切网关时不动 DNS
- 加载 / 保存都会先校验；两个档位网关相同会被拒绝（否则没法判断当前档位）
- 默认路径：`Config::default_config_path()`（macOS `~/Library/Application Support/gateshift/`，
  Windows `%APPDATA%\gateshift\`），但**不强制**，路径由调用方决定

## 构建与自测

```bash
cargo build --release
cargo test                       # 全部是纯逻辑测试，不动系统设置

# 自测入口（不是产品 UI，正式接界面后可直接删 examples/）
cargo run --example probe                    # 只读：接口 / 网关 / DNS / 档位
cargo run --example probe -- switch proxy    # 真切换，会弹授权
cargo run --example probe -- align-dns
cargo run --example probe -- dns-auto
cargo run --example probe -- snapshot        # 存快照
cargo run --example probe -- restore         # 还原快照
```

Windows 上如果中文输出乱码，用 Windows Terminal，或先 `chcp 65001`
（`examples/probe.rs` 已经会自己切一次代码页）。

> **关于 `.cargo/config.toml`**：里面配了 crates.io 的国内镜像（ustc），是为了在
> 「crates.io 直连不通」的网络下也能拉依赖。如果你的网络能直连 crates.io，
> **直接删掉这个文件**即可，不影响构建。

### Windows 上实测通过（2026-10-05，Windows 11 x86_64 / 非管理员账户）

`cargo build`、`cargo test --lib`、`cargo run --example probe` 全部正常，
能真实读到网卡 / 网关 / DNS。两个容易被误判成 bug 的点：

- **Windows 上测试是 37 项、macOS 上是 41 项**。差的 4 项是 `platform/macos.rs` 里被
  `#[cfg(target_os)]` 挡住的 macOS 专属测试，Windows 上根本不参与编译，**不是有测试失败**。
- 文档测试（`cargo test --doc`）在资源受限的沙箱里可能报
  `Failed to spawn rustc.exe ... error 231`，那是环境问题。该示例标了 `no_run`，
  **只编译不执行**，不会去动系统网络设置。

**没装 Visual Studio / MSVC 怎么办**：默认目标是 `x86_64-pc-windows-msvc`，需要
`link.exe`。没有的话退到 GNU 工具链：

```bat
rustup toolchain install stable-x86_64-pc-windows-gnu
set RUSTFLAGS=-C linker=x86_64-w64-mingw32-gcc
cargo build
```

**别在网络盘 / 中文路径下直接编译**（实测踩过）：项目若放在映射盘（如 `Z:\`，实为
`\\192.168.1.x\共享盘\`）或含中文的路径里，MinGW 的 `ld` 会因 rustc 临时文件路径在
`Z:` 与 UNC 形式间混用而报 `cannot find ...symbols.o`。**先拷到本地纯 ASCII 路径
（如 `C:\work\`）再编译**即可。装了 MSVC 的机器一般不受影响。

## 各平台怎么干的

| 操作 | macOS | Windows |
|---|---|---|
| 找默认接口 | `route -n get default` → `interface:` | `Get-NetRoute -DestinationPrefix 0.0.0.0/0` 按 `RouteMetric` 排序取第一条 |
| 接口标识 | 网络服务名（`Wi-Fi`），由 `networksetup -listnetworkserviceorder` 两行配对得到 | 数字 `ifIndex`（脚本里不出现中文别名） |
| 读网关 | `route -n get default` → `gateway:` | `Get-NetRoute ... .NextHop` |
| 切网关 | `route -n change default`（失败退回 `delete`+`add`） | 先 `New-NetRoute` 再加，后 `Remove-NetRoute` 摘旧的 |
| 读 DNS | `networksetup -getdnsservers <服务名>` | `Get-DnsClientServerAddress -AddressFamily IPv4 -InterfaceIndex <n>` |
| 设 DNS | `networksetup -setdnsservers <服务名> <列表 \| Empty>` | `Set-DnsClientServerAddress -InterfaceIndex <n> -ServerAddresses @(...)` |
| 恢复自动 | 同上，传 `Empty` | `Set-DnsClientServerAddress -InterfaceIndex <n> -ResetServerAddresses` |
| 刷新缓存 | `dscacheutil -flushcache` + `killall -HUP mDNSResponder` | `ipconfig /flushdns` |
| 提权 | `osascript … with administrator privileges` | `Start-Process -Verb RunAs`（退出码 1223 = 用户取消 UAC） |

### Windows 上踩过的坑（都已避开）

- `Set-DnsClientServerAddress` **没有** `-AddressFamily` 参数（只有 `Get-` 那个有），
  多写一个会直接报 "A parameter cannot be found"。
- PowerShell 一律用 `-EncodedCommand`（UTF-16LE + Base64）传参，并在脚本里把
  `[Console]::OutputEncoding` 设成 UTF-8——否则中文网卡名（`以太网`）会乱码 / 传参出错。
- 改默认路由**先加后删**，顺序反了会出现「短暂没有默认路由」的空窗期。
- `New-NetRoute` 默认写进 PersistentStore（重启还在）；这里显式用
  `-PolicyStore ActiveStore`，跟 macOS 的 `route` 对齐——只在当前生效，重启回到 DHCP 原状。
  **想常驻就去掉那个参数。**

## 已知取舍

- **只管 IPv4**。IPv6 DNS 一律不碰；`DnsConfig` 里只会有 IPv4 地址。
  注意 macOS 的 `networksetup -setdnsservers` 是**整体替换** DNS 列表，所以设置 IPv4 时
  会连带覆盖同一服务上原有的 IPv6 DNS（Windows 侧因为按 AddressFamily 操作，不受影响）。
- **网关切换是非持久的**：Wi-Fi 重连 / 重启后可能被 DHCP 复位。需要常驻请自行加
  launchd（macOS）/ 计划任务（Windows）。
- **Windows 的 `list_interfaces` 只列 `Status -eq 'Up'` 的网卡**（macOS 侧列出全部硬件
  端口）。切网关只对当前在用的网卡有意义，没插网线的适配器选了也切不动，所以过滤掉更干净。
  想列出全部网卡，把 `platform/windows_scripts.rs` 里 `LIST_INTERFACES` 的
  `Where-Object` 那行删掉即可。
- `restore()` 只还原 DNS，不还原网关；接口名变了会拒绝执行，避免把 DNS 设到错的网卡上。
