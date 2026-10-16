import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Chip, Input, ListBox, ListBoxItem, Select, Switch } from "@heroui/react";
import { useUI, type ThemeMode } from "../store/useUI";
import { Section, PrimaryButton, SecondaryButton, ErrorText, OkText, Loading, formatBytes } from "../components/ui";
import RuntimeSection from "../components/RuntimeSection";

interface TierInfo {
  tier: "free" | "pro";
  is_pro: boolean;
}

/** 后端 `core::updater::UpdateInfo`。 */
interface UpdateInfo {
  current_version: string;
  version: string;
  notes: string;
  pub_date: string;
  download_bytes: number | null;
}

/**
 * 更新源地址（只读展示，供用户审计）。
 *
 * 必须与 Rust 端 `core::updater::UPDATE_MANIFEST_URL` 保持一致；
 * 前端不做任何请求，仅展示。
 */
const UPDATE_MANIFEST_URL =
  "https://xsin.github.io/o-muse/releases/latest/latest.json";

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

      {/* 自动更新：版本、自动检查开关、手动检查、更新源（可审计） */}
      <UpdateSection />

      {/* 分轨运行时：按需自举的 PyTorch 环境 */}
      <RuntimeSection />
    </div>
  );
}

/**
 * 「更新」区块（PRD FR-U5）。
 *
 * - 当前版本：只读，来自 `package_info`（`tauri.conf.json` 单一来源）；
 * - 自动检查更新：开关，默认开；
 * - 检查更新：手动触发（`manual = true`），失败时展示原因（FR-U6）；
 * - 更新源：只读展示清单地址，供用户自行核对。
 */
function UpdateSection() {
  const { t } = useTranslation();
  const autoCheckUpdate = useUI((s) => s.autoCheckUpdate);
  const setAutoCheckUpdate = useUI((s) => s.setAutoCheckUpdate);

  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [status, setStatus] = useState<"idle" | "uptodate">("idle");
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("current_version")
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  async function checkNow() {
    setChecking(true);
    setError(null);
    setInfo(null);
    setStatus("idle");
    try {
      const hit = await invoke<UpdateInfo | null>("update_check", { manual: true });
      if (hit) setInfo(hit);
      else setStatus("uptodate");
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  }

  async function install() {
    setInstalling(true);
    setInstallError(null);
    try {
      await invoke("update_install");
      await invoke("update_apply");
      await invoke("app_exit");
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  }

  return (
    <Section title={t("update.title")} className="mt-5">
      <Row label={t("update.current")}>
        <span className="text-sm opacity-70">{version ?? "—"}</span>
      </Row>

      <Row label={t("update.autoCheck")}>
        <Switch.Root
          isSelected={autoCheckUpdate}
          onChange={(on: boolean) => setAutoCheckUpdate(on)}
          aria-label={t("update.autoCheck")}
        >
          <Switch.Content>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
          </Switch.Content>
        </Switch.Root>
      </Row>

      <Row label={t("update.check")}>
        <SecondaryButton onPress={checkNow} isDisabled={checking || installing}>
          {t("update.checkNow")}
        </SecondaryButton>
      </Row>

      <Row label={t("update.source")}>
        <span className="max-w-[280px] truncate text-xs opacity-70" title={UPDATE_MANIFEST_URL}>
          {UPDATE_MANIFEST_URL}
        </span>
      </Row>

      {checking && (
        <div className="mt-2">
          <Loading label={t("update.checking")} />
        </div>
      )}

      {status === "uptodate" && !checking && (
        <div className="mt-2">
          <OkText>{t("update.upToDate")}</OkText>
        </div>
      )}

      {info && !checking && (
        <div className="mt-2 flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <Chip size="sm" color="accent">
              <Chip.Label>{t("update.available")}</Chip.Label>
            </Chip>
            <span className="text-sm">
              {t("update.versionLine", {
                current: info.current_version,
                next: info.version,
              })}
            </span>
            {info.download_bytes != null && (
              <span className="text-xs opacity-70">
                {t("update.size", { size: formatBytes(info.download_bytes) })}
              </span>
            )}
          </div>
          {info.notes && (
            <p className="whitespace-pre-wrap text-sm opacity-70">{info.notes}</p>
          )}
          <div>
            <PrimaryButton onPress={install} isDisabled={installing}>
              {installing ? t("update.installing") : t("update.update")}
            </PrimaryButton>
          </div>
        </div>
      )}

      {error && (
        <div className="mt-2">
          <ErrorText>{t("update.checkFailed", { err: error })}</ErrorText>
        </div>
      )}
      {installError && (
        <div className="mt-2">
          <ErrorText>{t("update.failed", { err: installError })}</ErrorText>
        </div>
      )}
    </Section>
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
