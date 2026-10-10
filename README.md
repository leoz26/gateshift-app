# GateShift · Tauri + React 壳

基于 [`gateshift-core`](https://doc.rust-lang.org/)（旁路由网关切换 + 系统 DNS 的纯逻辑核心库）的桌面壳。
分层原则：**core 只管逻辑，Tauri 只管壳**。

## 结构

```
gateshift-app/
├── package.json / vite.config.ts / tsconfig.json   # 前端（Vite + React + TS）
├── src/
│   ├── App.tsx                 # 页面编排：状态 / 切换 / 配置 / 日志
│   ├── lib/
│   │   ├── api.ts              # 对 Tauri invoke 的封装，每个函数对应一个后端命令
│   │   └── types.ts            # 与 core 模型一一对应的 TS 类型
│   ├── components/
│   │   ├── StatusCard.tsx      # 当前接口 / 网关 / DNS / 档位
│   │   ├── ModeSwitch.tsx      # 主路由 ↔ 旁路由 一键切换（带确认弹窗）
│   │   ├── ConfigEditor.tsx    # 网关 + DNS 编辑（前端先做 IPv4 校验）
│   │   └── （开机自启开关在 App.tsx 底部设置区，基于 tauri-plugin-autostart）
│   │   └── LogView.tsx         # 操作 / 错误日志
│   └── styles.css
├── src-tauri/                  # Rust 侧
│   ├── Cargo.toml              # 依赖 gateshift-core（path）+ tauri v2
│   ├── tauri.conf.json         # 窗口 / 权限 / 打包配置
│   ├── capabilities/default.json
│   └── src/
│       ├── main.rs             # 二进制入口，调用 lib::run()
│       ├── lib.rs              # 注册 State + 命令 = invoke_handler；托盘 + 开机自启 + 单实例（tray + autostart + single-instance）
│       ├── state.rs            # AppState：持有 NetworkManager 单例
│       └── commands.rs         # 10 个薄命令，全部委托给 NetworkManager
```

├── icons/                      # 由 scripts/make-icons.mjs 生成（含 tray/ 托盘模板图）
├── gateshift-core/             # vendoring 进来的核心库（路径依赖 ../gateshift-core）
├── .github/workflows/build.yml # GitHub Actions：macOS + Windows 自动构建
└── scripts/make-icons.mjs      # 图标流水线：从 scripts/*.svg 重新生成全部图标
```

## 命令 ↔ core 对照

| 前端动作 | Tauri 命令 | core 方法 | 是否弹授权 |
|---|---|---|---|
| 读状态 | `probe` | `manager.probe()` | 否 |
| 读配置 | `get_config` | `Config::default/load` | 否 |
| 存配置 | `save_config` | `set_config` + 落盘 | 否 |
| 切网关 | `switch_to` | `manager.switch_to()` | **是（一次）** |
| 只对齐 DNS | `align_dns` | `manager.align_dns()` | 是 |
| 恢复 DNS 自动 | `reset_dns_to_auto` | `manager.reset_dns_to_auto()` | 是 |
| 留底 | `snapshot` | `manager.snapshot()` | 否 |
| 还原 | `restore` | `manager.restore()` | 是（仅 DNS） |

## 运行 / 构建

```bash
cd gateshift-app
npm install              # 前端依赖
npm run tauri dev        # 开发：Vite + Rust 热重载
npm run tauri build      # 出包：.app / .msi / .exe
```

> 核心库通过 path 依赖引入：
> `gateshift-core = { path = "/Users/sthho/WorkBuddy/2026-10-05-11-00-16/dist/gateshift-core-0.1.0" }`
> 把它换成你机器上 core 的真实路径即可。macOS / Windows 为完整可用，Linux 上 core 走 `unsupported` 占位（UI 可用、网络能力待补）。

## 在 Windows 上构建与运行

`gateshift-core` 对 Windows **已实测完整支持**（README 记录 Windows 11 实测通过、37 项测试；
PowerShell `-EncodedCommand` 中文乱码、`Set-DnsClientServerAddress` 无 `-AddressFamily`、先加后删路由避免空窗期等坑都已避开），
所以 Windows 上所有网关 / DNS 操作都是真生效的。下面是从零到跑起来的步骤。

### 1. 前置依赖（一次性）

| 依赖 | 装法（示例） | 说明 |
|---|---|---|
| C++ 构建工具 (MSVC) | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` | 必须含「MSVC v143」+「Windows 10/11 SDK」 |
| Rust (stable, MSVC) | `winget install Rustlang.Rustup` → 默认就是 `stable-msvc` | `cargo` / `rustc` 可用即可 |
| Node.js LTS (≥18) | `winget install OpenJS.NodeJS.LTS` | 提供 `npm` |
| WebView2 运行时 | `winget install Microsoft.EdgeWebView2Runtime` | **Win11 已内置，可跳过**；Win10 需手动装，否则窗口起不来 |

> 可选：若 `npm run tauri` 报 PowerShell 脚本被拦截，执行一次
> `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`。

### 2. 拿代码 & 改依赖路径（关键）

✅ **core 已 vendoring 进本仓库**（`gateshift-core/` 目录，依赖为相对路径 `../gateshift-core`），
仓库自包含——Windows / CI 上 clone 后**无需再改任何路径**即可构建。
若以后你把 core 拆成独立仓库，把 `src-tauri/Cargo.toml` 里的 path 改成 git 依赖或 submodule 即可。

### 3. 放置位置（避开中文 / 网络盘）

把 `gateshift-app` 源码目录放到**纯 ASCII、本地盘**路径，例如 `C:\work\gateshift-app`。
Tauri / core 的 C++ 与 Rust 工具链都怕中文、空格、网络盘路径（core README 里 MinGW `ld` 的坑同样会咬 MSVC 构建）。

> **不要跨平台拷贝 `node_modules` 和 `src-tauri/target`**——它们含平台专属二进制，拷过去只会报错。
> 只拷源码目录，到 Windows 上重新 `npm install` + `cargo build` 即可（这俩目录已在 `.gitignore`）。

### 4. 安装 & 运行

```powershell
cd C:\work\gateshift-app
npm install                       # 若用 npm 11/12，需先批准安装脚本：
npm install-scripts approve esbuild   # （fsevents 是 macOS only，Windows 不装）
npm run tauri dev                # 开发预览
npm run tauri build              # 出包：.msi / .exe，在 src-tauri\target\release\bundle\
```

### 5. Windows 专属注意

- **UAC 授权**：切网关 / 对齐 DNS 会弹一次 UAC（core 用 `Start-Process -Verb RunAs`）。
  点「是」继续执行；点「否」会被 core 识别为「取消」，界面提示「未做任何修改」，不会报错。
- **签名**：正式分发建议配代码签名证书，否则 SmartScreen 拦截、UAC 提示更吓人。
  自己开发自测可先不签名（`tauri build` 默认或加 `--debug` 都能跑）。
- **crates.io 不通**：若被旁路由 fake-IP 接管导致 `cargo build` 拉不到依赖，在 `.cargo/config.toml` 配 ustc 等镜像（core 已带示例）。

## 托盘 / 驻留

- **托盘图标**：macOS 用 22pt 单色模板图（`icons/tray/tray-22.png`，系统自动按菜单栏明暗反色），
  Windows 用 32px（`tray-32.png`）；编译期 `include_bytes!` 嵌进二进制。
- **交互**：左键点托盘 = 显示/隐藏主窗口；右键 = 菜单（显示主窗口 / 退出 GateShift）。
- **关窗 ≠ 退出**：点红色关闭按钮只是隐藏到托盘，应用驻留后台（方便随时切网关）；
  真正退出走托盘菜单。

## GitHub Actions 自动构建

推送到 `main`/`master` 后，`.github/workflows/build.yml` 会自动在 macOS + Windows 上构建，
并把产物（`.dmg` / `.app` / `.msi` / `.exe`）上传为 Actions artifact，无需任何本机构建。
也可在 Actions 页面手动 Run workflow 触发。

- **签名**：默认产出未签名包（自测够用）。要正式分发，在仓库 Secrets 配好证书后，
  取消 workflow 里 macOS 签名相关 env 的注释即可。
- **注意**：`tauri.conf.json` 里 `bundle > createUpdaterArtifacts` 未开启时，
  `TAURI_SIGNING_PRIVATE_KEY` 这两个 env 可留空，不影响普通构建。

## 测试

- core 自身的 `testkit` 已通过 `gateshift-core` 的 `testkit` feature 复用。
- 应用命令层也接了它：`commands.rs` 末尾有一个集成测试（`switch_to` 命令确实只触发一次 apply），
  需要开启 app 的 `testkit` feature 才编译运行：

```bash
cd src-tauri
cargo test --features testkit
```

## 平台注意

- **macOS**：`tauri build` 需要开发者签名 + Notarization，否则 Gatekeeper 拦截（CI 产物下载后
  首次打开需右键 → 打开，或在「系统设置 → 隐私与安全性」里放行）。
- **Windows**：完整可用，详见上方「在 Windows 上构建与运行」。要点：配 MSVC 构建工具 + WebView2、
  放纯 ASCII 本地盘、自测可暂不签名（core 路径已 vendoring，无需再改）。
- 若 crates.io 直连不通（例如被旁路由 fake-IP 接管），可在 `.cargo/config.toml` 配置 ustc 等镜像。
