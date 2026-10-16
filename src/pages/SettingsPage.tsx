import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Chip, Input, ListBox, ListBoxItem, Select } from "@heroui/react";
import { useUI, type ThemeMode } from "../store/useUI";
import { Section, PrimaryButton, ErrorText, OkText } from "../components/ui";
import RuntimeSection from "../components/RuntimeSection";

interface TierInfo {
  tier: "free" | "pro";
  is_pro: boolean;
}

type MsgKind = "ok" | "err";

const THEMES: ThemeMode[] = ["light", "dark", "system"];

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
    <div className="flex max-w-2xl flex-col">
      <h1 className="text-2xl font-bold">{t("settings.heading")}</h1>

      <Section className="mt-5">
        <Row label={t("settings.language")}>
          <Select.Root
            selectedKey={i18n.resolvedLanguage ?? lang}
            onSelectionChange={(k) => setLang(String(k))}
            aria-label={t("settings.language")}
          >
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                <ListBoxItem id="en">{t("language.en")}</ListBoxItem>
                <ListBoxItem id="zh">{t("language.zh")}</ListBoxItem>
              </ListBox>
            </Select.Popover>
          </Select.Root>
        </Row>

        <Row label={t("settings.theme")}>
          <Select.Root
            selectedKey={theme}
            onSelectionChange={(k) => setTheme(String(k) as ThemeMode)}
            aria-label={t("settings.theme")}
          >
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {THEMES.map((m) => (
                  <ListBoxItem key={m} id={m}>
                    {t(`theme.${m}`)}
                  </ListBoxItem>
                ))}
              </ListBox>
            </Select.Popover>
          </Select.Root>
        </Row>

        <Row label={t("settings.tier")}>
          <Chip size="sm" color={tier?.is_pro ? "success" : "default"}>
            <Chip.Label>
              {tier?.is_pro ? t("settings.tierPro") : t("settings.tierFree")}
            </Chip.Label>
          </Chip>
        </Row>
      </Section>

      {tierError && (
        <div className="mt-3">
          <ErrorText>{tierError}</ErrorText>
        </div>
      )}

      <Section title="License Key" className="mt-5">
        <div className="flex gap-2">
          <Input
            aria-label="License Key"
            placeholder="XXXX-XXXX-XXXX-XXXX"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            className="flex-1"
          />
          <PrimaryButton onPress={activate} isDisabled={busy || !key.trim()}>
            {t("settings.tierPro")}
          </PrimaryButton>
        </div>
        {msg && (
          <div className="mt-3">
            {msgKind === "ok" ? <OkText>{msg}</OkText> : <ErrorText>{msg}</ErrorText>}
          </div>
        )}
      </Section>

      {/* 分轨运行时：按需自举的 PyTorch 环境 */}
      <RuntimeSection />
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-border py-3 last:border-b-0">
      <span className="text-sm">{label}</span>
      {children}
    </div>
  );
}
