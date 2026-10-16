// 共享 UI 原语与格式化工具
//
// 样式栈：Tailwind CSS v4 + HeroUI 3。页面统一从这里取组件与工具函数，
// 避免各页各自造轮子（历史上的内联样式重复问题）。

import type { ReactNode } from "react";
import { Button, Card, Chip, EmptyState, ProgressBar, Spinner } from "@heroui/react";

/** 页面区块容器：HeroUI Card 的统一封装。 */
export function Section({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <Card className={className}>
      {(title || actions) && (
        <Card.Header className="flex flex-row items-center justify-between gap-3">
          {typeof title === "string" ? <Card.Title>{title}</Card.Title> : title}
          {actions}
        </Card.Header>
      )}
      <Card.Content>{children}</Card.Content>
    </Card>
  );
}

/** 主操作按钮（品牌色）。 */
export function PrimaryButton({
  children,
  onPress,
  isDisabled,
  className,
}: {
  children: ReactNode;
  onPress?: () => void;
  isDisabled?: boolean;
  className?: string;
}) {
  return (
    <Button variant="primary" onPress={onPress} isDisabled={isDisabled} className={className}>
      {children}
    </Button>
  );
}

/** 次级按钮。 */
export function SecondaryButton({
  children,
  onPress,
  isDisabled,
  className,
}: {
  children: ReactNode;
  onPress?: () => void;
  isDisabled?: boolean;
  className?: string;
}) {
  return (
    <Button variant="secondary" onPress={onPress} isDisabled={isDisabled} className={className}>
      {children}
    </Button>
  );
}

/** 危险操作按钮（删除等）。 */
export function DangerButton({
  children,
  onPress,
  isDisabled,
}: {
  children: ReactNode;
  onPress?: () => void;
  isDisabled?: boolean;
}) {
  return (
    <Button variant="danger" onPress={onPress} isDisabled={isDisabled}>
      {children}
    </Button>
  );
}

/** 空状态占位。 */
export function Empty({ children }: { children: ReactNode }) {
  return (
    <EmptyState.Root className="py-10 text-center text-sm opacity-70">{children}</EmptyState.Root>
  );
}

/** 加载中。 */
export function Loading({ label }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 py-6 text-sm opacity-70">
      <Spinner.Root />
      {label && <span>{label}</span>}
    </div>
  );
}

/** 错误提示。 */
export function ErrorText({ children }: { children: ReactNode }) {
  return <p className="text-sm text-danger">{children}</p>;
}

/** 成功提示。 */
export function OkText({ children }: { children: ReactNode }) {
  return <p className="text-sm text-success">{children}</p>;
}

/** 确定进度条。 */
export function Progress({ value, label }: { value: number; label?: string }) {
  const pct = Math.max(0, Math.min(100, Math.round(value)));
  return (
    <div className="flex flex-col gap-1">
      {label && (
        <div className="flex justify-between text-xs opacity-70">
          <span>{label}</span>
          <span>{pct}%</span>
        </div>
      )}
      <ProgressBar.Root value={pct}>
        <ProgressBar.Track>
          <ProgressBar.Fill />
        </ProgressBar.Track>
      </ProgressBar.Root>
    </div>
  );
}

/** 不确定进度条（耗时且无法预估时使用）。 */
export function IndeterminateBar() {
  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-default">
      <div
        className="h-full w-2/5 rounded-full bg-accent"
        style={{ animation: "om-indet 1.1s ease-in-out infinite" }}
      />
    </div>
  );
}

/** 小标签。 */
export function Badge({ children, color }: { children: ReactNode; color?: "default" | "accent" | "success" | "warning" | "danger" }) {
  return (
    <Chip size="sm" color={color ?? "default"}>
      <Chip.Label>{children}</Chip.Label>
    </Chip>
  );
}

// ── 格式化工具 ────────────────────────────────────────────

/** 字节 → 人类可读（用于插件大小、库占用、下载进度）。 */
export function formatBytes(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  const v = n / 1024 ** i;
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

/**
 * 把主目录下的路径显示为 `~/...`。
 * 同时覆盖 macOS 实际卷路径 `/System/Volumes/Data/Users/x`。
 */
export function displayPath(p: string): string {
  return p
    .replace(/^\/System\/Volumes\/Data\/Users\/[^/]+/, "~")
    .replace(/^\/Users\/[^/]+/, "~");
}

/** ISO 时间 → 本地短格式；解析失败原样返回。 */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
}
