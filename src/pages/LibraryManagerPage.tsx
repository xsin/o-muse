import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import {
  Panel,
  Button,
  cell,
  displayPath,
  ErrorText,
  EmptyState,
  IndeterminateBar,
} from "../components/ui";

interface Diagnostic {
  MissingPath?: string;
  DuplicateSnpid?: string;
  DisconnectedVolume?: string;
  MissingMetadata?: string;
}

interface LibraryEntry {
  snpid: string;
  name: string;
  path: string | null;
  registered: boolean;
  diagnostics: Diagnostic[];
}

export default function LibraryManagerPage() {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<LibraryEntry[]>("scan_libraries");
      setEntries(result ?? []);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    scan();
  }, [scan]);

  /** 把 Rust 枚举变体名翻译为本地化文案；未知变体回退为变体名。 */
  const describeDiagnostic = (d: Diagnostic): string => {
    const [kind, value] = Object.entries(d)[0] ?? [];
    if (!kind) return "";
    const key = `library.diag.${kind}`;
    const label = t(key, { defaultValue: kind });
    return value ? `${label}: ${value}` : label;
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <h1 style={{ margin: 0 }}>{t("apps.libraries.name")}</h1>
          <Button onClick={scan} disabled={loading}>
            {loading ? t("library.scanning") : t("library.scan")}
          </Button>
        </div>
        <p style={{ color: "var(--text-muted)", marginTop: 6, fontSize: 13 }}>
          {t("apps.libraries.desc")}
        </p>
      </div>

      {loading && <IndeterminateBar />}
      {error && <ErrorText>{error}</ErrorText>}

      {!loading && entries.length === 0 && !error && (
        <EmptyState>{t("library.empty")}</EmptyState>
      )}

      {entries.length > 0 && (
        <Panel style={{ padding: 0, overflow: "hidden" }}>
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: 13,
            }}
          >
            <thead>
              <tr style={{ textAlign: "left", background: "var(--surface-2)" }}>
                {[
                  t("library.cols.name"),
                  t("library.cols.snpid"),
                  t("library.cols.path"),
                  t("library.cols.status"),
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
              {entries.map((e, i) => (
                <tr key={`${e.snpid}-${i}`}>
                  <td style={cell}>{e.name}</td>
                  <td style={cell}>{e.snpid}</td>
                  <td style={{ ...cell, color: "var(--text-muted)" }}>
                    {e.path ? displayPath(e.path) : "—"}
                  </td>
                  <td style={cell}>
                    <span
                      style={{
                        color: e.registered ? "var(--ok)" : "var(--text-muted)",
                      }}
                    >
                      {e.registered
                        ? t("library.registered")
                        : t("library.unregistered")}
                    </span>
                    {e.diagnostics.length > 0 && (
                      <div
                        style={{
                          marginTop: 4,
                          fontSize: 12,
                          color: "var(--danger)",
                        }}
                      >
                        {e.diagnostics.map((d, di) => (
                          <div key={di}>{describeDiagnostic(d)}</div>
                        ))}
                      </div>
                    )}
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
