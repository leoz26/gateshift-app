//! Tauri 壳：只负责把 `NetworkManager` 接到窗口上，不写任何网络逻辑。
//! 所有业务都在 `gateshift-core` 里。

mod commands;
mod state;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use gateshift_core::RouterMode;

/// 托盘图标：macOS 用 22pt（模板图会自动适配 Retina 与明暗色），
/// 其余平台（Windows）用 32px。编译期直接嵌进二进制。
#[cfg(target_os = "macos")]
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray/tray-22.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray/tray-32.png");

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 退出前：若开启「还原主路由」且当前不在主路由，则切回主路由（需弹一次系统授权）。
/// 已是主路由就不弹授权，直接退出。任何失败只记录、不阻断退出。
fn restore_on_exit_if_needed(app: &tauri::AppHandle) {
    // 仅读取配置开关（语句级临时值，避免 State 与 MutexGuard 的借用纠缠）
    let restore = app.state::<state::AppState>().lock().config().restore_on_exit;
    if !restore {
        return;
    }
    // 先探测当前档位：已是主路由就别弹 UAC 了
    let mode = match app.state::<state::AppState>().lock().detect() {
        Ok(st) => st.mode,
        Err(e) => {
            eprintln!("[gateshift] 退出前探测失败，跳过还原：{e}");
            return;
        }
    };
    if mode == RouterMode::MainRouter {
        return;
    }
    if let Err(e) = app
        .state::<state::AppState>()
        .lock()
        .switch_to(RouterMode::MainRouter)
    {
        eprintln!("[gateshift] 退出前还原主路由失败（仍正常退出）：{e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 开机自启插件：macOS 默认用 LaunchAgent 写 ~/Library/LaunchAgents，
    // Windows 写注册表 Run 键。args 里的 `--hidden` 用于「开机自启时静默进托盘、不弹窗」。
    let autostart = tauri_plugin_autostart::Builder::new()
        .args(["--hidden"])
        .build();

    tauri::Builder::default()
        .manage(state::AppState::new())
        .plugin(autostart)
        // 单实例：第二个实例拉起时直接聚焦已存在的窗口（常驻托盘场景必备）。
        // 注意——带 --hidden 的开机自启实例是「第一个」，用户之后双击图标属于第二个，
        // 这里只负责把窗口唤到前台，不会重复初始化。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            commands::probe,
            commands::list_interfaces,
            commands::get_config,
            commands::save_config,
            commands::switch_to,
            commands::align_dns,
            commands::reset_dns_to_auto,
            commands::snapshot,
            commands::restore,
            commands::mode_of,
        ])
        .setup(|app| {
            // 若是「开机自启」拉起的（带了 --hidden），直接最小化进托盘，不弹主窗口。
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            // ---- 托盘菜单：显示主窗口 / 退出 ----
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 GateShift", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let icon = tauri::image::Image::from_bytes(TRAY_PNG)?;

            let tray = TrayIconBuilder::with_id("gateshift-tray")
                .icon(icon)
                .tooltip("GateShift")
                .menu(&menu)
                // 左键不弹菜单，留给「显示/隐藏窗口」切换；菜单走右键
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => {
                        restore_on_exit_if_needed(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            // 左键：可见则隐藏，否则唤起（驻留型应用的经典交互）
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                show_main_window(app);
                            }
                        }
                    }
                })
                .build(app)?;

            // macOS 菜单栏图标必须是「模板图」（单色，系统自动反色）
            #[cfg(target_os = "macos")]
            tray.set_icon_as_template(true)?;

            Ok(())
        })
        // 关窗 ≠ 退出：隐藏到托盘，驻留后台；真正退出走托盘菜单「退出 GateShift」
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running gateshift-app");
}
