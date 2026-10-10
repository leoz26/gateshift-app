import type { NetworkState } from "../lib/types";

const modeLabel: Record<string, string> = {
  main_router: "主路由",
  bypass_router: "旁路由",
  unknown: "未知",
};

export default function StatusCard({
  state,
  onRefresh,
}: {
  state: NetworkState | null;
  onRefresh: () => void;
}) {
  if (!state) {
    return <section className="card card--status">读取中…</section>;
  }
  const dns = state.dns.ipv4.length ? state.dns.ipv4.join(", ") : "自动（DHCP）";
  return (
    <section className="card card--status">
      <div className="status__mode">
        <span className={`badge badge--${state.mode}`}>{modeLabel[state.mode]}</span>
        <button
          className="btn btn--ghost status__refresh"
          onClick={onRefresh}
          title="重新探测当前网络状态"
        >
          刷新
        </button>
      </div>
      <dl className="status__grid">
        <dt>接口</dt>
        <dd>{state.interface.name}</dd>
        <dt>网关</dt>
        <dd>{state.gateway ?? "（读不到）"}</dd>
        <dt>DNS</dt>
        <dd>{dns}</dd>
      </dl>
    </section>
  );
}
