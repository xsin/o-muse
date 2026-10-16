import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useUI, type ThemeMode } from "../store/useUI";
import { OkText, ErrorText, Button, Panel } from "../components/ui";
import RuntimeSection from "../components/RuntimeSection";

interface TierInfo {
  tier: "free" | "pro";
  is_pro: boolean;
}

type MsgKind = "ok" | "err";

export default function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { theme, lang, setTheme, setLang } = useUI();
  const [tier, setTier] = useState<TierInfo | null>(null);
  const [key, setKey] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [msgKind, setMsgKind] = useState<MsgKind>("ok");
  const [tierError, setTierError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refreshTier() {
    try {
      const info = await invoke<TierInfo>("get_tier");
      setTier(info);
      setTierError(null);
    } catch (e) {
      // 不再静默忽略：store/后端异常时明确告知用户（review 修复）
      setTierError(String(e));
    }
  }

  useEffect(() => {
    refreshTier();
  }, []);

  async function activate() {
    if (!key.trim()) return;
    setBusy(true);
    setMsg(null);
    setMsgKind("err");
    try {
      const info = await invoke<TierInfo>("activate_license_key", { key });
      setTier(info);
      // 用结构化状态而非解析字符串决定颜色（review 修复）
      setMsgKind(info.is_pro ? "ok" : "err");
      setMsg(info.is_pro ? t("settings.activated") : t("settings.invalidKey"));
    } catch (e) {
      setMsgKind("err");
      setMsg(t("settings.activateFailed", { err: String(e) }));
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

      {tierError && (
        <div style={{ marginTop: 12 }}>
          <ErrorText>{tierError}</ErrorText>
        </div>
      )}

      <Panel style={{ marginTop: 24 }}>
        <label
          style={{ display: "block", marginBottom: 8, fontSize: 13 }}
          htmlFor="license-key"
        >
          {t("settings.tier")} · License Key
        </label>
        <div style={{ display: "flex", gap: 8 }}>
          <input
            id="license-key"
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
          <Button
            variant="primary"
            onClick={activate}
            disabled={busy || !key.trim()}
          >
            {t("settings.tierPro")}
          </Button>
        </div>
        {msg &&
          (msgKind === "ok" ? (
            <div style={{ marginTop: 10 }}>
              <OkText>{msg}</OkText>
            </div>
          ) : (
            <div style={{ marginTop: 10 }}>
              <ErrorText>{msg}</ErrorText>
            </div>
          ))}
      </Panel>

      {/* 分轨运行时：按需自举的 PyTorch 环境 */}
      <RuntimeSection />
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
