import type { NetworkState } from "../lib/types";
import { IconGateway, IconGlobe, IconNic, IconRefresh } from "./Icons";

const modeLabel: Record<string, string> = {
  main_router: "主路由",
  bypass_router: "旁路由",
  unknown: "未知",
};

// 一句话解释当前档位意味着什么，降低"看不懂状态"的成本。
const modeHint: Record<string, string> = {
  main_router: "直连出口，未经过旁路由",
  bypass_router: "流量经旁路由转发（代理 / 透明网关）",
  unknown: "网关与任一档位都不匹配，或尚未探测成功",
};

export default function StatusCard({
  state,
  onRefresh,
}: {
  state: NetworkState | null;
  onRefresh: () => void;
}) {
  if (!state) {
    return (
      <section
        className="card card--status card--status-unknown"
        aria-busy="true"
        aria-live="polite"
      >
        <div className="status__head">
          <span className="badge badge--unknown badge--pulse">
            <span className="badge__dot" aria-hidden="true" />
            探测中…
          </span>
          <button
            type="button"
            className="status__refresh"
            onClick={onRefresh}
            title="重新探测当前网络状态"
          >
            <IconRefresh />
            刷新
          </button>
        </div>
        <div className="status__skeleton" aria-hidden="true">
          <span className="skeleton" />
          <span className="skeleton" />
          <span className="skeleton" />
        </div>
      </section>
    );
  }

  // 「读不到 DNS」和「DNS 真的是自动(DHCP)」是两件事：
  // dns_readable === false 时 dns.ipv4 只是占位空列表，显示成「自动」会误导。
  const dnsUnreadable = state.dns_readable === false;
  const isAuto = !dnsUnreadable && state.dns.ipv4.length === 0;
  const dns = dnsUnreadable
    ? "不受管理"
    : isAuto
      ? "自动（DHCP）"
      : state.dns.ipv4.join(", ");

  return (
    <section className={`card card--status card--status-${state.mode}`}>
      <div className="status__head">
        <span className={`badge badge--${state.mode}`}>
          <span className="badge__dot" aria-hidden="true" />
          {modeLabel[state.mode]}
        </span>
        <button
          type="button"
          className="status__refresh"
          onClick={onRefresh}
          title="重新探测当前网络状态"
        >
          <IconRefresh />
          刷新
        </button>
      </div>
      <p className="status__hint">{modeHint[state.mode]}</p>
      <dl className="status__grid">
        <div className="status__item">
          <dt>
            <IconGateway />
            网关
          </dt>
          <dd className="mono">{state.gateway ?? "（读不到）"}</dd>
        </div>
        <div className="status__item">
          <dt>
            <IconGlobe />
            DNS
          </dt>
          <dd
            className={`mono ${isAuto ? "status__value--auto" : ""} ${
              dnsUnreadable ? "status__value--muted" : ""
            }`}
            title={
              dnsUnreadable
                ? "当前接口的 DNS 不受系统管理（常见于 VPN 连接期间），既读不到也改不了"
                : undefined
            }
          >
            {dns}
          </dd>
        </div>
        <div className="status__item">
          <dt>
            <IconNic />
            接口
          </dt>
          <dd className="mono">{state.interface.name}</dd>
        </div>
      </dl>
      {dnsUnreadable && (
        <p className="status__note">
          DNS 读不到也改不了：这个接口不归系统管（常见于 VPN 连接期间）。切换档位仍会正常切网关。
        </p>
      )}
    </section>
  );
}
