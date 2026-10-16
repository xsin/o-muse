import { useEffect } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { Toast } from "@heroui/react";
import { useUI, type ActiveView } from "./store/useUI";
import Sidebar from "./components/Sidebar";
import UpdateBanner from "./components/UpdateBanner";
import MarketPage from "./pages/MarketPage";
import SettingsPage from "./pages/SettingsPage";
import StemSeparationPage from "./pages/StemSeparationPage";
import KontaktLibraryPage from "./pages/KontaktLibraryPage";
import PluginManagerPage from "./pages/PluginManagerPage";

/**
 * 视图 ↔ URL 双向同步。
 *
 * activeView 映射为 hash 路由，刷新/深链均可恢复已打开的标签。
 */
function viewFromHash(hash: string): ActiveView {
  const m = hash.match(/^#\/apps\/([a-z_]+)$/);
  if (m && ["stem_separation", "libraries", "plugins"].includes(m[1])) {
    return m[1] as ActiveView;
  }
  if (hash.startsWith("#/settings")) return "settings";
  return "market";
}

function hashFromView(view: ActiveView): string {
  if (view === "market") return "#/apps/market";
  if (view === "settings") return "#/settings";
  return `#/apps/${view}`;
}

export default function App() {
  const { activeView, openApps, setActive } = useUI();
  const location = useLocation();
  const navigate = useNavigate();

  // URL → 视图：刷新 / 深链 / 浏览器前进后退
  useEffect(() => {
    const v = viewFromHash(location.hash || "#/apps/market");
    if (v !== activeView) setActive(v);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.hash]);

  // 视图 → URL：点击左栏时同步地址栏
  useEffect(() => {
    const target = hashFromView(activeView);
    if (location.hash !== target) navigate(target.replace(/^#/, ""), { replace: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeView]);

  return (
    <>
      <div className="flex h-full flex-col bg-background text-foreground">
        {/* 有可用更新时在顶部显示非模态提示条（自动检查 + 一键更新） */}
        <UpdateBanner />
        <div className="flex min-h-0 flex-1">
          <Sidebar />
          <main className="min-w-0 flex-1 overflow-auto p-6">
            {activeView === "market" && <MarketPage />}
            {activeView === "settings" && <SettingsPage />}
            {openApps.map((id) => (
              <div key={id} className={activeView === id ? "block" : "hidden"}>
                {id === "stem_separation" && <StemSeparationPage />}
                {id === "libraries" && <KontaktLibraryPage />}
                {id === "plugins" && <PluginManagerPage />}
              </div>
            ))}
          </main>
        </div>
      </div>

      {/* HeroUI 3 的 Toast 区域必须是**兄弟节点**，不能包裹应用：
          作为 wrapper 时其子元素完全不会被渲染（实测导致整个界面白屏）。
          toast() 是命令式 API，只需在此挂一个区域承载提示。 */}
      <Toast.Provider />
    </>
  );
}
