// 分轨运行时管理
//
// 分轨依赖 PyTorch（约 151MB）+ 模型权重（约 84MB）+ 自包含 CPython（24MB），
// 合计约 259MB 下载 / ~900MB 落盘，故按需自举而非随包分发。

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Chip, Modal, Button } from "@heroui/react";
import { Section, PrimaryButton, ErrorText, OkText, Progress } from "./ui";

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
  const [confirmRemove, setConfirmRemove] = useState(false);

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
    } catch (e) {
      setError(String(e));
      await refresh();
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const doRemove = async () => {
    setConfirmRemove(false);
    setError(null);
    try {
      await invoke("remove_runtime");
      setNotice(t("runtime.removed"));
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const stage: Stage = busy && progress ? progress.stage : (status?.stage ?? "missing");
  const pct = busy && progress?.progress != null ? progress.progress : (status?.progress ?? 0);
  const isReady = status?.stage === "ready";
  const running = busy || BUSY.includes(stage);

  return (
    <Section
      title={t("runtime.title")}
      className="mt-5"
      actions={
        <Chip size="sm" color={isReady ? "success" : "default"}>
          <Chip.Label>{t(`runtime.stages.${stage}`, { defaultValue: stage })}</Chip.Label>
        </Chip>
      }
    >
      <p className="text-xs opacity-70">
        {t("runtime.desc", {
          size: fmtMB(status?.estimated_download_bytes ?? 259 * 1024 * 1024),
        })}
      </p>

      <div className="mt-3 flex gap-4 text-xs opacity-70">
        <span>
          {t("runtime.size")}: {fmtMB(status?.size_bytes)}
        </span>
        {!isReady && (
          <span>
            {t("runtime.estimate", { size: fmtMB(status?.estimated_download_bytes) })}
          </span>
        )}
      </div>

      {running && (
        <div className="mt-3 flex flex-col gap-2">
          <Progress value={pct} />
          {progress?.message && <p className="text-xs opacity-70">{progress.message}</p>}
        </div>
      )}

      {!running && (
        <div className="mt-4 flex gap-2">
          <PrimaryButton onPress={install} isDisabled={busy}>
            {isReady ? t("runtime.reinstall") : t("runtime.install")}
          </PrimaryButton>
          {isReady && (
            <Button variant="secondary" onPress={() => setConfirmRemove(true)}>
              {t("runtime.remove")}
            </Button>
          )}
        </div>
      )}

      {!isReady && !running && (
        <p className="mt-3 text-xs opacity-70">{t("runtime.needPython")}</p>
      )}

      {error && (
        <div className="mt-3">
          <ErrorText>{error}</ErrorText>
        </div>
      )}
      {notice && (
        <div className="mt-3">
          <OkText>{notice}</OkText>
        </div>
      )}

      {/* 卸载需二次确认：会释放约 900MB，之后需重新下载 */}
      <Modal.Root isOpen={confirmRemove} onOpenChange={setConfirmRemove}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>{t("runtime.remove")}</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <p className="text-sm">{t("runtime.confirmRemove")}</p>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="secondary" onPress={() => setConfirmRemove(false)}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onPress={doRemove}>
                  {t("runtime.remove")}
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>
    </Section>
  );
}
