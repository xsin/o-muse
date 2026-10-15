import { useEffect } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useUI, type ActiveView } from "./store/useUI";
import Sidebar from "./components/Sidebar";
import MarketPage from "./pages/MarketPage";
import SettingsPage from "./pages/SettingsPage";
import StemSeparationPage from "./pages/StemSeparationPage";
import LibraryManagerPage from "./pages/LibraryManagerPage";
import PluginManagerPage from "./pages/PluginManagerPage";

/**
 * 视图 ↔ URL 双向同步（review 修复）。
 *
 * 此前 `<Routes>` 是摆设：`setActive` 不改 URL，刷新后永远回到市场页并丢失
 * 已打开的标签。现在把 activeView 映射为 hash 路由，刷新/深链均可恢复。
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
  const { activeView, openApps } = useUI();
  const location = useLocation();
  const navigate = useNavigate();
  const { setActive } = useUI();

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
    <div style={{ display: "flex", height: "100%" }}>
      <Sidebar />
      <main style={{ flex: 1, overflow: "auto", padding: 24, minWidth: 0 }}>
        {activeView === "market" && <MarketPage />}
        {activeView === "settings" && <SettingsPage />}
        {openApps.map((id) => (
          <div
            key={id}
            style={{ display: activeView === id ? "block" : "none" }}
          >
            {id === "stem_separation" && <StemSeparationPage />}
            {id === "libraries" && <LibraryManagerPage />}
            {id === "plugins" && <PluginManagerPage />}
          </div>
        ))}
      </main>
    </div>
  );
}
