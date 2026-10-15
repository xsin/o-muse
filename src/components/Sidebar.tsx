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
            <button
              onClick={() => setActive(id)}
              aria-current={activeView === id ? "page" : undefined}
              style={{
                flex: 1,
                minWidth: 0,
                cursor: "pointer",
                padding: "8px 12px",
                borderRadius: 8,
                border: "none",
                textAlign: "left",
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
            </button>
            <button
              onClick={() => closeApp(id)}
              title={t("common.close")}
              aria-label={t("common.close")}
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

/** 可键盘操作的导航项（review 修复：此前为纯 div，鼠标专用）。 */
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
    <button
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      style={{
        cursor: "pointer",
        padding: "8px 12px",
        borderRadius: 8,
        border: "none",
        textAlign: "left",
        background: active ? "var(--surface-2)" : "transparent",
        color: active ? "var(--brand-strong)" : "var(--text)",
        fontSize: 14,
        fontWeight: active ? 600 : 400,
      }}
    >
      {label}
    </button>
  );
}
