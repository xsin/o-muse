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
  openApp: (id: AppId) => void;
  closeApp: (id: AppId) => void;
  setActive: (view: "market" | "settings" | AppId) => void;
  setTheme: (mode: ThemeMode) => void;
  setLang: (lang: string) => void;
}

const initialTheme =
  (localStorage.getItem("ui_theme") as ThemeMode | null) ?? "system";
const initialLang = localStorage.getItem("ui_lang") ?? i18n.language ?? "en";

applyTheme(initialTheme);

export const useUI = create<UIState>((set) => ({
  openApps: [],
  activeView: "market",
  theme: initialTheme,
  lang: initialLang,
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
