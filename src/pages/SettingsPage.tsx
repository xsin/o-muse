import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useUI, type ThemeMode } from "../store/useUI";

interface TierInfo {
  tier: "free" | "pro";
  is_pro: boolean;
}

export default function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { theme, lang, setTheme, setLang } = useUI();
  const [tier, setTier] = useState<TierInfo | null>(null);
  const [key, setKey] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refreshTier() {
    try {
      const info = await invoke<TierInfo>("get_tier");
      setTier(info);
    } catch {
      /* 非 Tauri 环境下忽略 */
    }
  }

  useEffect(() => {
    refreshTier();
  }, []);

  async function activate() {
    if (!key.trim()) return;
    setBusy(true);
    setMsg(null);
    try {
      const info = await invoke<TierInfo>("activate_license_key", { key });
      setTier(info);
      setMsg(info.is_pro ? "Pro 已激活" : "密钥无效，仍为免费版");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      setKey("");
    }
  }

  return (
    <div style={{ maxWidth: 520 }}>
      <h1 style={{ marginTop: 0 }}>{t("settings.heading")}</h1>

      <Field label={t("settings.language")}>
        <select
          value={i18n.resolvedLanguage ?? lang}
          onChange={(e) => setLang(e.target.value)}
        >
          <option value="en">{t("language.en")}</option>
          <option value="zh">{t("language.zh")}</option>
        </select>
      </Field>

      <Field label={t("settings.theme")}>
        <select
          value={theme}
          onChange={(e) => setTheme(e.target.value as ThemeMode)}
        >
          <option value="light">{t("theme.light")}</option>
          <option value="dark">{t("theme.dark")}</option>
          <option value="system">{t("theme.system")}</option>
        </select>
      </Field>

      <Field label={t("settings.tier")}>
        {tier?.is_pro ? (
          <span style={{ color: "var(--ok)" }}>{t("settings.tierPro")}</span>
        ) : (
          <span style={{ color: "var(--text-muted)" }}>
            {t("settings.tierFree")}
          </span>
        )}
      </Field>

      <div style={{ marginTop: 24 }}>
        <label style={{ display: "block", marginBottom: 6, fontSize: 13 }}>
          {t("settings.tier")} · License Key
        </label>
        <div style={{ display: "flex", gap: 8 }}>
          <input
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder="XXXX-XXXX-XXXX-XXXX"
            style={{
              flex: 1,
              padding: "8px 10px",
              borderRadius: 8,
              border: "1px solid var(--border)",
              background: "var(--surface)",
              color: "var(--text)",
            }}
          />
          <button onClick={activate} disabled={busy || !key.trim()}>
            {t("settings.tierPro")}
          </button>
        </div>
        {msg && (
          <p
            style={{
              fontSize: 13,
              color: msg.includes("已激活") ? "var(--ok)" : "var(--danger)",
              marginTop: 8,
            }}
          >
            {msg}
          </p>
        )}
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        padding: "12px 0",
        borderBottom: "1px solid var(--border)",
      }}
    >
      <span style={{ color: "var(--text)" }}>{label}</span>
      {children}
    </div>
  );
}
