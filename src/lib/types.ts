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
  dns: DnsConfig;
}

export interface LogEntry {
  time: string;
  level: "info" | "error" | "warn";
  msg: string;
}
