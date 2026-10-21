// 与 gateshift-core 模型一一对应的前端类型。
// 字段名、枚举值（snake_case）都和 core 的 serde 输出保持一致，invoke 直接能反序列化。

export type RouterMode = "main_router" | "bypass_router" | "unknown";

export interface NetworkInterface {
  name: string;
  index?: number | null;
  device?: string | null;
  is_default: boolean;
}

export interface DnsConfig {
  ipv4: string[];
}

export interface NetworkState {
  interface: NetworkInterface;
  gateway: string | null;
  dns: DnsConfig;
  mode: RouterMode;
  /**
   * DNS 是否真的读到了。`false` 表示 `dns` 只是占位空列表（读不到），
   * 典型场景是 VPN 连接期间：`utun*` 接口的 DNS 不受系统管理，
   * 此时「对齐 DNS」「恢复自动」都会返回 `dns_unmanageable` 错误。
   *
   * 旧后端不会带这个字段 → 可选，缺省按 `true` 处理。
   */
  dns_readable?: boolean;
}

export interface RouterProfile {
  name: string;
  gateway: string;
  dns: string[];
}

export interface Config {
  set_dns_on_switch: boolean;
  restore_on_exit: boolean;
  main_router: RouterProfile;
  bypass_router: RouterProfile;
}

export interface Snapshot {
  interface: NetworkInterface;
  /**
   * 快照时刻的默认网关。core 里是 `Option<String>`，
   * 且「未知/没取到」时 serde 会直接省略该字段（`skip_serializing_if`），
   * 所以这里必须允许 `null` 与 `undefined`，并且**必须原样回传给 `restore`**——
   * 少这一个字段，还原时就只还原 DNS、网关仍指向旁路由。
   */
  gateway?: string | null;
  dns: DnsConfig;
}

export interface LogEntry {
  time: string;
  level: "info" | "error" | "warn";
  msg: string;
}
