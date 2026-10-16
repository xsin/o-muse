// Audio Plugin Manager（v1.2 空间管理）
//
// 目标：解决"系统盘空间焦虑"——让用户一眼看清装了哪些插件、各占多大、
// 按大小/时间排序，并能**安全地把插件搬到别的卷**（原位置留软链）。
//
// 关键约定：
//  1. 默认只扫 VST3（后端 `scan_all_plugins` 同为 VST3 默认），
//     其它格式由顶部页签显式触发。
//  2. 所有破坏性操作走 HeroUI Modal 二次确认；删除 = 移到废纸篓
//     （永久删除需要手动输入插件名）。
//  3. 排序按数值/时间比较（大小、安装时间），不是字符串比较。

import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Button,
  Chip,
  Modal,
  SearchField,
  Table,
  Tabs,
  toast,
  useOverlayState,
} from "@heroui/react";
import {
  DangerButton,
  Empty,
  ErrorText,
  Loading,
  PrimaryButton,
  SecondaryButton,
  Section,
  displayPath,
  formatBytes,
  formatDate,
} from "../components/ui";

// ── 与 Rust 端 camelCase 序列化对齐 ─────────────────────────

type PluginFormat = "AU" | "VST" | "VST3" | "AAX" | "CLAP" | "Auto";

/** `PluginInfo`（Rust `#[serde(rename_all = "camelCase")]`）。 */
interface PluginInfo {
  name: string;
  vendor: string | null;
  version: string | null;
  format: PluginFormat;
  path: string;
  bundleId: string | null;
  lastModified: string;
  /** 缓存指纹大小（bundle 为 Info.plist 大小），不用于展示 */
  fileSize: number;
  /** 真实占用（递归求和） */
  sizeBytes: number;
  installedAt: string | null;
  isSymlink: boolean;
  realPath: string | null;
  isBrokenLink: boolean;
}

/** `ScanSource` 仍是 snake_case（后端未改）。 */
interface ScanSource {
  path: string;
  plugin_format: PluginFormat;
  recursive: boolean;
  enabled: boolean;
  is_custom: boolean;
}

interface ScanResult {
  plugins: PluginInfo[];
  hits: number;
  misses: number;
  cache_warning: string | null;
  format: string;
}

/** `MovePlanView`（MovePlan 展平 + 目标卷余量）。 */
interface MovePlanView {
  source: string;
  targetDir: string;
  resolvedTarget: string;
  sizeBytes: number;
  targetFreeBytes: number | null;
  neededBytes: number;
}

type SortKey = "name" | "vendor" | "sizeBytes" | "installedAt";
type SortDir = "ascending" | "descending";

const FORMAT_TABS = ["VST3", "ALL", "AU", "VST", "AAX", "CLAP"] as const;

const UNKNOWN = "__unknown__";

function vendorKey(p: PluginInfo): string {
  const v = (p.vendor ?? "").trim();
  return v.length > 0 ? v : UNKNOWN;
}

/** ISO 时间 → 毫秒（无法解析按 0，保证"没有时间"排在最早的语义稳定）。 */
function timeOf(iso: string | null): number {
  if (!iso) return 0;
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? 0 : ms;
}

