import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useUI } from "../store/useUI";
import {
  Panel,
  Button,
  chip,
  displayPath,
  ErrorText,
  EmptyState,
  ProgressBar,
} from "../components/ui";

interface StatusResponse {
  job_id: string;
  status: "queued" | "processing" | "done" | "error" | "demo_done";
  progress: number;
  message: string | null;
  demo: boolean;
  stems: string[];
}

interface StemFileEntry {
  name: string;
  path: string;
  size: number;
}

interface StemsResponse {
  job_id: string;
  path: string;
  files: StemFileEntry[];
}

function formatBytes(n: number): string {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  return `${(n / 1024 ** i).toFixed(1)} ${units[i]}`;
}

export default function StemSeparationPage() {
  const { t } = useTranslation();
  const setActive = useUI((s) => s.setActive);
  const [runtimeReady, setRuntimeReady] = useState<boolean | null>(null);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [results, setResults] = useState<StemsResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const stopPolling = useCallback(() => {
    if (timer.current !== null) {
      window.clearInterval(timer.current);
      timer.current = null;
    }
  }, []);

  // 卸载时清理轮询，避免定时器泄漏
  useEffect(() => stopPolling, [stopPolling]);

  // 运行时是否就绪：未就绪时给出明确引导，而不是让用户点下去再失败
  useEffect(() => {
    invoke<{ stage: string }>("runtime_status")
      .then((s) => setRuntimeReady(s.stage === "ready"))
      .catch(() => setRuntimeReady(null));
  }, []);

  const poll = useCallback(
    (id: string) => {
      stopPolling();
      timer.current = window.setInterval(async () => {
        try {
          const s = await invoke<StatusResponse>("stem_status", { jobId: id });
          setStatus(s);
          if (s.status === "done" || s.status === "demo_done") {
            const r = await invoke<StemsResponse>("stem_results", { jobId: id });
            setResults(r);
            setBusy(false);
            stopPolling();
          } else if (s.status === "error") {
            setError(s.message ?? "unknown error");
            setBusy(false);
            stopPolling();
          }
        } catch (e) {
          setError(String(e));
          setBusy(false);
          stopPolling();
        }
      }, 1000);
    },
    [stopPolling],
  );

  const pick = async () => {
    const picked = await openDialog({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Audio",
          extensions: ["mp3", "wav", "flac", "m4a", "aac", "ogg", "aiff"],
        },
      ],
    });
    if (typeof picked === "string") setFilePath(picked);
  };

  const start = async () => {
    if (!filePath) return;
    setBusy(true);
    setError(null);
    setResults(null);
    setStatus(null);
    try {
      const id = await invoke<string>("separate", { filePath });
      setJobId(id);
      poll(id);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const pct = status ? Math.max(0, Math.min(100, Math.round(status.progress))) : 0;
  const running = busy && status?.status !== "error";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      <div>
        <h1 style={{ margin: "0 0 4px" }}>{t("apps.stem_separation.name")}</h1>
        <p style={{ margin: 0, color: "var(--text-muted)", fontSize: 13 }}>
          {t("apps.stem_separation.desc")}
        </p>
      </div>

      {runtimeReady === false && (
        <Panel
          style={{
            borderColor: "var(--brand)",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
          }}
        >
          <span style={{ fontSize: 13 }}>{t("stem.needRuntime")}</span>
          <Button onClick={() => setActive("settings")}>
            {t("stem.goSettings")}
          </Button>
        </Panel>
      )}

      {/* 文件选择 */}
      <Panel>
        <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
          <Button onClick={pick} disabled={running}>
            {t("stem.pickFile")}
          </Button>
          <span
            style={{
              flex: 1,
              color: filePath ? "var(--text)" : "var(--text-muted)",
              fontSize: 13,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={filePath ?? undefined}
            aria-label={t("stem.pickFile")}
          >
            {filePath ?? t("stem.noFile")}
          </span>
          <Button
            variant="primary"
            onClick={start}
            disabled={!filePath || running || runtimeReady === false}
          >
            {running ? t("stem.separating") : t("stem.start")}
          </Button>
        </div>
        <p style={{ margin: "10px 0 0", color: "var(--text-muted)", fontSize: 12 }}>
          {t("stem.hint")}
        </p>
      </Panel>

      {error && <ErrorText>{t("stem.error", { err: error })}</ErrorText>}

      {status?.demo && (
        <p style={{ color: "var(--text-muted)", fontSize: 12, margin: 0 }}>
          {t("stem.demoMode")}
        </p>
      )}

      {/* 进度 */}
      {(running || status) && (
        <Panel>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              fontSize: 12,
              color: "var(--text-muted)",
              marginBottom: 8,
            }}
          >
            <span>{t(`stem.status.${status?.status ?? "queued"}`)}</span>
            <span>{pct}%</span>
          </div>
          <ProgressBar value={pct} />
          {status?.stems?.length ? (
            <div style={{ marginTop: 12, display: "flex", gap: 6, flexWrap: "wrap" }}>
              {status.stems.map((s) => (
                <span key={s} style={chip}>
                  {s}
                </span>
              ))}
            </div>
          ) : null}
        </Panel>
      )}

      {!filePath && !status && <EmptyState>{t("stem.idle")}</EmptyState>}

      {/* 结果 */}
      {results && (
        <Panel>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              marginBottom: 10,
            }}
          >
            <strong style={{ fontSize: 13 }}>{t("stem.files")}</strong>
            <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
              {t("stem.output")}: {displayPath(results.path)}
            </span>
          </div>
          {results.files.length === 0 ? (
            <p style={{ color: "var(--text-muted)", fontSize: 13, margin: 0 }}>
              {t("stem.emptyFiles")}
            </p>
          ) : (
            <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
              {results.files.map((f) => (
                <li
                  key={f.path}
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    padding: "8px 0",
                    borderTop: "1px solid var(--border)",
                    fontSize: 13,
                  }}
                >
                  <span>{f.name}</span>
                  <span style={{ color: "var(--text-muted)" }}>
                    {formatBytes(f.size)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Panel>
      )}

      {jobId && !running && (
        <p style={{ fontSize: 11, color: "var(--text-muted)", margin: 0 }}>
          job: {jobId}
        </p>
      )}
    </div>
  );
}

