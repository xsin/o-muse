// Kontakt 库管理
//
// 列出已安装（已注册）的 Kontakt 库、磁盘上尚未注册的库，提供添加/移除注册、
// 以及为缺少 .nicnt 的库生成识别文件的能力。
//
// 数据来源（macOS）：
//   - 系统 Service Center XML      /Library/Application Support/...  （写需管理员授权）
//   - 系统 plist                   /Library/Preferences/...          （写需管理员授权）
//   - installed_products JSON      /Users/Shared/...                 （可写）
//   - 用户级 plist                 ~/Library/Preferences/...         （可写）
// 免提权部分（后两者）先完成；系统级来源缺什么由 `kontakt_privilege_plan`
// **逐条列出具体路径与原因**，用户确认后经系统管理员密码框写入，绝不静默提权。

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

/** 提权计划里的一条：具体文件 + 动作 + 原因（reason 为 i18n key 的回退文案）。 */
interface PrivilegeEntry {
  path: string;
  action: "write" | "remove";
  reason: string;
  reason_key: string;
}

interface PrivilegePlan {
  needed: boolean;
  entries: PrivilegeEntry[];
}

// ── 后端字段规整 ─────────────────────────────────────────────
//
// 页面只在**数据入口**做一次规整，之后渲染层拿到的永远是完整结构。
// 目的：`issues` / `sources` 缺失、`name` 为 null、`content_size_bytes` 变成
// 字符串等后端形状变化，都只会让某个格子显示默认值，而不会让整页白屏。

function asString(v: unknown): string {
  if (typeof v === "string") return v;
  if (v == null) return "";
  return String(v);
}

function asBool(v: unknown): boolean {
  return v === true;
}

function asNumberOrNull(v: unknown): number | null {
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

/** 严重级别做成大小写无关：后端改用 "warning"/"WARNING" 也不会错色或崩。 */
function asSeverity(v: unknown): Issue["severity"] {
  switch (asString(v).toLowerCase()) {
    case "error":
      return "Error";
    case "warning":
      return "Warning";
    default:
      return "Info";
  }
}

function normalizeIssue(raw: unknown): Issue | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const code = asString(r.code);
  if (!code) return null;
  return { severity: asSeverity(r.severity), code, detail: asString(r.detail) };
}

/** 一条后端记录 → 前端结构；必要字段全缺时返回 null（该条丢弃，不影响其余条目）。 */
function normalizeLibrary(raw: unknown): KontaktLibrary | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const sources = (r.sources && typeof r.sources === "object" ? r.sources : {}) as Record<
    string,
    unknown
  >;
  const name = asString(r.name);
  const regKey = asString(r.reg_key) || name;
  if (!name && !regKey) return null;
  return {
    name,
    reg_key: regKey,
    snpid: asString(r.snpid),
    content_path: typeof r.content_path === "string" && r.content_path ? r.content_path : null,
    visibility: asNumberOrNull(r.visibility),
    sources: {
      has_service_center: asBool(sources.has_service_center),
      has_system_plist: asBool(sources.has_system_plist),
      has_user_plist: asBool(sources.has_user_plist),
      has_installed_product_json: asBool(sources.has_installed_product_json),
    },
    issues: (Array.isArray(r.issues) ? r.issues : [])
      .map(normalizeIssue)
      .filter((i): i is Issue => i !== null),
    user_list_index: asNumberOrNull(r.user_list_index),
    content_size_bytes: asNumberOrNull(r.content_size_bytes),
  };
}

function normalizeLibraries(raw: unknown): KontaktLibrary[] {
  if (!Array.isArray(raw)) return [];
  return raw.map(normalizeLibrary).filter((l): l is KontaktLibrary => l !== null);
}

/**
 * 底层错误是否与「文件访问权限」有关。
 *
 * macOS 上 Kontakt 音色库常在 ~/Downloads、~/Documents、外置卷里，
 * 首次读取会触发 TCC 授权；用户点「不允许」后错误信息五花八门，
 * 这里统一识别，好给出可操作的指引而不是一句「失败」。
 */
