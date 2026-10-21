import { useCallback, useEffect, useRef, useState } from "react";
import type { Config, LogEntry, NetworkState, RouterMode, Snapshot } from "./lib/types";
import {
  alignDns,
  getAutostartEnabled,
  getConfig,
  probe,
  resetDnsToAuto,
  restore,
  saveConfig,
  setAutostart,
  snapshot as takeSnapshot,
  switchTo,
} from "./lib/api";
import { errorMessage, isElevationCancelled } from "./lib/errors";
import StatusCard from "./components/StatusCard";
import ModeSwitch from "./components/ModeSwitch";
import ConfigEditor from "./components/ConfigEditor";
import LogView from "./components/LogView";
import {
  IconAlert,
  IconArchive,
  IconLogo,
  IconRefresh,
  IconRestore,
  IconUndo,
} from "./components/Icons";

export default function App() {
  const [state, setState] = useState<NetworkState | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [busy, setBusy] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [pendingMode, setPendingMode] = useState<RouterMode | null>(null);
  const [lastSnapshot, setLastSnapshot] = useState<Snapshot | null>(null);
  const [autostart, setAutostartOn] = useState<boolean | null>(null);

  const pushLog = useCallback((level: LogEntry["level"], msg: string) => {
    setLogs((prev) => [{ time: new Date().toLocaleTimeString(), level, msg }, ...prev].slice(0, 200));
  }, []);

  // 写操作的失败提示统一走这里。
  // 用户取消授权不算失败——系统什么都没改，记一条 info 让用户知道「没动」就够了，
  // 判断依据是后端给的错误码（`elevation_cancelled`），不是中文文案。
  const reportFailure = useCallback(
    (action: string, e: unknown) => {
      if (isElevationCancelled(e)) {
        pushLog("info", "已取消授权，未做任何修改");
        return;
      }
      pushLog("error", `${action}失败：${errorMessage(e)}`);
    },
    [pushLog]
  );

  // 加载当前开机自启状态（开发模式可能不可用时静默置为 null）。
  useEffect(() => {
    getAutostartEnabled()
      .then(setAutostartOn)
      .catch(() => setAutostartOn(null));
  }, []);

  const handleToggleAutostart = useCallback(
    async (on: boolean) => {
      try {
        await setAutostart(on);
        setAutostartOn(on);
        pushLog("info", on ? "已开启开机自启" : "已关闭开机自启");
      } catch (e) {
        pushLog("error", `设置开机自启失败：${errorMessage(e)}`);
      }
    },
    [pushLog]
  );

  const refresh = useCallback(async () => {
    try {
      setState(await probe());
    } catch (e) {
      pushLog("error", `读取网络状态失败：${errorMessage(e)}`);
    }
  }, [pushLog]);

  const refreshAll = useCallback(async () => {
    try {
      const [st, cfg] = await Promise.all([probe(), getConfig()]);
      setState(st);
      setConfig(cfg);
    } catch (e) {
      pushLog("error", `初始化失败：${errorMessage(e)}`);
    }
  }, [pushLog]);

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  // 窗口重新聚焦时自动刷新状态（外部改动网络后回到窗口即所见即所得）。
  //
  // 两个节流点：
  // - 防抖 400ms：切窗口时 focus/blur 可能连发、还会和点击同时发生，去抖后只探测一次；
  // - `busy` 期间整段跳过：提权弹窗（Touch ID / UAC）会让窗口失去焦点又拿回来，
  //   此时插进来的一次探测可能读到「命令执行到一半」的中间态，把刚写好的状态覆盖掉。
  useEffect(() => {
    if (busy) return;
    let timer: number | undefined;
    const onFocus = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = undefined;
        refresh();
      }, 400);
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [busy, refresh]);

  const handleSwitch = useCallback(
    async (mode: RouterMode) => {
      setPendingMode(null);
      setBusy(true);
      const label = mode === "bypass_router" ? "旁路由" : "主路由";
      try {
        setState(await switchTo(mode));
        pushLog("info", `已切换到「${label}」`);
      } catch (e) {
        reportFailure(`切换到「${label}」`, e);
      } finally {
        setBusy(false);
      }
    },
    [pushLog, reportFailure]
  );

  const handleAlign = useCallback(async () => {
    setBusy(true);
    try {
      setState(await alignDns());
      pushLog("info", "已按当前档位对齐 DNS");
    } catch (e) {
      reportFailure("对齐 DNS", e);
    } finally {
      setBusy(false);
    }
  }, [pushLog, reportFailure]);

  const handleResetDns = useCallback(async () => {
    setBusy(true);
    try {
      setState(await resetDnsToAuto());
      pushLog("info", "已将 DNS 恢复为自动（DHCP）");
    } catch (e) {
      reportFailure("恢复 DNS", e);
    } finally {
      setBusy(false);
    }
  }, [pushLog, reportFailure]);

  const handleSaveConfig = useCallback(
    async (cfg: Config) => {
      try {
        await saveConfig(cfg);
        setConfig(cfg);
        pushLog("info", "配置已保存");
        await refresh();
      } catch (e) {
        reportFailure("保存配置", e);
      }
    },
    [pushLog, refresh, reportFailure]
  );

  const handleSnapshot = useCallback(async () => {
    try {
      const snap = await takeSnapshot();
      setLastSnapshot(snap);
      // 快照现在同时记下网关与 DNS（只记 DNS 的还原等于半个还原），日志里一并显示便于核对
      pushLog(
        "info",
        `已留底（接口 ${snap.interface.name}，网关 ${snap.gateway ?? "未知（还原时不动网关）"}，DNS ${
          snap.dns.ipv4.join(", ") || "自动"
        }）`
      );
    } catch (e) {
      reportFailure("留底", e);
    }
  }, [pushLog, reportFailure]);

  const handleRestore = useCallback(async () => {
    if (!lastSnapshot) {
      pushLog("warn", "还没有留底，先点「留底」再还原");
      return;
    }
    setBusy(true);
    try {
      await restore(lastSnapshot);
      pushLog("info", "已还原到留底状态");
      await refresh();
    } catch (e) {
      reportFailure("还原", e);
    } finally {
      setBusy(false);
    }
  }, [lastSnapshot, pushLog, refresh, reportFailure]);

  // 点档位卡片：当前档位只提示、不弹确认；其他档位走确认弹窗。
  const handlePick = useCallback(
    (m: RouterMode) => {
      if (state?.mode === m) {
        pushLog("info", `当前已处于「${m === "bypass_router" ? "旁路由" : "主路由"}」档位`);
        return;
      }
      setPendingMode(m);
    },
    [state?.mode, pushLog]
  );

  // 确认弹窗的键盘可达性：Esc 关闭、打开时焦点进弹窗、Tab 在弹窗内循环、
  // 关闭后焦点还给触发它的按钮。集中在一个 effect 里，不引入第三方库。
  const confirmDialogRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!pendingMode) return;
    const dialog = confirmDialogRef.current;
    // 打开前焦点所在的元素（档位卡片），关闭时还回去
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const focusables = (): HTMLElement[] => {
      if (!dialog) return [];
      const nodes = dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      );
      return Array.from(nodes).filter((el) => el.offsetParent !== null);
    };

    // 打开时把焦点移进弹窗：否则焦点还停在底下的档位卡片上，Tab 会跑到弹窗外的控件
    (focusables()[0] ?? dialog)?.focus();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setPendingMode(null);
        return;
      }
      if (e.key !== "Tab") return;
      const items = focusables();
      if (items.length === 0) {
        // 弹窗里没有可聚焦项时，别让 Tab 把焦点漏到背后的页面上
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      // 焦点不在弹窗内（例如点了遮罩、或确认按钮在 busy 时被禁用）就拉回弹窗
      if (!active || !dialog?.contains(active)) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
        return;
      }
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      // 关闭后把焦点还给触发它的按钮，键盘用户不会「掉回页面开头」
      if (opener && document.contains(opener)) opener.focus();
    };
  }, [pendingMode]);

  // DNS 不受系统管理（典型：VPN 连接期间的 utun 接口）。此时后端的
  // 「对齐 DNS」「恢复 DNS 为自动」会直接返回 dns_unmanageable，
  // 所以这两个按钮提前禁用，别让用户点了才拿到报错。
  const dnsLocked = state?.dns_readable === false;

  return (
    <div className="app">
      <header className="app__header">
        <div className="brand">
          <span className="brand__mark" aria-hidden="true">
            <IconLogo />
          </span>
          <div className="brand__text">
            <h1>GateShift</h1>
            <p>默认网关 / 系统 DNS 一键切换</p>
          </div>
        </div>
      </header>

      <StatusCard state={state} onRefresh={refresh} />

      <ModeSwitch current={state?.mode ?? "unknown"} busy={busy} onPick={handlePick} />

      <section className="panel">
        <div className="panel__head">
          <h3 className="panel__title">快捷操作</h3>
          <p className="panel__desc">按当前档位的配置对齐 DNS，或把 DNS 交还给 DHCP</p>
          {dnsLocked && (
            <p className="panel__desc panel__desc--note" id="dns-locked-note">
              当前接口的 DNS 不受系统管理（常见于 VPN 连接期间），这两项已禁用
            </p>
          )}
        </div>
        <div className="panel__row">
          <button
            type="button"
            className="btn btn--soft"
            disabled={busy || dnsLocked}
            aria-describedby={dnsLocked ? "dns-locked-note" : undefined}
            onClick={handleAlign}
            title={
              dnsLocked
                ? "当前网络接口的 DNS 不受系统管理（常见于 VPN 连接期间），无法通过 GateShift 修改"
                : "把 DNS 对齐到当前档位配置的值"
            }
          >
            <IconRefresh />
            只对齐 DNS
          </button>
          <button
            type="button"
            className="btn btn--danger"
            disabled={busy || dnsLocked}
            aria-describedby={dnsLocked ? "dns-locked-note" : undefined}
            onClick={handleResetDns}
            title={
              dnsLocked
                ? "当前网络接口的 DNS 不受系统管理（常见于 VPN 连接期间），无法通过 GateShift 修改"
                : "把系统 DNS 改回「自动（DHCP）」"
            }
          >
            <IconUndo />
            恢复 DNS 为自动
          </button>
          <button type="button" className="btn btn--ghost" disabled={busy} onClick={handleSnapshot}>
            <IconArchive />
            留底
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            disabled={busy || !lastSnapshot}
            onClick={handleRestore}
            title={lastSnapshot ? "还原到留底时的状态" : "还没有留底，先点「留底」"}
          >
            <IconRestore />
            还原留底
          </button>
        </div>
      </section>

      <section className="panel panel--settings">
        <div className="panel__head">
          <h3 className="panel__title">偏好设置</h3>
        </div>
        <div className="setting-row">
          <div className="setting-row__text">
            <span className="setting-row__title">开机自启</span>
            <span className="setting-row__hint">
              {autostart === null
                ? "当前环境不可用（请使用打包后的 .app / .msi）"
                : "登录系统后自动在后台运行（进托盘，不弹窗）"}
            </span>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              aria-label="开机自启"
              checked={autostart === true}
              disabled={autostart === null}
              onChange={(e) => handleToggleAutostart(e.target.checked)}
            />
            <span className="switch__slider" />
          </label>
        </div>

        {config && (
          <div className="setting-row">
            <div className="setting-row__text">
              <span className="setting-row__title">退出时还原主路由</span>
              <span className="setting-row__hint">
                退出软件时自动把网关切回「主路由」（需弹一次授权）
              </span>
            </div>
            <label className="switch">
              <input
                type="checkbox"
                aria-label="退出时还原主路由"
                checked={config.restore_on_exit}
                onChange={(e) =>
                  handleSaveConfig({ ...config, restore_on_exit: e.target.checked })
                }
              />
              <span className="switch__slider" />
            </label>
          </div>
        )}
      </section>

      {config && <ConfigEditor config={config} onSave={handleSaveConfig} />}

      <LogView logs={logs} />

      {pendingMode && (
        <div className="modal" onClick={() => setPendingMode(null)}>
          <div
            className="modal__card"
            ref={confirmDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="confirm-title"
            aria-describedby="confirm-desc"
            // 让卡片本身可被程序化聚焦（弹窗内暂时没有可聚焦元素时的兜底落点）
            tabIndex={-1}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal__head">
              <span className="modal__icon" aria-hidden="true">
                <IconAlert />
              </span>
              <span className={`badge badge--${pendingMode} badge--lg`}>
                <span className="badge__dot" aria-hidden="true" />
                {pendingMode === "bypass_router" ? "旁路由" : "主路由"}
              </span>
            </div>
            <h3 id="confirm-title">确认切换档位？</h3>
            <p id="confirm-desc">
              系统会弹出一次授权（Touch ID / 密码 / UAC），允许后才会修改网络设置。
            </p>
            {config && (
              <dl className="modal__summary">
                <div className="modal__row">
                  <dt>网关</dt>
                  <dd>
                    {(pendingMode === "bypass_router"
                      ? config.bypass_router
                      : config.main_router
                    ).gateway || "（未配置）"}
                  </dd>
                </div>
                <div className="modal__row">
                  <dt>DNS</dt>
                  <dd>
                    {dnsLocked ? (
                      <span className="status__value--muted">跳过（DNS 不受管理）</span>
                    ) : config.set_dns_on_switch ? (
                      (pendingMode === "bypass_router"
                        ? config.bypass_router
                        : config.main_router
                      ).dns.join(", ") || "自动（DHCP）"
                    ) : (
                      "不修改"
                    )}
                  </dd>
                </div>
              </dl>
            )}
            <div className="modal__actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => setPendingMode(null)}
              >
                取消
              </button>
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy}
                onClick={() => handleSwitch(pendingMode)}
              >
                确认切换
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
