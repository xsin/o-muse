import { useTranslation } from "react-i18next";
import { Card, Chip } from "@heroui/react";
import { useUI, type AppId } from "../store/useUI";
import { appRegistry } from "../appRegistry";

/** 工具箱首页：以卡片网格呈现所有可用 App。 */
export default function MarketPage() {
  const { t } = useTranslation();
  const { openApp } = useUI();

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h1 className="text-2xl font-bold">{t("market.heading")}</h1>
        <p className="mt-1 text-sm opacity-70">{t("market.subtitle")}</p>
      </div>

      <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">
        {appRegistry.map((app) => {
          const disabled = !app.enabled;
          return (
            <Card
              key={app.id}
              className={
                disabled
                  ? "opacity-60"
                  : "cursor-pointer transition-transform hover:-translate-y-0.5"
              }
            >
              <Card.Header
                className="cursor-pointer"
                onClick={() => !disabled && openApp(app.id as AppId)}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-2xl">{app.icon}</span>
                  {disabled && (
                    <Chip size="sm" color="default">
                      <Chip.Label>{t("apps.plugins.soon")}</Chip.Label>
                    </Chip>
                  )}
                </div>
                <Card.Title className="mt-2">{t(`apps.${app.id}.name`)}</Card.Title>
                <Card.Description>{t(`apps.${app.id}.desc`)}</Card.Description>
              </Card.Header>
              <Card.Footer>
                <span
                  className="cursor-pointer text-sm font-medium text-accent"
                  onClick={() => !disabled && openApp(app.id as AppId)}
                >
                  {disabled ? t("apps.plugins.soon") : t("market.open")}
                </span>
              </Card.Footer>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
