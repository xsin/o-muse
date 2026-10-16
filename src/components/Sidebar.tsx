import { useTranslation } from "react-i18next";
import { Button } from "@heroui/react";
import { useUI, type AppId } from "../store/useUI";
import { appRegistry } from "../appRegistry";

/** 左栏常驻坞：工具箱 + 已打开的 App 标签 + 设置。 */
export default function Sidebar() {
  const { t } = useTranslation();
  const { openApps, activeView, closeApp, setActive } = useUI();

  return (
    <aside className="flex w-56 shrink-0 flex-col gap-1 border-r border-border bg-surface p-3">
      <div className="px-3 pb-4 pt-2 text-lg font-bold text-accent">
        {t("app.title")}
      </div>

      <NavItem
        label={t("nav.market")}
        active={activeView === "market"}
        onPress={() => setActive("market")}
      />

      {openApps.map((id: AppId) => {
        const def = appRegistry.find((a) => a.id === id);
        const isActive = activeView === id;
        return (
          <div key={id} className="flex items-center gap-1">
            <Button
              variant={isActive ? "secondary" : "ghost"}
              className="min-w-0 flex-1 justify-start"
              onPress={() => setActive(id)}
              aria-current={isActive ? "page" : undefined}
            >
              <span className="shrink-0">{def?.icon}</span>
              <span className="truncate">{t(`apps.${id}.name`)}</span>
            </Button>
            <Button
              isIconOnly
              size="sm"
              variant="ghost"
              aria-label={t("common.close")}
              onPress={() => closeApp(id)}
            >
              ✕
            </Button>
          </div>
        );
      })}

      <div className="flex-1" />

      <NavItem
        label={t("nav.settings")}
        active={activeView === "settings"}
        onPress={() => setActive("settings")}
      />
    </aside>
  );
}

function NavItem({
  label,
  active,
  onPress,
}: {
  label: string;
  active: boolean;
  onPress: () => void;
}) {
  return (
    <Button
      variant={active ? "secondary" : "ghost"}
      className="justify-start font-medium"
      onPress={onPress}
      aria-current={active ? "page" : undefined}
    >
      {label}
    </Button>
  );
}
