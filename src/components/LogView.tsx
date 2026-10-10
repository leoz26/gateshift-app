import type { LogEntry } from "../lib/types";

export default function LogView({ logs }: { logs: LogEntry[] }) {
  return (
    <section className="card card--log">
      <h3>日志</h3>
      {logs.length === 0 ? (
        <p className="muted">暂无操作</p>
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
