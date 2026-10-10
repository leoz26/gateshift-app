//! 自测入口 —— **不是产品 UI**，只是为了能在 macOS / Windows 上手动验一下核心。
//! 将来接上真正的界面后，这个文件可以直接删掉。
//!
//! ```bash
//! cargo run --example probe                    # 只读：接口 / 网关 / DNS / 档位
//! cargo run --example probe -- switch proxy    # 真切换（会弹系统授权）
//! cargo run --example probe -- switch main
//! cargo run --example probe -- align-dns       # 只对齐 DNS，不动网关
//! cargo run --example probe -- dns-auto        # 系统 DNS 恢复自动（DHCP）
//! cargo run --example probe -- snapshot        # 存一份快照
//! cargo run --example probe -- restore         # 还原上次快照
//! ```
//!
//! 配置默认读当前目录的 `config.json`（没有就用内置默认值），
//! 也可以用环境变量 `GATESHIFT_CONFIG` 指定路径。

use std::path::PathBuf;
use std::process::ExitCode;

use gateshift_core::{Config, NetworkManager, RouterMode, Snapshot};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    enable_utf8_console();

    let path = config_path();
    let config = if path.exists() {
        println!("配置文件：{}", path.display());
        Config::load_from_file(&path)?
    } else {
        println!("配置文件：{} 不存在，先用内置默认值", path.display());
        Config::default()
    };

    let manager = NetworkManager::new(config)?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None | Some("probe") => probe(&manager)?,
        Some("switch") => switch(&manager, args.get(1).map(String::as_str))?,
        Some("align-dns") => {
            let state = manager.align_dns()?;
            println!("已按当前档位对齐 DNS：{}", state.dns.describe());
        }
        Some("dns-auto") => {
            let state = manager.reset_dns_to_auto()?;
            println!("系统 DNS 已恢复为自动：{}", state.dns.describe());
        }
        Some("snapshot") => {
            let snap = manager.snapshot()?;
            let file = snapshot_path();
            std::fs::write(&file, serde_json::to_string_pretty(&snap)?)?;
            println!(
                "已存快照到 {}（接口 {}，DNS {}）",
                file.display(),
                snap.interface.name,
                snap.dns.describe()
            );
        }
        Some("restore") => {
            let file = snapshot_path();
            let snap: Snapshot = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
            manager.restore(&snap)?;
            println!("已还原快照 {}（DNS {}）", file.display(), snap.dns.describe());
        }
        Some(other) => {
            eprintln!("未知参数「{}」", other);
            help();
        }
    }
    Ok(())
}

fn probe(manager: &NetworkManager) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n--- 可用接口 ---");
    for iface in manager.list_interfaces()? {
        println!(
            "  {}{}{}",
            iface.name,
            if iface.device.is_some() || iface.index.is_some() {
                format!("（{}）", iface.describe())
            } else {
                String::new()
            },
            if iface.is_default { "   ← 默认" } else { "" }
        );
    }

    let state = manager.probe()?;
    println!("\n--- 当前状态 ---");
    println!(
        "默认接口 : {}{}",
        state.interface.name,
        if state.interface.device.is_some() || state.interface.index.is_some() {
            format!("（{}）", state.interface.describe())
        } else {
            String::new()
        }
    );
    println!(
        "默认网关 : {}",
        state.gateway.as_deref().unwrap_or("（读不到）")
    );
    println!("系统 DNS : {}", state.dns.describe());
    println!("匹配档位 : {}", state.mode.label());

    let cfg = manager.config();
    println!("\n--- 配置档位 ---");
    for (label, profile) in [
        ("主路由", &cfg.main_router),
        ("旁路由", &cfg.bypass_router),
    ] {
        println!(
            "{} : {} <{}>  DNS {}",
            label,
            profile.display_name(label),
            profile.gateway,
            profile.dns_text()
        );
    }
    println!(
        "切换时改 DNS : {}",
        if cfg.set_dns_on_switch { "是" } else { "否" }
    );
    Ok(())
}

fn switch(manager: &NetworkManager, arg: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mode = match arg {
        Some("main") | Some("主路由") => RouterMode::MainRouter,
        Some("proxy") | Some("bypass") | Some("旁路由") => RouterMode::BypassRouter,
        other => {
            eprintln!("用法：probe switch <main|proxy>，收到的是 {:?}", other);
            return Ok(());
        }
    };

    println!("正在切换……（会弹系统授权框）");
    let state = manager.switch_to(mode)?;
    println!(
        "已切到「{}」：网关 {} · DNS {}",
        state.mode.label(),
        state.gateway.as_deref().unwrap_or("（读不到）"),
        state.dns.describe()
    );
    Ok(())
}

fn config_path() -> PathBuf {
    if let Ok(custom) = std::env::var("GATESHIFT_CONFIG") {
        return PathBuf::from(custom);
    }
    let local = PathBuf::from("config.json");
    if local.exists() {
        return local;
    }
    Config::default_config_path().unwrap_or(local)
}

fn snapshot_path() -> PathBuf {
    let mut p = config_path();
    p.set_file_name("gateshift-snapshot.json");
    p
}

fn help() {
    println!(
        "gateshift-core 自测入口

  probe            只读：列出接口 / 网关 / DNS / 档位（默认）
  switch <档位>    切到 main 或 proxy（会弹系统授权）
  align-dns        只按当前所在档位对齐 DNS，不动网关
  dns-auto         系统 DNS 恢复为自动（DHCP）
  snapshot         把当前接口 + DNS 存到 gateshift-snapshot.json
  restore          从 gateshift-snapshot.json 还原 DNS"
    );
}

/// cmd.exe 默认是 GBK 代码页，中文输出会乱码。
#[cfg(target_os = "windows")]
fn enable_utf8_console() {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "chcp 65001 > nul"])
        .status();
}
