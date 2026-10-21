import type { LogEntry } from "../lib/types";

export default function LogView({ logs }: { logs: LogEntry[] }) {
  return (
    <section className="card card--log">
      <div className="card__head">
        <h3>操作日志</h3>
        {logs.length > 0 && <span className="chip">{logs.length}</span>}
      </div>
      {logs.length === 0 ? (
        <p className="muted log__empty">暂无操作</p>
      ) : (
        <ul className="log">
          {logs.map((l, i) => (
            <li key={i} className={`log__item log__item--${l.level}`}>
              <span className="log__time">{l.time}</span>
              <span className="log__msg">{l.msg}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
