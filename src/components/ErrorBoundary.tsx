// 全局错误边界
//
// 目的：任何一处渲染异常只影响其所在区域，而不是把整个窗口变成白屏。
//
// 用法：
//   <ErrorBoundary>            —— 包住整棵应用（main.tsx，最后一道防线）
//   <ErrorBoundary label={...}>—— 包住单个页面/区块（App.tsx，故障隔离到该页）
//
// 崩溃时显示可读的错误信息与堆栈，并提供「重试」与「重新载入」两个出口，
// 让用户至少能看清发生了什么、继续用其他页面。

import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props {
  children: ReactNode;
  /** 崩溃区域的可读名称（如页面名），用于在提示里指明「哪一块」坏了。 */
  label?: string;
  /** 自定义回退界面；缺省时使用内置的可读错误面板。 */
  fallback?: (error: Error, retry: () => void) => ReactNode;
}

interface State {
  error: Error | null;
}

/** 真正的边界实现：捕获子树的渲染异常。 */
class Boundary extends Component<Props & { renderFallback: (e: Error, retry: () => void) => ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // 保留到控制台：开发期与崩溃报告都靠它定位。
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  private retry = () => this.setState({ error: null });

  render() {
    if (this.state.error) {
      return this.props.renderFallback(this.state.error, this.retry);
    }
    return this.props.children;
  }
}

/** 默认回退界面：标题 + 说明 + 堆栈 + 两个出口操作。 */
function CrashView({
  error,
  label,
  retry,
}: {
  error: Error;
  label?: string;
  retry: () => void;
}) {
  const { t } = useTranslation();
  const stack = error?.stack || error?.message || String(error);
  return (
    <div className="flex flex-col gap-3 p-6 text-sm" role="alert">
      <h2 className="text-lg font-semibold">{t("common.crash.title")}</h2>
      <p className="opacity-80">
        {label ? t("common.crash.where", { label }) : t("common.crash.hint")}
      </p>
      <pre className="max-h-64 w-full overflow-auto rounded bg-default/40 p-3 text-xs whitespace-pre-wrap">
        {stack}
      </pre>
      <div className="flex gap-2">
        <button
          type="button"
          onClick={retry}
          className="rounded bg-accent px-3 py-1 text-sm text-accent-foreground"
        >
          {t("common.crash.retry")}
        </button>
        <button
          type="button"
          onClick={() => window.location.reload()}
          className="rounded border border-default px-3 py-1 text-sm"
        >
          {t("common.crash.reload")}
        </button>
      </div>
    </div>
  );
}

/**
 * 错误边界。
 *
 * 类组件负责捕获，函数组件在此提供 i18n（语言切换后回退界面也会跟着变）。
 */
export default function ErrorBoundary({ children, label, fallback }: Props) {
  return (
    <Boundary
      label={label}
      renderFallback={(error, retry) =>
        fallback ? fallback(error, retry) : <CrashView error={error} label={label} retry={retry} />
      }
    >
      {children}
    </Boundary>
  );
}
