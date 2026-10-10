import { useCallback, useEffect, useState } from "react";
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
import StatusCard from "./components/StatusCard";
import ModeSwitch from "./components/ModeSwitch";
import ConfigEditor from "./components/ConfigEditor";
import LogView from "./components/LogView";

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
        pushLog("error", `设置开机自启失败：${(e as Error).message}`);
      }
    },
    [pushLog]
  );

  const refresh = useCallback(async () => {
    try {
      setState(await probe());
    } catch (e) {
      pushLog("error", `读取网络状态失败：${(e as Error).message}`);
    }
  }, [pushLog]);

  const refreshAll = useCallback(async () => {
    try {
      const [st, cfg] = await Promise.all([probe(), getConfig()]);
      setState(st);
      setConfig(cfg);
    } catch (e) {
      pushLog("error", `初始化失败：${(e as Error).message}`);
    }
  }, [pushLog]);

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  // 窗口重新聚焦时自动刷新状态（外部改动网络后回到窗口即所见即所得）。
  useEffect(() => {
    const onFocus = () => {
      refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const handleSwitch = useCallback(
    async (mode: RouterMode) => {
      setPendingMode(null);
      setBusy(true);
      const label = mode === "bypass_router" ? "旁路由" : "主路由";
      try {
        setState(await switchTo(mode));
        pushLog("info", `已切换到「${label}」`);
      } catch (e) {
        const m = (e as Error).message;
        if (m.includes("取消")) {
          pushLog("info", "已取消授权，未做任何修改");
        } else {
          pushLog("error", `切换到「${label}」失败：${m}`);
        }
      } finally {
        setBusy(false);
      }
    },
    [pushLog]
  );

  const handleAlign = useCallback(async () => {
    setBusy(true);
    try {
      setState(await alignDns());
      pushLog("info", "已按当前档位对齐 DNS");
    } catch (e) {
      pushLog("error", `对齐 DNS 失败：${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  }, [pushLog]);

  const handleResetDns = useCallback(async () => {
    setBusy(true);
    try {
      setState(await resetDnsToAuto());
      pushLog("info", "已将 DNS 恢复为自动（DHCP）");
    } catch (e) {
      pushLog("error", `恢复 DNS 失败：${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  }, [pushLog]);

  const handleSaveConfig = useCallback(
    async (cfg: Config) => {
      try {
        await saveConfig(cfg);
        setConfig(cfg);
        pushLog("info", "配置已保存");
        await refresh();
      } catch (e) {
        pushLog("error", `保存配置失败：${(e as Error).message}`);
      }
    },
    [pushLog, refresh]
  );

  const handleSnapshot = useCallback(async () => {
    try {
      const snap = await takeSnapshot();
      setLastSnapshot(snap);
      pushLog("info", `已留底（接口 ${snap.interface.name}，DNS ${snap.dns.ipv4.join(", ") || "自动"}）`);
    } catch (e) {
      pushLog("error", `留底失败：${(e as Error).message}`);
    }
  }, [pushLog]);

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
      pushLog("error", `还原失败：${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  }, [lastSnapshot, pushLog, refresh]);

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

  return (
    <div className="app">
      <header className="app__header">
        <h1>GateShift</h1>
        <p>默认网关 / 系统 DNS 一键切换</p>
      </header>

      <StatusCard state={state} onRefresh={refresh} />

      <ModeSwitch current={state?.mode ?? "unknown"} busy={busy} onPick={handlePick} />

      <section className="panel">
        <div className="panel__row">
          <button className="btn" disabled={busy} onClick={handleAlign}>
            只对齐 DNS
          </button>
          <button className="btn" disabled={busy} onClick={handleResetDns}>
            恢复 DNS 为自动
          </button>
          <button className="btn btn--ghost" disabled={busy} onClick={handleSnapshot}>
            留底
          </button>
          <button
            className="btn btn--ghost"
            disabled={busy || !lastSnapshot}
            onClick={handleRestore}
          >
            还原留底
          </button>
        </div>
      </section>

      <section className="panel">
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
              checked={autostart === true}
              disabled={autostart === null}
              onChange={(e) => handleToggleAutostart(e.target.checked)}
            />
            <span className="switch__slider" />
          </label>
        </div>
      </section>

      {config && <ConfigEditor config={config} onSave={handleSaveConfig} />}

      <LogView logs={logs} />

      {pendingMode && (
        <div className="modal" onClick={() => setPendingMode(null)}>
          <div className="modal__card" onClick={(e) => e.stopPropagation()}>
            <h3>确认切换？</h3>
            <p>
              即将切换到「{pendingMode === "bypass_router" ? "旁路由" : "主路由"}」，系统会弹出一次授权
              （Touch ID / 密码 / UAC），请允许。
            </p>
            <div className="modal__actions">
              <button className="btn btn--ghost" onClick={() => setPendingMode(null)}>
                取消
              </button>
              <button
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
