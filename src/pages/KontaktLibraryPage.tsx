// Kontakt 库管理
//
// 列出已安装（已注册）的 Kontakt 库、磁盘上尚未注册的库，提供添加/移除注册、
// 以及为缺少 .nicnt 的库生成识别文件的能力。
//
// 数据来源（macOS）：
//   - 系统 Service Center XML      /Library/Application Support/...  （只读，写需管理员）
//   - 系统 plist                   /Library/Preferences/...          （只读，写需管理员）
//   - installed_products JSON      /Users/Shared/...                 （可写）
//   - 用户级 plist                 ~/Library/Preferences/...         （可写）
// 本应用不做提权：仅能写入后两者，需要管理员的来源会明确告知用户。

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Button, Input, Modal, SearchField, Table } from "@heroui/react";
import {
  Section,
  PrimaryButton,
  SecondaryButton,
  DangerButton,
  Empty,
  Loading,
  ErrorText,
  OkText,
  Badge,
  formatBytes,
  displayPath,
} from "../components/ui";

interface Issue {
  severity: "Info" | "Warning" | "Error";
  code: string;
  detail: string;
}

interface LibrarySources {
  has_service_center: boolean;
  has_system_plist: boolean;
  has_user_plist: boolean;
  has_installed_product_json: boolean;
}

interface KontaktLibrary {
  name: string;
  reg_key: string;
  snpid: string;
  content_path: string | null;
  visibility: number | null;
  sources: LibrarySources;
  issues: Issue[];
  user_list_index: number | null;
  content_size_bytes: number | null;
}

interface NicntFormSpec {
  name: string;
  reg_key: string;
  snpid: string;
  company: string;
  product_type: string;
  upid: string | null;
  auth_system: string | null;
  hu: string | null;
  jdx: string | null;
  visibility: number;
}

type SortKey = "name" | "snpid" | "size";

