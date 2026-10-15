import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

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

  async function scan() {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<LibraryEntry[]>("scan_libraries");
      setEntries(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    scan();
  }, []);

  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <h1 style={{ marginTop: 0 }}>{t("apps.libraries.name")}</h1>
        <button onClick={scan} disabled={loading}>
          {loading ? t("library.scanning") : t("library.scan")}
        </button>
      </div>
      <p style={{ color: "var(--text-muted)", marginTop: 0 }}>
        {t("apps.libraries.desc")}
      </p>

      {error && <p style={{ color: "var(--danger)" }}>{error}</p>}
      {!loading && entries.length === 0 && !error && (
        <p style={{ color: "var(--text-muted)" }}>{t("library.empty")}</p>
      )}

      {entries.length > 0 && (
        <table
          style={{
            width: "100%",
            borderCollapse: "collapse",
            marginTop: 16,
            fontSize: 14,
          }}
        >
          <thead>
            <tr style={{ textAlign: "left", color: "var(--text-muted)" }}>
              <th style={cell}>{t("library.cols.name")}</th>
              <th style={cell}>{t("library.cols.snpid")}</th>
              <th style={cell}>{t("library.cols.path")}</th>
              <th style={cell}>{t("library.cols.status")}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e, i) => (
              <tr key={`${e.snpid}-${i}`} style={{ borderTop: "1px solid var(--border)" }}>
                <td style={cell}>{e.name}</td>
                <td style={cell}>{e.snpid}</td>
                <td style={{ ...cell, color: "var(--text-muted)" }}>
                  {e.path ?? "—"}
                </td>
                <td style={cell}>
                  <span style={{ color: e.registered ? "var(--ok)" : "var(--text-muted)" }}>
                    {e.registered
                      ? t("library.registered")
                      : t("library.unregistered")}
                  </span>
                  {e.diagnostics.length > 0 && (
                    <div style={{ marginTop: 4, fontSize: 12, color: "var(--danger)" }}>
                      {e.diagnostics.map((d, di) => (
                        <div key={di}>
                          {Object.entries(d)
                            .map(([k, v]) => `${k}: ${v}`)
                            .join("")}
                        </div>
                      ))}
                    </div>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

const cell: React.CSSProperties = {
  padding: "10px 8px",
  verticalAlign: "top",
};
