import { useState } from "react";
import type { Config, RouterProfile } from "../lib/types";

function isIpv4(s: string): boolean {
  return (
    /^(\d{1,3}\.){3}\d{1,3}$/.test(s) && s.split(".").every((n) => Number(n) <= 255)
  );
}

function ProfileForm({
  title,
  profile,
  onChange,
}: {
  title: string;
  profile: RouterProfile;
  onChange: (p: RouterProfile) => void;
}) {
  return (
    <div className="profile">
      <h4>{title}</h4>
      <label>
        网关
        <input
          value={profile.gateway}
          onChange={(e) => onChange({ ...profile, gateway: e.target.value })}
        />
      </label>
      <label>
        DNS（逗号分隔，留空=自动）
        <input
          value={profile.dns.join(", ")}
          placeholder="223.5.5.5, 8.8.8.8"
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

export default function ConfigEditor({
  config,
  onSave,
}: {
  config: Config;
  onSave: (c: Config) => void;
}) {
  const [draft, setDraft] = useState<Config>(config);
  const [error, setError] = useState<string | null>(null);

  const validate = (c: Config): string | null => {
    const rows: [string, RouterProfile][] = [
      ["主路由", c.main_router],
      ["旁路由", c.bypass_router],
    ];
    for (const [label, p] of rows) {
      if (!isIpv4(p.gateway)) return `${label}网关不是合法 IPv4`;
      for (const d of p.dns) if (!isIpv4(d)) return `${label}DNS「${d}」不是合法 IPv4`;
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
      <h3>配置</h3>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={draft.set_dns_on_switch}
          onChange={(e) => setDraft({ ...draft, set_dns_on_switch: e.target.checked })}
        />
        切换网关时同时设置 DNS
      </label>
      <div className="config__profiles">
        <ProfileForm
          title="主路由"
          profile={draft.main_router}
          onChange={(p) => setDraft({ ...draft, main_router: p })}
        />
        <ProfileForm
          title="旁路由"
          profile={draft.bypass_router}
          onChange={(p) => setDraft({ ...draft, bypass_router: p })}
        />
      </div>
      {error && <p className="error-text">{error}</p>}
      <button className="btn btn--primary" onClick={handleSave}>
        保存配置
      </button>
    </section>
  );
}
