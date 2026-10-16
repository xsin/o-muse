// 分轨分离（Stem Separation）
//
// 两条能力：
//   1. 分析卡片  —— 选完文件即可点「分析」，秒级返回调性（含大/小调 + Camelot）与 BPM
//   2. 分轨卡片  —— 真正跑 Demucs 分轨，显示进度、阶段、实际推理设备与产出文件
//
// 后端契约（python-backend/server.py）：
//   POST /separate  |  POST /analyze   → {job_id}
//   GET  /status/{id}                  → 状态 + 进度 + stage + analysis + device
//   GET  /stems/{id}                   → 分轨文件清单
// Rust 端把上述 HTTP 包装成 `separate` / `stem_status` / `stem_results` 三个命令；
// 「仅分析」复用 `separate` 命令的 `analyzeOnly` 参数（见 commands/stem_separation.rs）。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Button,
  Chip,
  Separator,
  Table,
  Tabs,
} from "@heroui/react";
import { useUI } from "../store/useUI";
import {
  Section,
  PrimaryButton,
  SecondaryButton,
  Empty,
  ErrorText,
  OkText,
  Progress,
  Badge,
  formatBytes,
  displayPath,
} from "../components/ui";

const AUDIO_EXTENSIONS = ["mp3", "wav", "flac", "m4a", "aac", "ogg", "aiff", "aif"];

type JobKind = "separate" | "analyze";
type JobStatus = "queued" | "processing" | "done" | "error" | "demo_done";

interface KeyResult {
  key: string;
  mode: "major" | "minor";
  camelot: string;
  confidence: number;
}

interface TempoResult {
  bpm: number;
  confidence: number;
}

interface AnalysisResult {
  key: KeyResult;
  tempo: TempoResult;
  duration_sec: number;
}

