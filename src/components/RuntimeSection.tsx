// 分轨运行时管理（v1.2）
//
// 分轨依赖 PyTorch（约 151MB）+ 模型权重（约 84MB）+ 自包含 CPython（24MB），
// 合计约 259MB 下载 / ~900MB 落盘，故按需自举而非随包分发。

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Panel, Button, ErrorText, OkText, ProgressBar } from "./ui";

type Stage =
  | "missing"
  | "checking"
  | "downloading_python"
  | "verifying"
  | "extracting"
  | "creating_venv"
  | "installing_deps"
  | "downloading_model"
  | "ready"
  | "failed";

interface RuntimeStatus {
  stage: Stage;
  progress: number | null;
  message: string;
  python_path: string | null;
  size_bytes: number | null;
  estimated_download_bytes: number;
}

interface RuntimeProgress {
  stage: Stage;
  progress: number | null;
  message: string;
}

function fmtMB(bytes: number | null | undefined): string {
  if (!bytes) return "—";
  return `${(bytes / 1_048_576).toFixed(0)} MB`;
}

const BUSY: Stage[] = [
  "downloading_python",
  "verifying",
  "extracting",
  "creating_venv",
  "installing_deps",
  "downloading_model",
];

export default function RuntimeSection() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [progress, setProgress] = useState<RuntimeProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<RuntimeStatus>("runtime_status"));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    // 后端经事件推送进度，避免前端轮询
    const un = listen<RuntimeProgress>("omuse://runtime-progress", (e) => {
      setProgress(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  const install = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    setProgress(null);
    try {
      const s = await invoke<RuntimeStatus>("install_runtime");
      setStatus(s);
      setNotice(null);
    } catch (e) {
      setError(String(e));
      await refresh();
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const remove = async () => {
    if (!window.confirm(t("runtime.confirmRemove"))) return;
    setError(null);
    try {
      await invoke("remove_runtime");
      setNotice(t("runtime.removed"));
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const stage = busy && progress ? progress.stage : status?.stage ?? "missing";
  const pct = busy && progress?.progress != null ? progress.progress : status?.progress ?? 0;
  const isReady = status?.stage === "ready";
  const running = busy || BUSY.includes(stage);

  return (
    <Panel style={{ marginTop: 20 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <strong style={{ fontSize: 13 }}>{t("runtime.title")}</strong>
        <span
          style={{
            fontSize: 12,
            color: isReady ? "var(--ok)" : "var(--text-muted)",
          }}
        >
          {t(`runtime.stages.${stage}`, { defaultValue: stage })}
        </span>
      </div>

      <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--text-muted)" }}>
        {t("runtime.desc", { size: fmtMB(status?.estimated_download_bytes ?? 259 * 1024 * 1024) })}
      </p>

      <div
        style={{
          display: "flex",
          gap: 16,
          fontSize: 12,
          color: "var(--text-muted)",
          margin: "10px 0",
        }}
      >
        <span>
          {t("runtime.size")}: {fmtMB(status?.size_bytes)}
        </span>
        {!isReady && (
          <span>
            {t("runtime.estimate", {
              size: fmtMB(status?.estimated_download_bytes),
            })}
          </span>
        )}
      </div>

      {running && (
        <>
          <ProgressBar value={pct} />
          {progress?.message && (
            <p style={{ margin: "8px 0 0", fontSize: 12, color: "var(--text-muted)" }}>
              {progress.message}
            </p>
          )}
        </>
      )}

      {!running && (
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <Button variant="primary" onClick={install} disabled={busy}>
            {isReady ? t("runtime.reinstall") : t("runtime.install")}
          </Button>
          {isReady && (
            <Button onClick={remove} disabled={busy}>
              {t("runtime.remove")}
            </Button>
          )}
        </div>
      )}

      {!isReady && !running && (
        <p style={{ margin: "10px 0 0", fontSize: 12, color: "var(--text-muted)" }}>
          {t("runtime.needPython")}
        </p>
      )}

      {error && (
        <div style={{ marginTop: 10 }}>
          <ErrorText>{error}</ErrorText>
        </div>
      )}
      {notice && (
        <div style={{ marginTop: 10 }}>
          <OkText>{notice}</OkText>
        </div>
      )}
    </Panel>
  );
}
