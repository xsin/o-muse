// 顶部更新提示条（非模态）
//
// 行为（对应 PRD FR-U1/U2/U3/U4/U6）：
//   - 挂载后延迟 5 秒首次自动检查（manual=false，静默失败），之后每 6 小时一次；
//   - 自动检查可在设置页关闭（`autoCheckUpdate`）；
//   - 有新版本且未被「稍后」→ 顶部显示非模态条：当前版本 → 新版本，可展开看更新要点；
//   - 点「更新」→ 后端下载/校验/解压（监听 omuse://update-progress）→
//     调度替换脚本（update_apply）→ 退出应用（app_exit）；
//   - 失败：危险色提示 + 保留重试入口。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button, Chip, Spinner } from "@heroui/react";
import { useUI } from "../store/useUI";
import { ErrorText, Progress, formatBytes, formatDate } from "./ui";

/** 后端 `core::updater::UpdateInfo`。 */
export interface UpdateInfo {
  current_version: string;
  version: string;
  notes: string;
  pub_date: string;
  download_bytes: number | null;
}

/** 后端 `core::updater::UpdateProgress`。 */
interface UpdateProgress {
  stage: string;
  downloaded: number;
  total: number | null;
  message: string;
}

/** 首次自动检查的延迟（避免与首屏渲染争抢资源）。 */
const FIRST_CHECK_DELAY_MS = 5_000;
/** 周期检查间隔（6 小时）。 */
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1_000;

type Flow = "idle" | "downloading" | "applying" | "restarting" | "failed";

export default function UpdateBanner() {
  const { t } = useTranslation();
  const autoCheckUpdate = useUI((s) => s.autoCheckUpdate);
  const updateDismissed = useUI((s) => s.updateDismissed);
  const setUpdateDismissed = useUI((s) => s.setUpdateDismissed);

  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [flow, setFlow] = useState<Flow>("idle");
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  // 已开始安装后不再允许隐藏提示条（否则进度会消失）
  const busy = flow === "downloading" || flow === "applying" || flow === "restarting";

  const check = useCallback(async () => {
    try {
      const hit = await invoke<UpdateInfo | null>("update_check", { manual: false });
      if (hit) setInfo(hit);
    } catch {
      // manual=false 时后端本就不返回错误；这里兜底保持静默（FR-U6）
    }
  }, []);

  // 自动检查：延迟首次 + 周期
  useEffect(() => {
    if (!autoCheckUpdate) return;
    let cancelled = false;
    const first = setTimeout(() => {
      if (!cancelled) void check();
    }, FIRST_CHECK_DELAY_MS);
    const timer = setInterval(() => {
      if (!cancelled) void check();
    }, CHECK_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [autoCheckUpdate, check]);

  // 进度由后端事件推送，避免前端轮询
  useEffect(() => {
    const un = listen<UpdateProgress>("omuse://update-progress", (e) => {
      setProgress(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const dismissed = useMemo(
    () => !!info && updateDismissed === info.version,
    [info, updateDismissed],
  );

  const install = useCallback(async () => {
    setError(null);
    setProgress(null);
    setFlow("downloading");
    try {
      const fresh = await invoke<UpdateInfo>("update_install");
      setInfo(fresh);
      setFlow("applying");
      await invoke("update_apply");
      setFlow("restarting");
      // 替换脚本会等本进程退出；退出由我们主动触发
      await invoke("app_exit");
    } catch (e) {
      setError(String(e));
      setFlow("failed");
    }
  }, []);

  const doing = useRef(false);
  const onUpdate = useCallback(() => {
    if (doing.current) return;
    doing.current = true;
    void install().finally(() => {
      doing.current = false;
    });
  }, [install]);

  const onLater = useCallback(() => {
    if (info) setUpdateDismissed(info.version);
  }, [info, setUpdateDismissed]);

  if (!info) return null;
  // 「稍后」后本次不再提示；但一旦开始安装必须继续显示进度
  if (dismissed && !busy) return null;

  const pct =
    progress && progress.total && progress.total > 0
      ? (progress.downloaded / progress.total) * 100
      : 0;
  const stageKey = progress?.stage;

  return (
    <div className="border-b border-border bg-surface px-4 py-2">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
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

        {busy && (
          <span className="flex items-center gap-2 text-xs opacity-70">
            <Spinner.Root />
            {stageKey
              ? t(`update.stages.${stageKey}`, { defaultValue: progress?.message ?? "" })
              : t("update.stages.downloading")}
            {progress && progress.total != null && (
              <span>
                {formatBytes(progress.downloaded)} / {formatBytes(progress.total)}
              </span>
            )}
          </span>
        )}

        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="ghost"
            isDisabled={busy}
            onPress={() => setExpanded((v) => !v)}
          >
            {expanded ? t("update.hideNotes") : t("update.showNotes")}
          </Button>
          <Button variant="ghost" isDisabled={busy} onPress={onLater}>
            {t("update.later")}
          </Button>
          <Button variant="primary" isDisabled={busy} onPress={onUpdate}>
            {flow === "failed" ? t("update.retry") : t("update.update")}
          </Button>
        </div>
      </div>

      {busy && (
        <div className="mt-2">
          <Progress
            value={pct}
            label={progress?.message ?? t("update.stages.downloading")}
          />
        </div>
      )}

      {expanded && (
        <div className="mt-2 border-t border-border pt-2">
          <div className="flex gap-3 text-xs opacity-70">
            <span>{t("update.released", { date: formatDate(info.pub_date) })}</span>
            {info.download_bytes != null && (
              <span>{t("update.size", { size: formatBytes(info.download_bytes) })}</span>
            )}
          </div>
          <p className="mt-2 whitespace-pre-wrap text-sm">
            {info.notes || t("update.noNotes")}
          </p>
        </div>
      )}

      {flow === "failed" && error && (
        <div className="mt-2 flex items-center gap-3">
          <ErrorText>{t("update.failed", { err: error })}</ErrorText>
          <Button variant="secondary" onPress={onUpdate}>
            {t("update.retry")}
          </Button>
        </div>
      )}
    </div>
  );
}