interface StatusResponse {
  job_id: string;
  kind: JobKind | null;
  status: JobStatus;
  stage: string | null;
  progress: number;
  message: string | null;
  demo: boolean;
  stems: string[];
  analysis: AnalysisResult | null;
  device: string | null;
  device_note: string | null;
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

interface RuntimeStatus {
  stage: string;
}

const TERMINAL: JobStatus[] = ["done", "error", "demo_done"];

function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function percent(value: number): number {
  return Math.max(0, Math.min(100, Math.round(value * 100)));
}

/** 大号读数（调性 / BPM / 时长共用）。 */
function Readout({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className="min-w-24">
      <div className="text-xs uppercase tracking-wide opacity-70">{label}</div>
      <div className="flex items-baseline gap-1">
        <span className="text-4xl font-semibold tabular-nums text-accent">{value}</span>
        {unit && <span className="text-sm opacity-70">{unit}</span>}
      </div>
    </div>
  );
}

export default function StemSeparationPage() {
  const { t } = useTranslation();
  const setActive = useUI((s) => s.setActive);

  const [runtimeReady, setRuntimeReady] = useState<boolean | null>(null);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const [jobKind, setJobKind] = useState<JobKind>("separate");
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [results, setResults] = useState<StemsResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [tab, setTab] = useState("files");
  const tRef = useRef(t);
  tRef.current = t;

  const running = jobId !== null;

  // 运行时是否就绪：未就绪时先给引导，而不是让用户点下去再失败
  useEffect(() => {
    invoke<RuntimeStatus>("runtime_status")
      .then((s) => setRuntimeReady(s.stage === "ready"))
      .catch(() => setRuntimeReady(null));
  }, []);

  // 轮询任务状态。终态时清空 jobId → 依赖变化触发本 effect 的清理，停止轮询。
  useEffect(() => {
    if (!jobId) return;
    let cancelled = false;
    let timer: number | null = null;

    const stop = () => {
      if (timer !== null) {
        window.clearInterval(timer);
        timer = null;
      }
    };

    const tick = async () => {
      try {
        const s = await invoke<StatusResponse>("stem_status", { jobId });
        if (cancelled) return;
        setStatus(s);
        if (s.analysis) setAnalysis(s.analysis);
        if (s.status === "error") {
          setError(s.message ?? tRef.current("stem.unknownError"));
          setJobId(null);
          stop();
          return;
        }
        if (TERMINAL.includes(s.status)) {
          // 分轨成功后的分析失败只会落在 message 上，不改变成功终态
          if (s.kind !== "analyze" && s.message) setWarning(s.message);
          if (s.kind !== "analyze") {
            try {
              const r = await invoke<StemsResponse>("stem_results", { jobId });
              if (!cancelled) setResults(r);
            } catch (e) {
              if (!cancelled) setError(String(e));
            }
          }
          setJobId(null);
          stop();
        }
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
        setJobId(null);
        stop();
      }
    };

    timer = window.setInterval(() => void tick(), 800);
    void tick();
    return () => {
      cancelled = true;
      stop();
    };
  }, [jobId]);

  const pick = useCallback(async () => {
    const picked = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
    });
    if (typeof picked === "string") {
      setFilePath(picked);
      setAnalysis(null);
      setResults(null);
      setStatus(null);
      setError(null);
      setWarning(null);
    }
  }, []);

  const submit = useCallback(
    async (kind: JobKind) => {
      if (!filePath) return;
      setError(null);
      setWarning(null);
      setStatus(null);
      setResults(null);
      if (kind === "analyze") setAnalysis(null);
      setJobKind(kind);
      try {
        const id = await invoke<string>("separate", {
          filePath,
          analyzeOnly: kind === "analyze",
        });
        setJobId(id);
      } catch (e) {
        setError(String(e));
      }
    },
    [filePath],
  );

  const reveal = useCallback(async (path: string) => {
    try {
      await invoke("reveal_in_finder", { path });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const stageKey = status?.stage ?? "queued";
  const stageText = useMemo(
    () => t(`stem.stage.${stageKey}`, { defaultValue: stageKey }),
    [t, stageKey],
  );
  const progressValue =
    status?.status === "done" || status?.status === "demo_done"
      ? 100
      : Math.max(0, Math.min(100, status?.progress ?? 0));
  const bpm = analysis?.tempo.bpm ?? 0;

  return (
    <div className="flex flex-col gap-4">
      <header>
        <h1 className="m-0 text-xl font-semibold">{t("apps.stem_separation.name")}</h1>
        <p className="m-0 text-sm opacity-70">{t("apps.stem_separation.desc")}</p>
      </header>

      {runtimeReady === false && (
        <Alert.Root status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>{t("runtime.title")}</Alert.Title>
            <Alert.Description>{t("stem.needRuntime")}</Alert.Description>
            <div className="mt-2">
              <Button variant="secondary" size="sm" onPress={() => setActive("settings")}>
                {t("stem.goSettings")}
              </Button>
            </div>
          </Alert.Content>
        </Alert.Root>
      )}

      {/* 文件选择 */}
      <Section title={t("stem.sourceTitle")}>
        <div className="flex flex-wrap items-center gap-3">
          <SecondaryButton onPress={() => void pick()} isDisabled={running}>
            {t("stem.pickFile")}
          </SecondaryButton>
          <span
            className={`min-w-0 flex-1 truncate text-sm ${filePath ? "" : "opacity-70"}`}
            title={filePath ?? undefined}
          >
            {filePath ? displayPath(filePath) : t("stem.noFile")}
          </span>
        </div>
        <p className="mt-3 text-xs opacity-70">{t("stem.hint")}</p>
      </Section>

      {/* 分析卡片 */}
      <Section
        title={t("stem.analysisTitle")}
        actions={
          <div className="flex items-center gap-2">
            {analysis && (
              <Badge color="accent">{t("stem.camelot", { code: analysis.key.camelot })}</Badge>
            )}
            <PrimaryButton
              onPress={() => void submit("analyze")}
              isDisabled={!filePath || running || runtimeReady === false}
            >
              {running && jobKind === "analyze" ? t("stem.analyzing") : t("stem.analyze")}
            </PrimaryButton>
          </div>
        }
      >
        {analysis ? (
          <>
            <div className="flex flex-wrap items-center gap-x-6 gap-y-4">
              <Readout
                label={t("stem.key")}
                value={analysis.key.key}
                unit={t(`stem.mode.${analysis.key.mode}`)}
              />
              <Separator orientation="vertical" className="hidden h-14 sm:block" />
              <div>
                <Readout label={t("stem.bpm")} value={bpm.toFixed(1)} unit="BPM" />
                <div className="mt-1 flex flex-wrap gap-2">
                  {bpm >= 80 && (
                    <Badge>{t("stem.halfTime", { bpm: (bpm / 2).toFixed(1) })}</Badge>
                  )}
                  {bpm > 0 && bpm <= 160 && (
                    <Badge>{t("stem.doubleTime", { bpm: (bpm * 2).toFixed(1) })}</Badge>
                  )}
                </div>
              </div>
              <Separator orientation="vertical" className="hidden h-14 sm:block" />
              <Readout
                label={t("stem.duration")}
                value={formatDuration(analysis.duration_sec)}
              />
            </div>
            <div className="mt-5 grid gap-3 sm:grid-cols-2">
              <Progress
                value={percent(analysis.key.confidence)}
                label={t("stem.keyConfidence")}
              />
              <Progress
                value={percent(analysis.tempo.confidence)}
                label={t("stem.tempoConfidence")}
              />
            </div>
            <p className="mt-3 text-xs opacity-60">{t("stem.analysisHint")}</p>
          </>
        ) : (
          <Empty>{t("stem.noAnalysis")}</Empty>
        )}
      </Section>

      {/* 分轨卡片 */}
      <Section
        title={t("stem.separationTitle")}
        actions={
          <PrimaryButton
            onPress={() => void submit("separate")}
            isDisabled={!filePath || running || runtimeReady === false}
          >
            {running && jobKind === "separate" ? t("stem.separating") : t("stem.start")}
          </PrimaryButton>
        }
      >
        {!filePath ? (
          <Empty>{t("stem.idle")}</Empty>
        ) : !status ? (
          <Empty>{t("stem.notStarted")}</Empty>
        ) : (
          <>
            <Progress value={progressValue} label={stageText} />
            <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
              <Chip size="sm" color="default">
                <Chip.Label>{`${Math.round(progressValue)}%`}</Chip.Label>
              </Chip>
              {status.device && (
                <Badge color={status.device === "mps" ? "success" : "default"}>
                  {status.device === "mps" ? t("stem.deviceMps") : t("stem.deviceCpu")}
                </Badge>
              )}
              {status.device_note && (
                <span className="text-warning opacity-90">
                  {t("stem.deviceFallback", { note: status.device_note })}
                </span>
              )}
              {status.demo && <span className="opacity-70">{t("stem.demoMode")}</span>}
            </div>

            {error && (
              <div className="mt-3">
                <ErrorText>{t("stem.error", { err: error })}</ErrorText>
              </div>
            )}
            {warning && (
              <div className="mt-3">
                <ErrorText>{warning}</ErrorText>
              </div>
            )}
            {!error && !warning && !running && status && !status.demo && (
              <div className="mt-3">
                <OkText>{t("stem.separationDone")}</OkText>
              </div>
            )}

            {results && results.files.length > 0 && (
              <>
                <Separator className="my-4" />
                <Tabs.Root selectedKey={tab} onSelectionChange={(k) => setTab(String(k))}>
                  <Tabs.List aria-label={t("stem.separationTitle")}>
                    <Tabs.Tab id="files">{t("stem.tabFiles")}</Tabs.Tab>
                    <Tabs.Tab id="info">{t("stem.tabInfo")}</Tabs.Tab>
                  </Tabs.List>
                </Tabs.Root>

                {tab === "files" ? (
                  <div className="mt-3">
                    <Table>
                      <Table.Header>
                        <Table.Column isRowHeader>{t("stem.cols.name")}</Table.Column>
                        <Table.Column>{t("stem.cols.size")}</Table.Column>
                        <Table.Column>{t("stem.cols.path")}</Table.Column>
                        <Table.Column>{t("stem.cols.actions")}</Table.Column>
                      </Table.Header>
                      <Table.Body>
                        {results.files.map((f) => (
                          <Table.Row key={f.path} id={f.path}>
                            <Table.Cell>
                              <span className="font-medium">
                                {t(`stem.stemNames.${f.name}`, { defaultValue: f.name })}
                              </span>
                            </Table.Cell>
                            <Table.Cell className="tabular-nums">
                              {formatBytes(f.size)}
                            </Table.Cell>
                            <Table.Cell>
                              <span
                                className="block max-w-72 truncate text-xs opacity-70"
                                title={f.path}
                              >
                                {displayPath(f.path)}
                              </span>
                            </Table.Cell>
                            <Table.Cell>
                              <Button
                                size="sm"
                                variant="secondary"
                                onPress={() => void reveal(f.path)}
                              >
                                {t("stem.openFolder")}
                              </Button>
                            </Table.Cell>
                          </Table.Row>
                        ))}
                      </Table.Body>
                    </Table>
                  </div>
                ) : (
                  <dl className="mt-3 grid gap-2 text-sm sm:grid-cols-2">
                    <div>
                      <dt className="text-xs opacity-70">{t("stem.output")}</dt>
                      <dd className="truncate" title={results.path}>
                        {displayPath(results.path)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs opacity-70">{t("stem.cols.name")}</dt>
                      <dd>{t("stem.stemsReady", { n: results.files.length })}</dd>
                    </div>
                    <div>
                      <dt className="text-xs opacity-70">{t("stem.device")}</dt>
                      <dd>{status?.device ?? "—"}</dd>
                    </div>
                    <div>
                      <dt className="text-xs opacity-70">{t("stem.jobId")}</dt>
                      <dd className="truncate" title={status?.job_id}>
                        {status?.job_id ?? "—"}
                      </dd>
                    </div>
                  </dl>
                )}
              </>
            )}
          </>
        )}
      </Section>
    </div>
  );
}