export default function KontaktLibraryPage() {
  const { t } = useTranslation();
  const [libs, setLibs] = useState<KontaktLibrary[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortAsc, setSortAsc] = useState(true);
  const [pendingRemove, setPendingRemove] = useState<KontaktLibrary | null>(null);
  const [nicntFor, setNicntFor] = useState<KontaktLibrary | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setLibs((await invoke<KontaktLibrary[]>("kontakt_inventory")) ?? []);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  /** 扫描用户选定目录，找出磁盘上尚未注册的库。 */
  const scanDir = async () => {
    setError(null);
    setNotice(null);
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: t("kontakt.selectDir"),
    });
    if (!picked || Array.isArray(picked)) return;
    setBusy(true);
    try {
      const found = await invoke<KontaktLibrary[]>("kontakt_scan", { root: picked });
      if (found.length === 0) {
        setNotice(t("kontakt.empty"));
      } else {
        // 合并进现有列表（按 reg_key 去重，磁盘结果优先展示）
        setLibs((prev) => {
          const byKey = new Map(prev.map((l) => [l.reg_key, l]));
          for (const f of found) byKey.set(f.reg_key, f);
          return [...byKey.values()];
        });
        setNotice(t("kontakt.added", { name: found[0].name }));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 选择 .nicnt 文件并注册该库。 */
  const addLibrary = async () => {
    setError(null);
    const picked = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Kontakt", extensions: ["nicnt"] }],
    });
    if (typeof picked !== "string") return;
    setBusy(true);
    try {
      const lib = await invoke<KontaktLibrary>("kontakt_add", { nicntPath: picked });
      setNotice(t("kontakt.added", { name: lib.name }));
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doRemove = async () => {
    const lib = pendingRemove;
    setPendingRemove(null);
    if (!lib) return;
    setError(null);
    try {
      await invoke("kontakt_remove", { regKey: lib.reg_key, name: lib.name });
      setNotice(t("kontakt.removed"));
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const reveal = async (p: string) => {
    try {
      await invoke("reveal_in_finder", { path: p });
    } catch (e) {
      setError(String(e));
    }
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = libs.filter(
      (l) =>
        !q ||
        l.name.toLowerCase().includes(q) ||
        l.snpid.toLowerCase().includes(q) ||
        l.reg_key.toLowerCase().includes(q),
    );
    const dir = sortAsc ? 1 : -1;
    return [...list].sort((a, b) => {
      if (sortKey === "size") {
        return ((a.content_size_bytes ?? 0) - (b.content_size_bytes ?? 0)) * dir;
      }
      if (sortKey === "snpid") {
        return a.snpid.localeCompare(b.snpid) * dir;
      }
      return a.name.localeCompare(b.name) * dir;
    });
  }, [libs, query, sortKey, sortAsc]);

  const totalSize = useMemo(
    () => libs.reduce((s, l) => s + (l.content_size_bytes ?? 0), 0),
    [libs],
  );

  const toggleSort = (k: SortKey) => {
    if (sortKey === k) setSortAsc((v) => !v);
    else {
      setSortKey(k);
      setSortAsc(true);
    }
  };

  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">{t("kontakt.title")}</h1>
          <p className="mt-1 text-sm opacity-70">
            {t("kontakt.total", { n: libs.length, size: formatBytes(totalSize) })}
          </p>
        </div>
        <div className="flex gap-2">
          <SecondaryButton onPress={scanDir} isDisabled={busy}>
            {t("kontakt.scanDir")}
          </SecondaryButton>
          <PrimaryButton onPress={addLibrary} isDisabled={busy}>
            {t("kontakt.addLib")}
          </PrimaryButton>
          <Button variant="ghost" onPress={load} isDisabled={loading}>
            {t("common.refresh")}
          </Button>
        </div>
      </div>

      <SearchField.Root value={query} onChange={setQuery} className="max-w-md">
        <SearchField.Group>
          <SearchField.SearchIcon />
          <SearchField.Input placeholder={t("common.search")} />
          <SearchField.ClearButton />
        </SearchField.Group>
      </SearchField.Root>

      {loading && <Loading label={t("kontakt.scanning")} />}
      {error && <ErrorText>{error}</ErrorText>}
      {notice && <OkText>{notice}</OkText>}

      {!loading && filtered.length === 0 && !error && <Empty>{t("kontakt.empty")}</Empty>}

      {filtered.length > 0 && (
        <Section>
          <Table>
            <Table.Header>
              <Table.Column isRowHeader>
                <SortHeader
                  label={t("kontakt.cols.name")}
                  active={sortKey === "name"}
                  asc={sortAsc}
                  onPress={() => toggleSort("name")}
                />
              </Table.Column>
              <Table.Column>
                <SortHeader
                  label={t("kontakt.cols.snpid")}
                  active={sortKey === "snpid"}
                  asc={sortAsc}
                  onPress={() => toggleSort("snpid")}
                />
              </Table.Column>
              <Table.Column>
                <SortHeader
                  label={t("kontakt.cols.size")}
                  active={sortKey === "size"}
                  asc={sortAsc}
                  onPress={() => toggleSort("size")}
                />
              </Table.Column>
              <Table.Column>{t("kontakt.cols.path")}</Table.Column>
              <Table.Column>{t("kontakt.cols.status")}</Table.Column>
              <Table.Column>{t("kontakt.cols.actions")}</Table.Column>
            </Table.Header>
            <Table.Body>
              {filtered.map((l) => (
                <Table.Row key={`${l.reg_key}-${l.snpid}`}>
                  <Table.Cell>{l.name}</Table.Cell>
                  <Table.Cell>
                    <code className="text-xs">{l.snpid || "—"}</code>
                  </Table.Cell>
                  <Table.Cell>{formatBytes(l.content_size_bytes)}</Table.Cell>
                  <Table.Cell className="max-w-xs">
                    <span className="text-xs opacity-70" title={l.content_path ?? ""}>
                      {l.content_path ? displayPath(l.content_path) : "—"}
                    </span>
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex flex-wrap gap-1">
                      {isRegistered(l) ? (
                        <Badge color="success">{t("kontakt.registered")}</Badge>
                      ) : (
                        <Badge>{t("kontakt.diskOnly")}</Badge>
                      )}
                      {l.issues.slice(0, 2).map((i) => (
                        <Badge
                          key={i.code}
                          color={
                            i.severity === "Error"
                              ? "danger"
                              : i.severity === "Warning"
                                ? "warning"
                                : "default"
                          }
                        >
                          {t(`kontakt.issue.${i.code}`, { defaultValue: i.code })}
                        </Badge>
                      ))}
                    </div>
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex gap-1">
                      {!hasNicnt(l) && (
                        <Button size="sm" variant="secondary" onPress={() => setNicntFor(l)}>
                          {t("kontakt.createNicnt")}
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        onPress={() => l.content_path && reveal(l.content_path)}
                        isDisabled={!l.content_path}
                      >
                        {t("kontakt.openInFinder")}
                      </Button>
                      <DangerButton onPress={() => setPendingRemove(l)}>
                        {t("kontakt.remove")}
                      </DangerButton>
                    </div>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table>
        </Section>
      )}

      {/* 移除注册：只删注册记录，不动磁盘文件 */}
      <Modal.Root isOpen={!!pendingRemove} onOpenChange={(o) => !o && setPendingRemove(null)}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>{t("kontakt.remove")}</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <p className="text-sm">{pendingRemove?.name}</p>
                <p className="mt-2 text-sm opacity-80">{t("kontakt.removeConfirm")}</p>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="secondary" onPress={() => setPendingRemove(null)}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onPress={doRemove}>
                  {t("common.confirm")}
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>

      {/* 创建 .nicnt 表单 */}
      <NicntDialog
        lib={nicntFor}
        onClose={() => setNicntFor(null)}
        onDone={(p) => {
          setNotice(t("kontakt.nicntCreated", { path: displayPath(p) }));
          setNicntFor(null);
        }}
        onError={setError}
      />
    </div>
  );
}

/** 可点击的表头，展示排序方向。 */
function SortHeader({
  label,
  active,
  asc,
  onPress,
}: {
  label: string;
  active: boolean;
  asc: boolean;
  onPress: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onPress}
      className="flex items-center gap-1 font-medium hover:text-accent"
      aria-sort={active ? (asc ? "ascending" : "descending") : "none"}
    >
      {label}
      <span className="text-[10px] opacity-60">{active ? (asc ? "▲" : "▼") : "↕"}</span>
    </button>
  );
}

function isRegistered(l: KontaktLibrary): boolean {
  return (
    l.sources.has_service_center ||
    l.sources.has_system_plist ||
    l.sources.has_user_plist ||
    l.sources.has_installed_product_json
  );
}

/** 无任何来源表明存在 .nicnt 时，提示可创建。 */
function hasNicnt(l: KontaktLibrary): boolean {
  return l.sources.has_service_center || l.sources.has_installed_product_json;
}

/** 表单字段：标签 + 控件。 */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-xs opacity-70">{label}</span>
      {children}
    </label>
  );
}

function NicntDialog({
  lib,
  onClose,
  onDone,
  onError,
}: {
  lib: KontaktLibrary | null;
  onClose: () => void;
  onDone: (path: string) => void;
  onError: (msg: string) => void;
}) {
  const { t } = useTranslation();
  const [spec, setSpec] = useState<NicntFormSpec | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!lib) {
      setSpec(null);
      return;
    }
    invoke<NicntFormSpec>("kontakt_nicnt_spec")
      .then((base) =>
        setSpec({
          ...base,
          name: lib.name,
          reg_key: lib.reg_key,
          snpid: lib.snpid,
        }),
      )
      .catch((e) => onError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lib]);

  const create = async () => {
    if (!spec || !lib?.content_path) return;
    setBusy(true);
    try {
      const p = await invoke<string>("kontakt_create_nicnt", {
        dir: lib.content_path,
        spec,
      });
      onDone(p);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const set = (k: keyof NicntFormSpec, v: string | number) =>
    setSpec((s) => (s ? { ...s, [k]: v } : s));

  return (
    <Modal.Root isOpen={!!lib} onOpenChange={(o) => !o && onClose()}>
      <Modal.Backdrop>
        <Modal.Container>
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>{t("kontakt.createNicnt")}</Modal.Heading>
            </Modal.Header>
            <Modal.Body>
              <p className="mb-3 text-xs opacity-70">{t("kontakt.nicntHint")}</p>
              {spec && (
                <div className="flex flex-col gap-3">
                  <Field label={t("kontakt.nicntForm.name")}>
                    <Input value={spec.name} onChange={(e) => set("name", e.target.value)} />
                  </Field>
                  <Field label={t("kontakt.nicntForm.regKey")}>
                    <Input value={spec.reg_key} onChange={(e) => set("reg_key", e.target.value)} />
                  </Field>
                  <Field label={t("kontakt.nicntForm.snpid")}>
                    <Input value={spec.snpid} onChange={(e) => set("snpid", e.target.value)} />
                  </Field>
                  <Field label={t("kontakt.nicntForm.company")}>
                    <Input value={spec.company} onChange={(e) => set("company", e.target.value)} />
                  </Field>
                  <Field label={t("kontakt.nicntForm.upid")}>
                    <Input
                      value={spec.upid ?? ""}
                      onChange={(e) => set("upid", e.target.value)}
                    />
                  </Field>
                </div>
              )}
            </Modal.Body>
            <Modal.Footer>
              <Button variant="secondary" onPress={onClose}>
                {t("common.cancel")}
              </Button>
              <PrimaryButton onPress={create} isDisabled={busy || !spec?.name}>
                {t("common.confirm")}
              </PrimaryButton>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal.Root>
  );
}
