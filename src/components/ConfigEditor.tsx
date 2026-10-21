import { useEffect, useRef, useState } from "react";
import type { Config, RouterProfile } from "../lib/types";
import { isIpv4 } from "../lib/ipv4";
import { IconSave } from "./Icons";

function ProfileForm({
  title,
  tone,
  profile,
  onChange,
}: {
  title: string;
  tone: "main" | "bypass";
  profile: RouterProfile;
  onChange: (p: RouterProfile) => void;
}) {
  return (
    <div className={`profile profile--${tone}`}>
      <h4>{title}</h4>
      <label>
        网关
        <input
          value={profile.gateway}
          inputMode="decimal"
          spellCheck={false}
          onChange={(e) => onChange({ ...profile, gateway: e.target.value })}
        />
      </label>
      <label>
        DNS（逗号分隔，留空=自动）
        <input
          value={profile.dns.join(", ")}
          placeholder="223.5.5.5, 8.8.8.8"
          inputMode="decimal"
          spellCheck={false}
          onChange={(e) =>
            onChange({
              ...profile,
              dns: e.target.value
                .split(",")
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
        />
      </label>
    </div>
  );
}

function sameConfig(a: Config, b: Config): boolean {
  // 配置是很小的纯数据对象，结构比较足够（不引入额外依赖）
  return JSON.stringify(a) === JSON.stringify(b);
}

export default function ConfigEditor({
  config,
  onSave,
}: {
  config: Config;
  onSave: (c: Config) => void;
}) {
  const [draft, setDraft] = useState<Config>(config);
  const [error, setError] = useState<string | null>(null);
  // 草稿是否被用户改过。脏了就不让 props 覆盖它（用户正在输入的网关不能突然被换掉）；
  // 干净时必须跟着 props 走 —— 否则 App 里刚改的开关（如 restore_on_exit）会被这份
  // 陈旧草稿在下次保存时覆盖回去。
  const [dirty, setDirty] = useState(false);
  // props 变了但草稿是脏的：既不能丢用户的输入，也不能假装没变化，提示用户自己选。
  const [stale, setStale] = useState(false);
  // 上一次看到的 props 配置：用来区分「props 变了」和「只是用户在改草稿」，
  // 否则用户每敲一个字都会被判成「配置已在别处更新」。
  const lastConfigRef = useRef(config);

  useEffect(() => {
    const configChanged = lastConfigRef.current !== config;
    lastConfigRef.current = config;

    if (!dirty) {
      // 草稿干净：永远跟随最新配置（这里是历史上那个「旧草稿覆盖回开关」bug 的修复点）
      if (configChanged || !sameConfig(draft, config)) setDraft(config);
      setStale(false);
      return;
    }
    // 用户正在编辑，props 又没变：什么都不做
    if (!configChanged) return;
    if (sameConfig(draft, config)) {
      // 脏草稿与最新配置已经一致 = 这次改动被采纳了（保存成功），脏标记归零
      setDirty(false);
      setStale(false);
    } else {
      setStale(true);
    }
  }, [config, dirty, draft]);

  const edit = (next: Config) => {
    setDraft(next);
    setDirty(true);
    // 上一次的错误是针对旧草稿说的，留着会误导
    setError(null);
  };

  const reload = () => {
    setDraft(config);
    setDirty(false);
    setStale(false);
    setError(null);
  };

  // 常见误输入给一句具体提示，别让用户对着「不是合法 IPv4」发呆。
  // 目前只补「前导零」这一种（core 也拒绝它，且最容易写错）。
  const ipv4Hint = (raw: string): string =>
    /^[0-9.]+$/.test(raw) && /(^|\.)0[0-9]/.test(raw)
      ? "（不能有前导零，如 192.168.01.1 应写成 192.168.1.1）"
      : "";

  const validate = (c: Config): string | null => {
    const rows: [string, RouterProfile][] = [
      ["主路由", c.main_router],
      ["旁路由", c.bypass_router],
    ];
    for (const [label, p] of rows) {
      if (!isIpv4(p.gateway)) return `${label}网关不是合法 IPv4${ipv4Hint(p.gateway)}`;
      for (const d of p.dns) {
        if (!isIpv4(d)) return `${label}DNS「${d}」不是合法 IPv4${ipv4Hint(d)}`;
      }
    }
    if (c.main_router.gateway === c.bypass_router.gateway) {
      return "主路由与旁路由网关不能相同";
    }
    return null;
  };

  const handleSave = () => {
    const err = validate(draft);
    if (err) {
      setError(err);
      return;
    }
    setError(null);
    onSave(draft);
  };

  return (
    <section className="card card--config">
      <div className="card__head">
        <h3>档位配置</h3>
      </div>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={draft.set_dns_on_switch}
          onChange={(e) => edit({ ...draft, set_dns_on_switch: e.target.checked })}
        />
        切换网关时同时设置 DNS
      </label>
      <div className="config__profiles">
        <ProfileForm
          title="主路由"
          tone="main"
          profile={draft.main_router}
          onChange={(p) => edit({ ...draft, main_router: p })}
        />
        <ProfileForm
          title="旁路由"
          tone="bypass"
          profile={draft.bypass_router}
          onChange={(p) => edit({ ...draft, bypass_router: p })}
        />
      </div>
      <div className="config__footer">
        {error ? (
          <p className="error-text" role="alert">
            {error}
          </p>
        ) : stale ? (
          // 草稿是脏的，而外部（偏好设置里的开关等）又改了配置：不偷偷覆盖任何一边，
          // 把选择权交给用户。
          <span className="config__hint muted">
            配置已在别处更新，当前草稿仍以你的修改为准
            <button type="button" className="btn btn--ghost btn--sm" onClick={reload}>
              载入最新配置
            </button>
          </span>
        ) : (
          <span className="config__hint muted">修改后点保存才会生效</span>
        )}
        <button type="button" className="btn btn--primary" onClick={handleSave}>
          <IconSave size={15} />
          保存配置
        </button>
      </div>
    </section>
  );
}
