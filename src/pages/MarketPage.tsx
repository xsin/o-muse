import { useTranslation } from "react-i18next";
import { useUI, type AppId } from "../store/useUI";
import { appRegistry } from "../appRegistry";

export default function MarketPage() {
  const { t } = useTranslation();
  const { openApp } = useUI();

  return (
    <div>
      <h1 style={{ margin: "0 0 4px" }}>{t("market.heading")}</h1>
      <p style={{ color: "var(--text-muted)", marginTop: 0 }}>
        {t("market.subtitle")}
      </p>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(240px, 1fr))",
          gap: 16,
          marginTop: 20,
        }}
      >
        {appRegistry.map((app) => (
          <Card
            key={app.id}
            icon={app.icon}
            title={t(`apps.${app.id}.name`)}
            desc={t(`apps.${app.id}.desc`)}
            badge={app.badge ? t(`apps.${app.id}.soon`) : undefined}
            enabled={app.enabled}
            onClick={() => openApp(app.id as AppId)}
          />
        ))}
      </div>
    </div>
  );
}

function Card({
  icon,
  title,
  desc,
  badge,
  enabled,
  onClick,
}: {
  icon: string;
  title: string;
  desc: string;
  badge?: string;
  enabled: boolean;
  onClick: () => void;
}) {
  return (
    <div
      onClick={enabled ? onClick : undefined}
      style={{
        background: "var(--surface)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius)",
        padding: 20,
        boxShadow: "var(--shadow)",
        cursor: enabled ? "pointer" : "not-allowed",
        opacity: enabled ? 1 : 0.6,
        position: "relative",
      }}
    >
      {badge && (
        <span
          style={{
            position: "absolute",
            top: 12,
            right: 12,
            fontSize: 11,
            background: "var(--surface-2)",
            color: "var(--text-muted)",
            padding: "2px 8px",
            borderRadius: 999,
          }}
        >
          {badge}
        </span>
      )}
      <div style={{ fontSize: 32 }}>{icon}</div>
      <h3 style={{ margin: "12px 0 6px" }}>{title}</h3>
      <p style={{ color: "var(--text-muted)", fontSize: 13, margin: 0 }}>
        {desc}
      </p>
    </div>
  );
}
