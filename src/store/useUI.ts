import { create } from "zustand";
import i18n from "../i18n";

export type ThemeMode = "light" | "dark" | "system";
export type AppId = "stem_separation" | "libraries" | "plugins";
export type ActiveView = "market" | "settings" | AppId;

function systemPrefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-color-scheme: dark)").matches
  );
}

function applyTheme(mode: ThemeMode) {
  const dark = mode === "dark" || (mode === "system" && systemPrefersDark());
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

interface UIState {
  openApps: AppId[];
  activeView: ActiveView;
  theme: ThemeMode;
  lang: string;
  /** 自动检查更新（默认开；关掉后仅保留设置页的手动检查）。 */
  autoCheckUpdate: boolean;
  /**
   * 已被「稍后」的版本号；`null` 表示未稍后任何版本。
   *
   * 持久化它（而非一个布尔）有两个好处：
   *   1. 同一版本不会在下次启动时又打扰用户 —— 对应「不再提示」；
   *   2. 出现**更高**版本时能自动重新提示 —— 对应「有新版本可用」。
   */
  updateDismissed: string | null;
  openApp: (id: AppId) => void;
  closeApp: (id: AppId) => void;
  setActive: (view: "market" | "settings" | AppId) => void;
  setTheme: (mode: ThemeMode) => void;
  setLang: (lang: string) => void;
  setAutoCheckUpdate: (on: boolean) => void;
  setUpdateDismissed: (version: string | null) => void;
}

const initialTheme =
  (localStorage.getItem("ui_theme") as ThemeMode | null) ?? "system";
const initialLang = localStorage.getItem("ui_lang") ?? i18n.language ?? "en";
const initialAutoCheck = localStorage.getItem("ui_auto_check_update") !== "0";
const initialDismissed = localStorage.getItem("ui_update_dismissed");

applyTheme(initialTheme);

export const useUI = create<UIState>((set) => ({
  openApps: [],
  activeView: "market",
  theme: initialTheme,
  lang: initialLang,
  autoCheckUpdate: initialAutoCheck,
  updateDismissed: initialDismissed,
  openApp: (id) =>
    set((s) => ({
      openApps: s.openApps.includes(id) ? s.openApps : [...s.openApps, id],
      activeView: id,
    })),
  closeApp: (id) =>
    set((s) => {
      const next = s.openApps.filter((a) => a !== id);
      const activeView =
        s.activeView === id ? (next[next.length - 1] ?? "market") : s.activeView;
      return { openApps: next, activeView };
    }),
  setActive: (view) => set({ activeView: view }),
  setTheme: (mode) => {
    localStorage.setItem("ui_theme", mode);
    applyTheme(mode);
    set({ theme: mode });
  },
  setLang: (lang) => {
    localStorage.setItem("ui_lang", lang);
    i18n.changeLanguage(lang);
    set({ lang });
  },
  setAutoCheckUpdate: (on) => {
    localStorage.setItem("ui_auto_check_update", on ? "1" : "0");
    set({ autoCheckUpdate: on });
  },
  setUpdateDismissed: (version) => {
    if (version) localStorage.setItem("ui_update_dismissed", version);
    else localStorage.removeItem("ui_update_dismissed");
    set({ updateDismissed: version });
  },
}));

// 跟随系统主题变化
if (typeof window !== "undefined") {
  window
    .matchMedia?.("(prefers-color-scheme: dark)")
    .addEventListener("change", () => {
      const cur = (useUI.getState().theme as ThemeMode) ?? "system";
      if (cur === "system") applyTheme("system");
    });
}