function isPermissionError(message: string): boolean {
  return /权限|permission|denied|not permitted|Operation not permitted|EPERM|EACCES|未能读取任何/i.test(
    message,
  );
}

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
  const [privPlan, setPrivPlan] = useState<PrivilegePlan | null>(null);
  const [privTarget, setPrivTarget] = useState<{ regKey: string; name: string } | null>(null);
  const [privBusy, setPrivBusy] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      // 后端返回什么形状都先规整：字段缺失按空数组/false 处理。
      setLibs(normalizeLibraries(await invoke<KontaktLibrary[]>("kontakt_inventory")));
    } catch (e) {
      setLibs([]);
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
      const found = normalizeLibraries(await invoke<KontaktLibrary[]>("kontakt_scan", { root: picked }));
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

  /**
   * 询问系统级（/Library）注册是否需要提权写入。
   *
   * 只做查询：`needed` 为真时把**每一条具体路径 + 原因**展示给用户，
   * 由用户点「继续」后才会触发管理员授权。
   */
  const askPrivilege = async (regKey: string, name: string) => {
    try {
      const plan = await invoke<PrivilegePlan>("kontakt_privilege_plan", { regKey, name });
      if (plan.needed && plan.entries.length > 0) {
        setPrivTarget({ regKey, name });
        setPrivPlan(plan);
      }
    } catch (e) {
      // 计划失败不阻断已经完成的免提权部分，但要如实告知。
      setError(String(e));
    }
  };

  const closePrivilege = () => {
    setPrivPlan(null);
    setPrivTarget(null);
  };

  /**
   * 用户对提权计划的决定。
   *
   * `allow === false` 表示用户跳过：直接收起对话框，不做任何提权动作
   * （免提权部分此前已经写好了）。
   *
   * 点「继续」后会**重新取一遍计划**再执行：只有清单与用户看到的完全一致才动手，
   * 否则把新清单展示出来让用户重新确认——保证「告知的操作对象」＝「真正执行的操作」。
   */
  const decidePrivilege = async (allow: boolean) => {
    const target = privTarget;
    if (!allow || !target) {
      closePrivilege();
      return;
    }
    setPrivBusy(true);
    setError(null);
    try {
      const fresh = await invoke<PrivilegePlan>("kontakt_privilege_plan", {
        regKey: target.regKey,
        name: target.name,
      });
      if (!samePlan(fresh, privPlan)) {
        if (fresh.needed && fresh.entries.length > 0) {
          setPrivPlan(fresh);
          setNotice(t("kontakt.priv.replanned"));
        } else {
          closePrivilege();
        }
        return;
      }

      const applied = await invoke<string[]>("kontakt_apply_privileged", {
        regKey: target.regKey,
        name: target.name,
        allow: true,
      });
      setNotice(t("kontakt.priv.applied", { n: applied.length }));
      closePrivilege();
      await load();
    } catch (e) {
      const message = String(e);
      // 用户取消管理员授权不是失败：友好提示，不显示红字。
      if (message.includes("已取消")) setNotice(t("kontakt.priv.cancelled"));
      else setError(message);
      closePrivilege();
    } finally {
      setPrivBusy(false);
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
      const lib = normalizeLibrary(await invoke<KontaktLibrary>("kontakt_add", { nicntPath: picked }));
      if (!lib) {
        setError(t("kontakt.error.badResponse"));
        return;
      }
      setNotice(t("kontakt.added", { name: lib.name }));
      await load();
      await askPrivilege(lib.reg_key, lib.name);
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
      await askPrivilege(lib.reg_key, lib.name);
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
    const list = libs.filter((l) => {
      if (!q) return true;
      // 逐字段兜底：任何一个是 null/undefined 都不该让筛选抛异常。
      return [l.name, l.snpid, l.reg_key].some((v) => asString(v).toLowerCase().includes(q));
    });
    const dir = sortAsc ? 1 : -1;
    return [...list].sort((a, b) => {
      if (sortKey === "size") {
        return ((a.content_size_bytes ?? 0) - (b.content_size_bytes ?? 0)) * dir;
      }
      if (sortKey === "snpid") {
        return asString(a.snpid).localeCompare(asString(b.snpid)) * dir;
      }
      return asString(a.name).localeCompare(asString(b.name)) * dir;
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
      {error && (
        <div className="flex flex-col items-start gap-2 rounded border border-danger/40 p-3">
          <ErrorText>{error}</ErrorText>
          {/* TCC 被拒 / 目录不可读时给出可操作的下一步，而不是只说“失败”。 */}
          {isPermissionError(error) && (
            <p className="text-sm opacity-80">{t("kontakt.permission.hint")}</p>
          )}
          <Button size="sm" variant="secondary" onPress={load} isDisabled={loading}>
            {t("kontakt.retry")}
          </Button>
        </div>
      )}
      {notice && <OkText>{notice}</OkText>}

      {!loading && filtered.length === 0 && !error && <Empty>{t("kontakt.empty")}</Empty>}

      {filtered.length > 0 && (
        <Section>
          {/* 注意：HeroUI 3 里 `Table`（= Table.Root）只是个 div 外壳，
              真正提供集合上下文的是 `Table.Content`。
              Header/Body/Column/Row/Cell 必须放在 Table.Content 之内，
              否则 react-aria 会抛 "cannot be rendered outside a collection."
              ——这正是「有数据才白屏、库为 0 时正常」的原因。 */}
          <Table.Root>
            <Table.ScrollContainer>
              <Table.Content aria-label={t("kontakt.title")}>
                <Table.Header>
                  <Table.Column key="name" id="name" isRowHeader>
                    <SortHeader
                      label={t("kontakt.cols.name")}
                      active={sortKey === "name"}
                      asc={sortAsc}
                      onPress={() => toggleSort("name")}
                    />
                  </Table.Column>
                  <Table.Column key="snpid" id="snpid">
                    <SortHeader
                      label={t("kontakt.cols.snpid")}
                      active={sortKey === "snpid"}
                      asc={sortAsc}
                      onPress={() => toggleSort("snpid")}
                    />
                  </Table.Column>
                  <Table.Column key="size" id="size">
                    <SortHeader
                      label={t("kontakt.cols.size")}
                      active={sortKey === "size"}
                      asc={sortAsc}
                      onPress={() => toggleSort("size")}
                    />
                  </Table.Column>
                  <Table.Column key="path" id="path">
                    {t("kontakt.cols.path")}
                  </Table.Column>
                  <Table.Column key="status" id="status">
                    {t("kontakt.cols.status")}
                  </Table.Column>
                  <Table.Column key="actions" id="actions">
                    {t("kontakt.cols.actions")}
                  </Table.Column>
                </Table.Header>
                <Table.Body>
                  {filtered.map((l, index) => (
                    <Table.Row key={`${l.reg_key}-${l.snpid}-${index}`} id={`${l.reg_key}-${l.snpid}-${index}`}>
                      <Table.Cell>{l.name || "—"}</Table.Cell>
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
                          {(l.issues ?? []).slice(0, 2).map((i) => (
                            <span key={i.code} title={i.detail || undefined}>
                              <Badge
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
                            </span>
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
              </Table.Content>
            </Table.ScrollContainer>
          </Table.Root>
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

      {/* 系统级注册（/Library）需要管理员授权：逐条列出具体文件与原因后才继续 */}
      <Modal.Root
        isOpen={!!privPlan}
        onOpenChange={(o) => {
          if (!o && !privBusy) closePrivilege();
        }}
      >
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>{t("kontakt.priv.title")}</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <p className="text-sm">
                  {t("kontakt.priv.intro", { name: privTarget?.name ?? "" })}
                </p>
                <ul className="mt-3 flex flex-col gap-3">
                  {(privPlan?.entries ?? []).map((entry) => (
                    <li key={`${entry.action}-${entry.path}`} className="flex flex-col gap-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge color={entry.action === "write" ? "accent" : "danger"}>
                          {entry.action === "write"
                            ? t("kontakt.priv.actionWrite")
                            : t("kontakt.priv.actionRemove")}
                        </Badge>
                        <code className="break-all text-xs">{entry.path}</code>
                      </div>
                      <p className="text-xs opacity-70">
                        {t(entry.reason_key, { defaultValue: entry.reason })}
                      </p>
                    </li>
                  ))}
                </ul>
                <p className="mt-4 text-xs opacity-80">{t("kontakt.priv.notice")}</p>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="ghost" onPress={() => decidePrivilege(false)} isDisabled={privBusy}>
                  {t("kontakt.priv.decline")}
                </Button>
                <PrimaryButton onPress={() => decidePrivilege(true)} isDisabled={privBusy}>
                  {t("kontakt.priv.continue")}
                </PrimaryButton>
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

/** 两份计划是否描述同一组操作（路径 + 动作）。 */
function samePlan(a: PrivilegePlan | null, b: PrivilegePlan | null): boolean {
  const fingerprint = (plan: PrivilegePlan | null) =>
    (plan?.entries ?? [])
      .map((entry) => `${entry.action}:${entry.path}`)
      .sort()
      .join("|");
  return fingerprint(a) === fingerprint(b);
}

/** 有任一来源记录即视为「已注册」；`sources` 缺失时按未注册处理。 */
function isRegistered(l: KontaktLibrary): boolean {
  const s = l.sources;
  return (
    asBool(s?.has_service_center) ||
    asBool(s?.has_system_plist) ||
    asBool(s?.has_user_plist) ||
    asBool(s?.has_installed_product_json)
  );
}

/** 无任何来源表明存在 .nicnt 时，提示可创建。 */
function hasNicnt(l: KontaktLibrary): boolean {
  const s = l.sources;
  return asBool(s?.has_service_center) || asBool(s?.has_installed_product_json);
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
