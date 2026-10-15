import { useTranslation } from "react-i18next";

export default function StemSeparationPage() {
  const { t } = useTranslation();
  return (
    <div>
      <h1 style={{ marginTop: 0 }}>{t("apps.stem_separation.name")}</h1>
      <p style={{ color: "var(--text-muted)" }}>
        {t("apps.stem_separation.desc")}
      </p>
      <div
        style={{
          marginTop: 24,
          padding: 24,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "var(--radius)",
          color: "var(--text-muted)",
        }}
      >
        {t("apps.stem_separation.brandPath")} — UI placeholder (Task 4).
      </div>
    </div>
  );
}