export default function PluginManagerPage() {
  const { t } = useTranslation();

  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [sources, setSources] = useState<ScanSource[]>([]);
  const [format, setFormat] = useState<string>("VST3");
  const [query, setQuery] = useState("");
  const [pickedVendors, setPickedVendors] = useState<string[]>([]);
  const [sortKey, setSortKey] = useState<SortKey>("sizeBytes");
  const [sortDir, setSortDir] = useState<SortDir>("descending");

  const [busy, setBusy] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [cacheNote, setCacheNote] = useState<string | null>(null);

  // 移动 / 删除 / 还原 三个确认流程
  const moveState = useOverlayState();
  const trashState = useOverlayState();
  const undoState = useOverlayState();

  const [moveTarget, setMoveTarget] = useState<PluginInfo | null>(null);
  const [moveDir, setMoveDir] = useState<string | null>(null);
  const [plan, setPlan] = useState<MovePlanView | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [moving, setMoving] = useState(false);

  const [trashTarget, setTrashTarget] = useState<PluginInfo | null>(null);
  const [typedName, setTypedName] = useState("");
  const [deleting, setDeleting] = useState(false);

  const [undoTarget, setUndoTarget] = useState<PluginInfo | null>(null);
  const [undoing, setUndoing] = useState(false);

  // ── 扫描 ────────────────────────────────────────────────

  const refreshSources = useCallback(async () => {
    try {
      setSources(await invoke<ScanSource[]>("list_scan_sources"));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const scan = useCallback(async (fmt: string, clearCache = false) => {
    setBusy(true);
    setError(null);
    try {
      const res = clearCache
        ? await invoke<ScanResult>("rescan_plugins", { format: fmt })
        : await invoke<ScanResult>("scan_plugins_by_format", { format: fmt });
      setPlugins(res.plugins ?? []);
      setWarning(res.cache_warning);
      setPickedVendors([]);
      setCacheNote(
        res.hits + res.misses > 0
          ? t("plugins.cachedCount", { hits: res.hits, total: res.hits + res.misses })
          : null,
      );
      setScanned(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [t]);

  useEffect(() => {
    void refreshSources();
  }, [refreshSources]);

  // 默认 VST3；切换页签即重新扫描该格式（后端按格式只遍历相关目录）
  useEffect(() => {
    void scan(format, false);
  }, [format, scan]);

  // ── 派生数据 ────────────────────────────────────────────

  const vendorLabel = useCallback(
    (p: PluginInfo) => {
      const key = vendorKey(p);
      return key === UNKNOWN ? t("plugins.unknownVendor") : key;
    },
    [t],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return plugins.filter((p) => {
      if (pickedVendors.length > 0 && !pickedVendors.includes(vendorKey(p))) {
        return false;
      }
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        (p.vendor ?? "").toLowerCase().includes(q)
      );
    });
  }, [plugins, pickedVendors, query]);

  /** 数值/时间排序——不是字符串排序。 */
  const rows = useMemo(() => {
    const dir = sortDir === "ascending" ? 1 : -1;
    const cmp = (a: PluginInfo, b: PluginInfo): number => {
      switch (sortKey) {
        case "name":
          return a.name.localeCompare(b.name);
        case "vendor":
          return vendorLabel(a).localeCompare(vendorLabel(b));
        case "sizeBytes":
          return (a.sizeBytes ?? 0) - (b.sizeBytes ?? 0);
        case "installedAt":
          return timeOf(a.installedAt) - timeOf(b.installedAt);
        default:
          return 0;
      }
    };
    const arr = [...filtered];
    arr.sort((a, b) => cmp(a, b) * dir || a.path.localeCompare(b.path));
    return arr;
  }, [filtered, sortKey, sortDir, vendorLabel]);

  const overview = useMemo(() => {
    let bytes = 0;
    let symlinks = 0;
    let broken = 0;
    for (const p of plugins) {
      bytes += p.sizeBytes ?? 0;
      if (p.isSymlink) symlinks += 1;
      if (p.isBrokenLink) broken += 1;
    }
    return { count: plugins.length, bytes, symlinks, broken };
  }, [plugins]);

  const byFormat = useMemo(() => {
    const m = new Map<string, { count: number; bytes: number }>();
    for (const p of plugins) {
      const cur = m.get(p.format) ?? { count: 0, bytes: 0 };
      cur.count += 1;
      cur.bytes += p.sizeBytes ?? 0;
      m.set(p.format, cur);
    }
    return [...m.entries()].sort((a, b) => b[1].bytes - a[1].bytes);
  }, [plugins]);

  const byVendor = useMemo(() => {
    const m = new Map<string, { count: number; bytes: number }>();
    for (const p of plugins) {
      const key = vendorKey(p);
      const cur = m.get(key) ?? { count: 0, bytes: 0 };
      cur.count += 1;
      cur.bytes += p.sizeBytes ?? 0;
      m.set(key, cur);
    }
    return [...m.entries()].sort((a, b) => b[1].bytes - a[1].bytes);
  }, [plugins]);

  // ── 扫描源（自定义目录为 Pro 能力，保留原有入口）─────────

  const addDir = async () => {
    setError(null);
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: t("plugins.pickDir"),
      });
      if (!picked || Array.isArray(picked)) return;
      await invoke<ScanSource>("add_custom_plugin_dir", { path: picked });
      toast.success(t("plugins.added"));
      await refreshSources();
    } catch {
      setError(t("plugins.proRequired"));
    }
  };

  const removeDir = async (path: string) => {
    setError(null);
    try {
      await invoke("remove_custom_plugin_dir", { path });
      await refreshSources();
    } catch {
      setError(t("plugins.proRequired"));
    }
  };

  // ── 移动 ────────────────────────────────────────────────

  const openMove = (p: PluginInfo) => {
    setMoveTarget(p);
    setMoveDir(null);
    setPlan(null);
    setError(null);
    moveState.open();
  };

  const pickTarget = async () => {
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: t("plugins.move.pickTarget"),
    });
    if (!picked || Array.isArray(picked) || !moveTarget) return;
    setMoveDir(picked);
    setPlan(null);
    setPlanBusy(true);
    try {
      const res = await invoke<MovePlanView>("plan_move_plugin", {
        source: moveTarget.path,
        targetDir: picked,
      });
      setPlan(res);
    } catch (e) {
      setError(String(e));
    } finally {
      setPlanBusy(false);
    }
  };

  const confirmMove = async () => {
    if (!moveTarget || !moveDir) return;
    setMoving(true);
    setError(null);
    try {
      const link = await invoke<string>("move_plugin", {
        source: moveTarget.path,
        targetDir: moveDir,
      });
      toast.success(t("plugins.move.done", { path: displayPath(link) }));
      moveState.close();
      await scan(format);
    } catch (e) {
      setError(String(e));
    } finally {
      setMoving(false);
    }
  };

  // ── 删除（废纸篓优先）───────────────────────────────────

  const openTrash = (p: PluginInfo) => {
    setTrashTarget(p);
    setTypedName("");
    setError(null);
    trashState.open();
  };

  // 与后端 `delete_plugin_permanently` 的校验完全一致：
  // 接受完整文件名（`Serum.vst3`）或去扩展名的目录名（`Serum`）。
  const requiredName = trashTarget?.path.split("/").pop() ?? "";
  const requiredStem = requiredName.replace(/\.[^.]+$/, "");
  const typed = typedName.trim();
  const nameMatches =
    typed.length > 0 && (typed === requiredName || typed === requiredStem);

  const confirmTrash = async () => {
    if (!trashTarget || !nameMatches) return;
    setDeleting(true);
    setError(null);
    try {
      const dest = await invoke<string>("trash_plugin", { path: trashTarget.path });
      toast.success(t("plugins.delete.done", { path: displayPath(dest) }));
      trashState.close();
      await scan(format);
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
    }
  };

  const confirmPermanent = async () => {
    if (!trashTarget || !nameMatches) return;
    setDeleting(true);
    setError(null);
    try {
      await invoke("delete_plugin_permanently", {
        path: trashTarget.path,
        confirmName: typedName.trim(),
      });
      toast.danger(t("plugins.delete.permanentDone"));
      trashState.close();
      await scan(format);
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
    }
  };

  // ── 还原（软链 → 真实文件）──────────────────────────────

  const openUndo = (p: PluginInfo) => {
    setUndoTarget(p);
    setError(null);
    undoState.open();
  };

  const confirmUndo = async () => {
    if (!undoTarget) return;
    setUndoing(true);
    setError(null);
    try {
      await invoke("undo_move_plugin", { linkPath: undoTarget.path });
      toast.success(t("plugins.undo.done"));
      undoState.close();
      await scan(format);
    } catch (e) {
      setError(String(e));
    } finally {
      setUndoing(false);
    }
  };

  // ── 渲染 ────────────────────────────────────────────────

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h1 className="text-xl font-semibold">{t("apps.plugins.name")}</h1>
        <p className="mt-1 text-sm opacity-70">{t("apps.plugins.desc")}</p>
      </div>

      {/* 空间概览 */}
      <Section
        title={t("plugins.space.title")}
        actions={
          <span className="text-sm opacity-80">
            {t("plugins.space.summary", {
              n: overview.count,
              size: formatBytes(overview.bytes),
            })}
          </span>
        }
      >
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs opacity-60">{t("plugins.byFormat")}</span>
            {byFormat.length === 0 ? (
              <span className="text-xs opacity-60">—</span>
            ) : (
              byFormat.map(([fmt, s]) => (
                <Chip key={fmt} size="sm" color="default">
                  <Chip.Label>
                    {fmt} · {s.count} · {formatBytes(s.bytes)}
                  </Chip.Label>
                </Chip>
              ))
            )}
          </div>

          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs opacity-60">{t("plugins.byVendor")}</span>
              {pickedVendors.length > 0 && (
                <Button
                  size="sm"
                  variant="ghost"
                  onPress={() => setPickedVendors([])}
                >
                  {t("plugins.filter.clear")}
                </Button>
              )}
            </div>
            <div className="flex flex-wrap gap-2">
              {byVendor.length === 0 ? (
                <span className="text-xs opacity-60">—</span>
              ) : (
                byVendor.map(([key, s]) => {
                  const picked = pickedVendors.includes(key);
                  const label =
                    key === UNKNOWN ? t("plugins.unknownVendor") : key;
                  return (
                    <FilterChip
                      key={key}
                      color={picked ? "accent" : "default"}
                      onToggle={() =>
                        setPickedVendors((prev) =>
                          prev.includes(key)
                            ? prev.filter((x) => x !== key)
                            : [...prev, key],
                        )
                      }
                      title={t("plugins.filter.vendorHint", {
                        name: label,
                        n: s.count,
                        size: formatBytes(s.bytes),
                      })}
                    >
                      {label} · {s.count} · {formatBytes(s.bytes)}
                    </FilterChip>
                  );
                })
              )}
            </div>
          </div>

          {(overview.symlinks > 0 || overview.broken > 0) && (
            <div className="flex flex-wrap items-center gap-2 text-xs">
              {overview.symlinks > 0 && (
                <Chip size="sm" color="accent">
                  <Chip.Label>
                    {t("plugins.tag.symlinkCount", { n: overview.symlinks })}
                  </Chip.Label>
                </Chip>
              )}
              {overview.broken > 0 && (
                <Chip size="sm" color="danger">
                  <Chip.Label>
                    {t("plugins.tag.brokenCount", { n: overview.broken })}
                  </Chip.Label>
                </Chip>
              )}
            </div>
          )}
        </div>
      </Section>

      {/* 扫描源（自定义目录 = Pro） */}
      <Section
        title={t("plugins.sources")}
        actions={
          <SecondaryButton onPress={addDir} isDisabled={busy}>
            {t("plugins.addDirPro")}
          </SecondaryButton>
        }
      >
        {sources.length === 0 ? (
          <span className="text-xs opacity-60">—</span>
        ) : (
          <div className="flex flex-wrap gap-2">
            {sources.map((s) => (
              <span
                key={s.path}
                className="flex items-center gap-2 rounded-full border border-border px-3 py-1 text-xs"
              >
                <span className="opacity-70">{s.plugin_format}</span>
                <span title={s.path} className="max-w-64 truncate">
                  {displayPath(s.path)}
                </span>
                {s.is_custom && (
                  <Button
                    size="sm"
                    variant="ghost"
                    isIconOnly
                    aria-label={t("plugins.remove")}
                    onPress={() => removeDir(s.path)}
                  >
                    ✕
                  </Button>
                )}
              </span>
            ))}
          </div>
        )}
      </Section>

      {/* 格式切换 + 操作条 */}
      <div className="flex flex-col gap-3">
        <Tabs.Root
          selectedKey={format}
          onSelectionChange={(key) => setFormat(String(key))}
        >
          <Tabs.List aria-label={t("plugins.cols.format")}>
            {FORMAT_TABS.map((f) => (
              <Tabs.Tab key={f} id={f}>
                {f === "ALL" ? t("plugins.formats.all") : f}
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs.Root>

        <div className="flex flex-wrap items-center gap-3">
          <PrimaryButton onPress={() => void scan(format)} isDisabled={busy}>
            {busy ? t("plugins.scanning") : t("plugins.scan")}
          </PrimaryButton>
          <SecondaryButton
            onPress={() => void scan(format, true)}
            isDisabled={busy}
          >
            {t("plugins.rescan")}
          </SecondaryButton>
          <SearchField.Root
            value={query}
            onChange={setQuery}
            aria-label={t("plugins.search")}
            className="w-72"
          >
            <SearchField.Group>
              <SearchField.SearchIcon />
              <SearchField.Input placeholder={t("plugins.search")} />
              <SearchField.ClearButton />
            </SearchField.Group>
          </SearchField.Root>
          <span className="text-xs opacity-70">
            {t("plugins.count", { n: rows.length })}
            {plugins.length !== rows.length
              ? ` / ${t("plugins.totalCount", { n: plugins.length })}`
              : ""}
          </span>
          {cacheNote && (
            <span className="text-xs opacity-60">{cacheNote}</span>
          )}
        </div>
      </div>

      {busy && <Loading label={t("plugins.scanning")} />}
      {error && <ErrorText>{t("plugins.failed", { err: error })}</ErrorText>}
      {warning && (
        <Alert.Root status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>{t("plugins.cacheWarning")}</Alert.Title>
            <Alert.Description>{warning}</Alert.Description>
          </Alert.Content>
        </Alert.Root>
      )}

      {/* 主表格 */}
      {scanned && plugins.length === 0 ? (
        <Empty>{t("plugins.empty")}</Empty>
      ) : rows.length === 0 && !busy ? (
        <Empty>{t("plugins.empty")}</Empty>
      ) : (
        <Table.Root>
          <Table.ScrollContainer>
            <Table.Content
              aria-label={t("apps.plugins.name")}
              sortDescriptor={{ column: sortKey, direction: sortDir }}
              onSortChange={(d) => {
                setSortKey(String(d.column) as SortKey);
                setSortDir(
                  d.direction === "ascending" ? "ascending" : "descending",
                );
              }}
            >
              <Table.Header>
                <Table.Column
                  key="name"
                  id="name"
                  allowsSorting
                  isRowHeader
                >
                  {({ sortDirection }) => (
                    <Table.SortableColumnHeader sortDirection={sortDirection}>
                      {t("plugins.cols.name")}
                    </Table.SortableColumnHeader>
                  )}
                </Table.Column>
                <Table.Column key="vendor" id="vendor" allowsSorting>
                  {({ sortDirection }) => (
                    <Table.SortableColumnHeader sortDirection={sortDirection}>
                      {t("plugins.cols.vendor")}
                    </Table.SortableColumnHeader>
                  )}
                </Table.Column>
                <Table.Column key="version" id="version">
                  {t("plugins.cols.version")}
                </Table.Column>
                <Table.Column key="format" id="format">
                  {t("plugins.cols.format")}
                </Table.Column>
                <Table.Column key="sizeBytes" id="sizeBytes" allowsSorting>
                  {({ sortDirection }) => (
                    <Table.SortableColumnHeader sortDirection={sortDirection}>
                      {t("plugins.cols.size")}
                    </Table.SortableColumnHeader>
                  )}
                </Table.Column>
                <Table.Column key="installedAt" id="installedAt" allowsSorting>
                  {({ sortDirection }) => (
                    <Table.SortableColumnHeader sortDirection={sortDirection}>
                      {t("plugins.cols.installedAt")}
                    </Table.SortableColumnHeader>
                  )}
                </Table.Column>
                <Table.Column key="path" id="path">
                  {t("plugins.cols.path")}
                </Table.Column>
                <Table.Column key="actions" id="actions">
                  {t("plugins.cols.actions")}
                </Table.Column>
              </Table.Header>
              <Table.Body>
                {rows.map((p) => (
                  <Table.Row key={p.path} id={p.path}>
                    <Table.Cell>
                      <span className="font-medium">{p.name}</span>
                    </Table.Cell>
                    <Table.Cell>{vendorLabel(p)}</Table.Cell>
                    <Table.Cell>{p.version ?? "—"}</Table.Cell>
                    <Table.Cell>
                      <Chip size="sm" color="default">
                        <Chip.Label>{p.format}</Chip.Label>
                      </Chip>
                    </Table.Cell>
                    <Table.Cell className="tabular-nums">
                      {formatBytes(p.sizeBytes)}
                    </Table.Cell>
                    <Table.Cell>{formatDate(p.installedAt)}</Table.Cell>
                    <Table.Cell>
                      <div className="flex flex-col gap-1">
                        <span
                          className="max-w-72 truncate text-xs opacity-70"
                          title={p.path}
                        >
                          {displayPath(p.path)}
                        </span>
                        {p.isSymlink && (
                          <span className="flex items-center gap-1">
                            <Chip
                              size="sm"
                              color={p.isBrokenLink ? "danger" : "accent"}
                            >
                              <Chip.Label>
                                {p.isBrokenLink
                                  ? t("plugins.tag.broken")
                                  : t("plugins.tag.symlink")}
                              </Chip.Label>
                            </Chip>
                            <span
                              className="max-w-72 truncate text-xs opacity-60"
                              title={p.realPath ?? undefined}
                            >
                              →{" "}
                              {p.realPath
                                ? displayPath(p.realPath)
                                : t("plugins.tag.missingTarget")}
                            </span>
                          </span>
                        )}
                      </div>
                    </Table.Cell>
                    <Table.Cell>
                      <div className="flex items-center gap-2">
                        {p.isSymlink ? (
                          <Button
                            size="sm"
                            variant="secondary"
                            isDisabled={p.isBrokenLink || undoState.isOpen}
                            onPress={() => openUndo(p)}
                          >
                            {t("plugins.actions.undo")}
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="primary"
                            onPress={() => openMove(p)}
                          >
                            {t("plugins.actions.move")}
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="danger"
                          onPress={() => openTrash(p)}
                        >
                          {t("plugins.actions.trash")}
                        </Button>
                      </div>
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table.Root>
      )}

      {/* ── 移动确认 ─────────────────────────────────────── */}
      <Modal.Root isOpen={moveState.isOpen} onOpenChange={moveState.setOpen}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>
                  {t("plugins.move.title", { name: moveTarget?.name ?? "" })}
                </Modal.Heading>
                <Modal.CloseTrigger />
              </Modal.Header>
              <Modal.Body className="flex flex-col gap-3">
                <p className="text-sm opacity-80">{t("plugins.move.symlinkNote")}</p>
                <div className="flex items-center gap-3">
                  <SecondaryButton onPress={pickTarget} isDisabled={planBusy}>
                    {t("plugins.move.pickTarget")}
                  </SecondaryButton>
                  <span className="truncate text-sm">
                    {moveDir
                      ? displayPath(moveDir)
                      : t("plugins.move.noTarget")}
                  </span>
                </div>
                {planBusy && <Loading label={t("plugins.move.planning")} />}
                {plan && (
                  <ul className="flex flex-col gap-1 text-sm">
                    <li>
                      {t("plugins.move.size", {
                        size: formatBytes(plan.sizeBytes),
                      })}
                    </li>
                    <li>
                      {t("plugins.move.free", {
                        size:
                          plan.targetFreeBytes == null
                            ? "—"
                            : formatBytes(plan.targetFreeBytes),
                      })}
                    </li>
                    {plan.targetFreeBytes != null &&
                      plan.targetFreeBytes < plan.neededBytes && (
                        <li className="text-danger">
                          {t("plugins.move.insufficient")}
                        </li>
                      )}
                  </ul>
                )}
                {error && <ErrorText>{error}</ErrorText>}
              </Modal.Body>
              <Modal.Footer className="flex justify-end gap-2">
                <Button variant="secondary" onPress={moveState.close}>
                  {t("plugins.move.cancel")}
                </Button>
                <PrimaryButton
                  onPress={confirmMove}
                  isDisabled={!plan || moving}
                >
                  {moving ? t("plugins.move.working") : t("plugins.move.confirm")}
                </PrimaryButton>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>

      {/* ── 删除确认（必须手动输入插件名）────────────────── */}
      <Modal.Root isOpen={trashState.isOpen} onOpenChange={trashState.setOpen}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>
                  {t("plugins.delete.title", { name: trashTarget?.name ?? "" })}
                </Modal.Heading>
                <Modal.CloseTrigger />
              </Modal.Header>
              <Modal.Body className="flex flex-col gap-3">
                <Alert.Root status="warning">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>{t("plugins.delete.warning")}</Alert.Title>
                    <Alert.Description>
                      {t("plugins.delete.body")}
                    </Alert.Description>
                  </Alert.Content>
                </Alert.Root>
                {trashTarget?.isSymlink && (
                  <p className="text-xs opacity-70">
                    {t("plugins.delete.linkNote", {
                      path: displayPath(trashTarget.realPath ?? ""),
                    })}
                  </p>
                )}
                <label className="flex flex-col gap-2 text-sm">
                  <span>
                    {t("plugins.delete.typeHint", { name: requiredName })}
                  </span>
                  <input
                    value={typedName}
                    onChange={(e) => setTypedName(e.target.value)}
                    placeholder={requiredName}
                    aria-label={t("plugins.delete.typeHint", { name: requiredName })}
                    className="rounded-md border border-border bg-surface px-3 py-2 text-sm"
                  />
                </label>
                {typedName.length > 0 && !nameMatches && (
                  <ErrorText>{t("plugins.delete.mismatch")}</ErrorText>
                )}
                {error && <ErrorText>{error}</ErrorText>}
              </Modal.Body>
              <Modal.Footer className="flex justify-end gap-2">
                <Button variant="secondary" onPress={trashState.close}>
                  {t("plugins.delete.cancel")}
                </Button>
                <Button
                  variant="ghost"
                  className="text-danger"
                  isDisabled={!nameMatches || deleting}
                  onPress={confirmPermanent}
                >
                  {t("plugins.delete.permanent")}
                </Button>
                <DangerButton
                  onPress={confirmTrash}
                  isDisabled={!nameMatches || deleting}
                >
                  {deleting ? t("plugins.delete.working") : t("plugins.delete.trash")}
                </DangerButton>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>

      {/* ── 还原确认 ─────────────────────────────────────── */}
      <Modal.Root isOpen={undoState.isOpen} onOpenChange={undoState.setOpen}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>
                  {t("plugins.undo.title", { name: undoTarget?.name ?? "" })}
                </Modal.Heading>
                <Modal.CloseTrigger />
              </Modal.Header>
              <Modal.Body>
                <p className="text-sm opacity-80">
                  {t("plugins.undo.body", {
                    path: displayPath(undoTarget?.realPath ?? ""),
                  })}
                </p>
                {error && <ErrorText>{error}</ErrorText>}
              </Modal.Body>
              <Modal.Footer className="flex justify-end gap-2">
                <Button variant="secondary" onPress={undoState.close}>
                  {t("plugins.undo.cancel")}
                </Button>
                <PrimaryButton onPress={confirmUndo} isDisabled={undoing}>
                  {undoing ? t("plugins.undo.working") : t("plugins.undo.confirm")}
                </PrimaryButton>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>
    </div>
  );
}

/** 可点选的 Vendor 标签（多选筛选）。 */
function FilterChip({
  children,
  color,
  title,
  onToggle,
}: {
  children: ReactNode;
  color: "default" | "accent";
  title: string;
  onToggle: () => void;
}) {
  return (
    <Chip
      size="sm"
      color={color}
      role="button"
      tabIndex={0}
      title={title}
      className="cursor-pointer select-none"
      onClick={onToggle}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onToggle();
        }
      }}
    >
      <Chip.Label>{children}</Chip.Label>
    </Chip>
  );
}
