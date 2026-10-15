import { useTranslation } from "react-i18next";
import { useUI, type AppId } from "../store/useUI";
import { appRegistry } from "../appRegistry";

export default function Sidebar() {
  const { t } = useTranslation();
  const { openApps, activeView, closeApp, setActive } = useUI();

  return (
    <aside
      style={{
        width: 232,
        flexShrink: 0,
        background: "var(--surface)",
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        padding: "16px 12px",
        gap: 4,
      }}
    >
      <div
        style={{
          fontWeight: 700,
          fontSize: 18,
          padding: "8px 12px 16px",
          color: "var(--brand-strong)",
        }}
      >
        {t("app.title")}
      </div>

      <NavItem
        label={t("nav.market")}
        active={activeView === "market"}
        onClick={() => setActive("market")}
      />

      {openApps.map((id: AppId) => {
        const def = appRegistry.find((a) => a.id === id);
        return (
          <div
            key={id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
            }}
          >
            <div
              onClick={() => setActive(id)}
              style={{
                flex: 1,
                cursor: "pointer",
                padding: "8px 12px",
                borderRadius: 8,
                background:
                  activeView === id ? "var(--surface-2)" : "transparent",
                color: activeView === id ? "var(--brand-strong)" : "var(--text)",
                fontSize: 14,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {def?.icon} {t(`apps.${id}.name`)}
            </div>
            <button
              onClick={() => closeApp(id)}
              title={t("common.close")}
              style={{
                border: "none",
                background: "transparent",
                color: "var(--text-muted)",
                cursor: "pointer",
                fontSize: 14,
                padding: 4,
              }}
            >
              ✕
            </button>
          </div>
        );
      })}

      <div style={{ flex: 1 }} />

      <NavItem
        label={t("nav.settings")}
        active={activeView === "settings"}
        onClick={() => setActive("settings")}
      />
    </aside>
  );
}

function NavItem({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <div
      onClick={onClick}
      style={{
        cursor: "pointer",
        padding: "8px 12px",
        borderRadius: 8,
        background: active ? "var(--surface-2)" : "transparent",
        color: active ? "var(--brand-strong)" : "var(--text)",
        fontSize: 14,
        fontWeight: active ? 600 : 400,
      }}
    >
      {label}
    </div>
  );
}
