import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

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

export default function PluginManagerPage() {
  const { t } = useTranslation();
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [sources, setSources] = useState<ScanSource[]>([]);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refreshSources = useCallback(async () => {
    try {
      const s = await invoke<ScanSource[]>("list_scan_sources");
      setSources(s ?? []);
    } catch {
      /* 非 Tauri 环境忽略 */
    }
  }, []);

  const scan = useCallback(
    async (force = false) => {
      setBusy(true);
      setError(null);
      try {
        const list = force
          ? await invoke<PluginInfo[]>("rescan_plugins")
          : await invoke<PluginInfo[]>("scan_all_plugins");
        setPlugins(list ?? []);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  useEffect(() => {
    refreshSources();
    scan();
  }, [refreshSources, scan]);

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
      const msg = String(e);
      setError(msg.includes("Pro") ? t("plugins.proRequired") : msg);
    }
  };

  const removeDir = async (path: string) => {
    setError(null);
    try {
      await invoke("remove_custom_plugin_dir", { path });
      await refreshSources();
      await scan();
    } catch (e) {
      const msg = String(e);
      setError(msg.includes("Pro") ? t("plugins.proRequired") : msg);
    }
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return plugins;
    return plugins.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        (p.vendor ?? "").toLowerCase().includes(q),
    );
  }, [plugins, query]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div>
        <h1 style={{ margin: "0 0 4px" }}>{t("apps.plugins.name")}</h1>
        <p style={{ margin: 0, color: "var(--text-muted)", fontSize: 13 }}>
          {t("apps.plugins.desc")}
        </p>
      </div>

      {/* 扫描源 */}
      <section style={panel}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 10,
          }}
        >
          <strong style={{ fontSize: 13 }}>{t("plugins.sources")}</strong>
          <button onClick={addDir} disabled={busy} style={btnSecondary}>
            {t("plugins.addDirPro")}
          </button>
        </div>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {sources.map((s) => (
            <span key={s.path} style={chip(s.is_custom)}>
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
                {s.path.replace(/^\/Users\/[^/]+/, "~")}
              </span>
              {s.is_custom && (
                <button
                  onClick={() => removeDir(s.path)}
                  style={chipClose}
                  title={t("plugins.remove")}
                >
                  ✕
                </button>
              )}
            </span>
          ))}
        </div>
      </section>

      {/* 操作条 */}
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button onClick={() => scan(false)} disabled={busy} style={btnPrimary}>
          {busy ? t("plugins.scanning") : t("plugins.scan")}
        </button>
        <button
          onClick={() => scan(true)}
          disabled={busy}
          style={btnSecondary}
        >
          {t("plugins.rescan")}
        </button>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("plugins.search")}
          style={input}
        />
        <span style={{ color: "var(--text-muted)", fontSize: 12 }}>
          {t("plugins.count", { n: filtered.length })}
        </span>
      </div>

      {busy && <ProgressBar />}
      {error && (
        <p style={{ color: "var(--danger)", fontSize: 13, margin: 0 }}>
          {t("plugins.failed", { err: error })}
        </p>
      )}
      {notice && (
        <p style={{ color: "var(--ok)", fontSize: 13, margin: 0 }}>{notice}</p>
      )}

      {/* 主表格 */}
      {!busy && filtered.length === 0 ? (
        <div style={{ ...panel, color: "var(--text-muted)", fontSize: 13 }}>
          {t("plugins.empty")}
        </div>
      ) : (
        <div style={{ ...panel, padding: 0, overflow: "hidden" }}>
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: 13,
            }}
          >
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
                      padding: "10px 12px",
                      fontWeight: 600,
                      color: "var(--text-muted)",
                      borderBottom: "1px solid var(--border)",
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
                    {p.path.replace(/^\/Users\/[^/]+/, "~")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ProgressBar() {
  return (
    <div
      style={{
        height: 4,
        background: "var(--surface-2)",
        borderRadius: 999,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: "100%",
          width: "40%",
          background: "var(--brand)",
          borderRadius: 999,
          animation: "mr-indet 1.1s ease-in-out infinite",
        }}
      />
    </div>
  );
}

const panel: React.CSSProperties = {
  padding: 16,
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  boxShadow: "var(--shadow)",
};

const cell: React.CSSProperties = {
  padding: "10px 12px",
  borderBottom: "1px solid var(--border)",
  color: "var(--text)",
  verticalAlign: "top",
};

const btnPrimary: React.CSSProperties = {
  padding: "8px 16px",
  borderRadius: 8,
  border: "none",
  background: "var(--brand)",
  color: "#fff",
  fontWeight: 600,
  cursor: "pointer",
  fontSize: 13,
};

const btnSecondary: React.CSSProperties = {
  padding: "8px 14px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  cursor: "pointer",
  fontSize: 13,
};

const input: React.CSSProperties = {
  flex: 1,
  padding: "8px 12px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  fontSize: 13,
};

const chip = (custom: boolean): React.CSSProperties => ({
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  padding: "3px 6px 3px 10px",
  borderRadius: 999,
  border: `1px solid ${custom ? "var(--brand)" : "var(--border)"}`,
  background: custom ? "transparent" : "var(--surface-2)",
  color: custom ? "var(--brand)" : "var(--text-muted)",
  fontSize: 11,
});

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
