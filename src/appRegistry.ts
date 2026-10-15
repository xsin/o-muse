import type { AppId } from "./store/useUI";

// 轻量 App Registry（Q9）。
// 仅存 id / route / icon / enabled / minVersion；字符串全部走 i18n。
// 注意：按强制命名规范，分轨 App 的 id 与 route 固定为 "stem_separation"，
// 代码中不得使用被替换掉的旧项目名。
export interface AppDef {
  id: AppId;
  route: string;
  icon: string; // 简单内联 svg path 或 emoji 占位
  enabled: boolean;
  minVersion: string;
  badge?: "soon";
}

export const appRegistry: AppDef[] = [
  {
    id: "stem_separation",
    route: "/apps/stem_separation",
    icon: "🎚️",
    enabled: true,
    minVersion: "1.0",
  },
  {
    id: "libraries",
    route: "/apps/libraries",
    icon: "📚",
    enabled: true,
    minVersion: "1.0",
  },
  {
    id: "plugins",
    route: "/apps/plugins",
    icon: "🎛️",
    enabled: false,
    minVersion: "1.1",
    badge: "soon",
  },
];

export function getApp(id: AppId): AppDef | undefined {
  return appRegistry.find((a) => a.id === id);
}
