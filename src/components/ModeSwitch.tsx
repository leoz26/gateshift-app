import type { RouterMode } from "../lib/types";
import { IconRoute, IconShield } from "./Icons";

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
      <div className="modes__head">
        <h3>切换默认网关</h3>
        {busy && (
          <span className="modes__busy" role="status">
            <span className="spinner" aria-hidden="true" />
            处理中…
          </span>
        )}
      </div>
      <div className="modes__grid">
        <button
          type="button"
          className={`mode-btn mode-btn--main ${
            current === "main_router" ? "is-active" : ""
          }`}
          aria-pressed={current === "main_router"}
          disabled={busy}
          onClick={() => onPick("main_router")}
        >
          <span className="mode-btn__icon" aria-hidden="true">
            <IconShield size={19} />
          </span>
          <span className="mode-btn__body">
            <span className="mode-btn__title">
              主路由
              {current === "main_router" && (
                <span className="mode-btn__tag">当前</span>
              )}
            </span>
            <span className="mode-btn__hint">直连，不走代理</span>
          </span>
        </button>

        <button
          type="button"
          className={`mode-btn mode-btn--bypass ${
            current === "bypass_router" ? "is-active" : ""
          }`}
          aria-pressed={current === "bypass_router"}
          disabled={busy}
          onClick={() => onPick("bypass_router")}
        >
          <span className="mode-btn__icon" aria-hidden="true">
            <IconRoute size={19} />
          </span>
          <span className="mode-btn__body">
            <span className="mode-btn__title">
              旁路由
              {current === "bypass_router" && (
                <span className="mode-btn__tag">当前</span>
              )}
            </span>
            <span className="mode-btn__hint">走代理 / 透明网关</span>
          </span>
        </button>
      </div>
    </section>
  );
}
