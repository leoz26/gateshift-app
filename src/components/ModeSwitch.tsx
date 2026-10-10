import type { RouterMode } from "../lib/types";

export default function ModeSwitch({
  current,
  busy,
  onPick,
}: {
  current: RouterMode;
  busy: boolean;
  onPick: (m: RouterMode) => void;
}) {
  return (
    <section className="card card--modes">
      <button
        className={`mode-btn ${current === "main_router" ? "is-active" : ""}`}
        disabled={busy}
        onClick={() => onPick("main_router")}
      >
        {current === "main_router" && <span className="mode-btn__tag">当前</span>}
        <span className="mode-btn__title">主路由</span>
        <span className="mode-btn__hint">直连，不走代理</span>
      </button>
      <button
        className={`mode-btn ${current === "bypass_router" ? "is-active" : ""}`}
        disabled={busy}
        onClick={() => onPick("bypass_router")}
      >
        {current === "bypass_router" && <span className="mode-btn__tag">当前</span>}
        <span className="mode-btn__title">旁路由</span>
        <span className="mode-btn__hint">走代理 / 透明网关</span>
      </button>
    </section>
  );
}
