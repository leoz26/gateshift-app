// 对 Tauri `invoke` 的薄封装，每个函数对应后端一个命令。
// 参数名与后端 Rust 参数名一致（单单词，无大小写转换问题）。
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import type { Config, NetworkInterface, NetworkState, RouterMode, Snapshot } from "./types";

export const probe = () => invoke<NetworkState>("probe");
export const listInterfaces = () => invoke<NetworkInterface[]>("list_interfaces");
export const getConfig = () => invoke<Config>("get_config");
export const saveConfig = (config: Config) => invoke<void>("save_config", { config });
export const switchTo = (mode: RouterMode) =>
  invoke<NetworkState>("switch_to", { mode });
export const alignDns = () => invoke<NetworkState>("align_dns");
export const resetDnsToAuto = () => invoke<NetworkState>("reset_dns_to_auto");
export const snapshot = () => invoke<Snapshot>("snapshot");
export const restore = (snapshot: Snapshot) =>
  invoke<void>("restore", { snapshot });
export const modeOf = (gateway: string | null) =>
  invoke<RouterMode>("mode_of", { gateway });

// ---- 开机自启（tauri-plugin-autostart）----
// 注意：开发模式（tauri dev）或非打包环境下 enable/isEnabled 可能抛错，
// 调用方需做好 try/catch；生产打包的 .app/.msi 下完全可用。
export const getAutostartEnabled = () => isEnabled();
export const setAutostart = (on: boolean) => (on ? enable() : disable());
