import { useTranslation } from "react-i18next";

export default function PluginManagerPage() {
  const { t } = useTranslation();
  return (
    <div>
      <h1 style={{ marginTop: 0 }}>{t("apps.plugins.name")}</h1>
      <p style={{ color: "var(--text-muted)" }}>{t("apps.plugins.desc")}</p>
      <div
        style={{
          marginTop: 24,
          padding: 24,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
        }}
      >
        <span
          style={{
            fontSize: 12,
            background: "var(--surface-2)",
            color: "var(--text-muted)",
            padding: "2px 8px",
            borderRadius: 999,
          }}
        >
          {t("apps.plugins.soon")}
        </span>
      </div>
    </div>
  );
}
