import { useTranslation } from "react-i18next";
import { useUI, type ThemeMode } from "../store/useUI";

export default function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { theme, lang, setTheme, setLang } = useUI();

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
        <span style={{ color: "var(--ok)" }}>{t("settings.tierFree")}</span>
      </Field>
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
