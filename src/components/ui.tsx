// 共享 UI 原语（review 优化）
//
// 此前 panel/btnPrimary/btnSecondary/input/chip 在 PluginManagerPage 与
// StemSeparationPage 中**逐字重复**，且 Market/Library/Settings 各自内联一套
// 样式，导致视觉语言不统一。此处统一收敛，供各页面复用。

import type { CSSProperties, ReactNode } from "react";

export const panel: CSSProperties = {
  padding: 16,
  background: "var(--surface)",
  border: "1px solid var(--border)",
  borderRadius: "var(--radius)",
  boxShadow: "var(--shadow)",
};

export const btnPrimary: CSSProperties = {
  padding: "8px 16px",
  borderRadius: 8,
  border: "none",
  background: "var(--brand)",
  // 用 --brand-fg 令牌而非硬编码 #fff：深色主题下 brand 更亮，
  // 白字对比度不足 4.5:1（review 修复）。
  color: "var(--brand-fg)",
  fontWeight: 600,
  cursor: "pointer",
  fontSize: 13,
};

export const btnSecondary: CSSProperties = {
  padding: "8px 14px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  cursor: "pointer",
  fontSize: 13,
};

export const input: CSSProperties = {
  flex: 1,
  padding: "8px 12px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  fontSize: 13,
};

export const chip: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  padding: "3px 6px 3px 10px",
  borderRadius: 999,
  border: "1px solid var(--border)",
  background: "var(--surface-2)",
  color: "var(--text-muted)",
  fontSize: 11,
};

export const cell: CSSProperties = {
  padding: "10px 12px",
  borderBottom: "1px solid var(--border)",
  color: "var(--text)",
  verticalAlign: "top",
};

/** 面板容器 */
export function Panel({
  children,
  style,
}: {
  children: ReactNode;
  style?: CSSProperties;
}) {
  return <div style={{ ...panel, ...style }}>{children}</div>;
}

/** 可键盘操作的按钮（全站统一，修复 a11y：此前点击区均为纯 div）。 */
export function Button({
  children,
  onClick,
  disabled,
  variant = "secondary",
  style,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: "primary" | "secondary";
  style?: CSSProperties;
  title?: string;
}) {
  const base = variant === "primary" ? btnPrimary : btnSecondary;
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      style={{
        ...base,
        opacity: disabled ? 0.5 : 1,
        ...style,
      }}
    >
      {children}
    </button>
  );
}

/** 不确定进度条（耗时操作：扫描 / 分轨）。 */
export function IndeterminateBar() {
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

/** 确定进度条（0–100）。 */
export function ProgressBar({ value }: { value: number }) {
  const pct = Math.max(0, Math.min(100, Math.round(value)));
  return (
    <div
      style={{
        height: 8,
        background: "var(--surface-2)",
        borderRadius: 999,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: "100%",
          width: `${pct}%`,
          background: "var(--brand)",
          borderRadius: 999,
          transition: "width .3s ease",
        }}
      />
    </div>
  );
}

/** 空状态占位 */
export function EmptyState({ children }: { children: ReactNode }) {
  return <Panel style={{ color: "var(--text-muted)", fontSize: 13 }}>{children}</Panel>;
}

/** 错误提示 */
export function ErrorText({ children }: { children: ReactNode }) {
  return (
    <p style={{ color: "var(--danger)", fontSize: 13, margin: 0 }}>{children}</p>
  );
}

/** 成功提示 */
export function OkText({ children }: { children: ReactNode }) {
  return <p style={{ color: "var(--ok)", fontSize: 13, margin: 0 }}>{children}</p>;
}

/**
 * 把主目录下的路径显示为 `~/...`。
 * 覆盖 `/Users/x` 与 macOS 实际卷路径 `/System/Volumes/Data/Users/x`。
 */
export function displayPath(p: string): string {
  return p
    .replace(/^\/System\/Volumes\/Data\/Users\/[^/]+/, "~")
    .replace(/^\/Users\/[^/]+/, "~");
}
