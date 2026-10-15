import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Panel,
  Button,
  cell,
  chip,
  input,
  displayPath,
  ErrorText,
  EmptyState,
  IndeterminateBar,
} from "../components/ui";

interface PluginInfo {
  name: string;
  vendor: string | null;
  version: string | null;
  format: "AU" | "VST" | "VST3" | "AAX" | "CLAP" | "Auto";
  path: string;
  bundle_id: string | null;
  last_modified: string;
  file_size: number;
}

interface ScanSource {
  path: string;
  plugin_format: "AU" | "VST" | "VST3" | "AAX" | "CLAP" | "Auto";
  recursive: boolean;
  enabled: boolean;
  is_custom: boolean;
}

interface ScanResult {
  plugins: PluginInfo[];
  hits: number;
  misses: number;
  cache_warning: string | null;
}

const FORMATS = ["AU", "VST", "VST3", "AAX", "CLAP"] as const;

export default function PluginManagerPage() {
  const { t } = useTranslation();
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [sources, setSources] = useState<ScanSource[]>([]);
  const [query, setQuery] = useState("");
  const [format, setFormat] = useState<string>("ALL");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);

  const refreshSources = useCallback(async () => {
    try {
      setSources(await invoke<ScanSource[]>("list_scan_sources"));
    } catch (e) {
      // 不再静默：源列表拉取失败要明确告知（review 修复）
      setError(String(e));
    }
  }, []);

  const scan = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const res = await invoke<ScanResult>("scan_all_plugins");
      setPlugins(res.plugins ?? []);
      setWarning(res.cache_warning);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  // 进入页面时挂载源列表；**不自动全盘扫描**（避免打开即长耗时且无法取消，
  // review 修复）。由用户点击"扫描"触发。
  useEffect(() => {
    refreshSources();
  }, [refreshSources]);

  const addDir = async () => {
    setNotice(null);
    setError(null);
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: t("plugins.pickDir"),
      });
      if (!picked || Array.isArray(picked)) return;
      await invoke<ScanSource>("add_custom_plugin_dir", { path: picked });
      setNotice(t("plugins.added"));
      await refreshSources();
      await scan();
    } catch (e) {
      setError(t("plugins.proRequired"));
    }
  };

  const removeDir = async (path: string) => {
    setError(null);
    try {
      await invoke("remove_custom_plugin_dir", { path });
      await refreshSources();
      await scan();
    } catch (e) {
      setError(t("plugins.proRequired"));
    }
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return plugins.filter((p) => {
      if (format !== "ALL" && p.format !== format) return false;
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        (p.vendor ?? "").toLowerCase().includes(q)
      );
    });
  }, [plugins, query, format]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      <div>
        <h1 style={{ margin: "0 0 4px" }}>{t("apps.plugins.name")}</h1>
        <p style={{ margin: 0, color: "var(--text-muted)", fontSize: 13 }}>
          {t("apps.plugins.desc")}
        </p>
      </div>

      {/* 扫描源 */}
      <Panel>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 10,
          }}
        >
          <strong style={{ fontSize: 13 }}>{t("plugins.sources")}</strong>
          <Button onClick={addDir} disabled={busy}>
            {t("plugins.addDirPro")}
          </Button>
        </div>
        {sources.length === 0 ? (
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>—</span>
        ) : (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            {sources.map((s) => (
              <span key={s.path} style={chip}>
                <span style={{ opacity: 0.85 }}>{s.plugin_format}</span>
                <span
                  title={s.path}
                  style={{
                    maxWidth: 260,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {displayPath(s.path)}
                </span>
                {s.is_custom && (
                  <button
                    onClick={() => removeDir(s.path)}
                    style={chipClose}
                    title={t("plugins.remove")}
                    aria-label={t("plugins.remove")}
                  >
                    ✕
                  </button>
                )}
              </span>
            ))}
          </div>
        )}
      </Panel>

      {/* 操作条 */}
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <Button variant="primary" onClick={scan} disabled={busy}>
          {busy ? t("plugins.scanning") : t("plugins.scan")}
        </Button>
        <select
          value={format}
          onChange={(e) => setFormat(e.target.value)}
          aria-label={t("plugins.cols.format")}
        >
          <option value="ALL">{t("plugins.cols.format")}: *</option>
          {FORMATS.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("plugins.search")}
          style={input}
          aria-label={t("plugins.search")}
        />
        <span style={{ color: "var(--text-muted)", fontSize: 12 }}>
          {t("plugins.count", { n: filtered.length })}
        </span>
      </div>

      {busy && <IndeterminateBar />}
      {error && <ErrorText>{t("plugins.failed", { err: error })}</ErrorText>}
      {warning && <ErrorText>{warning}</ErrorText>}
      {notice && (
        <span style={{ color: "var(--ok)", fontSize: 13 }}>{notice}</span>
      )}

      {/* 主表格 */}
      {!busy && plugins.length === 0 ? (
        <EmptyState>{t("plugins.empty")}</EmptyState>
      ) : plugins.length > 0 && filtered.length === 0 ? (
        <EmptyState>{t("plugins.empty")}</EmptyState>
      ) : (
        <Panel style={{ padding: 0, overflow: "hidden" }}>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
            <thead>
              <tr style={{ background: "var(--surface-2)", textAlign: "left" }}>
                {[
                  t("plugins.cols.name"),
                  t("plugins.cols.vendor"),
                  t("plugins.cols.version"),
                  t("plugins.cols.format"),
                  t("plugins.cols.path"),
                ].map((h) => (
                  <th
                    key={h}
                    style={{
                      ...cell,
                      fontWeight: 600,
                      color: "var(--text-muted)",
                    }}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map((p) => (
                <tr key={p.path}>
                  <td style={cell}>{p.name}</td>
                  <td style={cell}>{p.vendor ?? "—"}</td>
                  <td style={cell}>{p.version ?? "—"}</td>
                  <td style={cell}>
                    <span style={formatBadge}>{p.format}</span>
                  </td>
                  <td
                    style={{
                      ...cell,
                      color: "var(--text-muted)",
                      maxWidth: 320,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                    title={p.path}
                  >
                    {displayPath(p.path)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      )}
    </div>
  );
}

const chipClose: React.CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--text-muted)",
  cursor: "pointer",
  padding: "0 2px",
  fontSize: 11,
  lineHeight: 1,
};

const formatBadge: React.CSSProperties = {
  display: "inline-block",
  padding: "1px 7px",
  borderRadius: 999,
  background: "var(--surface-2)",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 600,
};
