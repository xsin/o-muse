import { Routes, Route, Navigate } from "react-router-dom";
import { useUI } from "./store/useUI";
import Sidebar from "./components/Sidebar";
import MarketPage from "./pages/MarketPage";
import SettingsPage from "./pages/SettingsPage";
import StemSeparationPage from "./pages/StemSeparationPage";
import LibraryManagerPage from "./pages/LibraryManagerPage";
import PluginManagerPage from "./pages/PluginManagerPage";

export default function App() {
  const { activeView, openApps } = useUI();

  return (
    <div style={{ display: "flex", height: "100%" }}>
      <Sidebar />
      <main style={{ flex: 1, overflow: "auto", padding: 24, minWidth: 0 }}>
        {activeView === "market" && <MarketPage />}
        {activeView === "settings" && <SettingsPage />}
        <Routes>
          <Route path="/" element={<Navigate to="/apps/market" replace />} />
          <Route path="/apps/market" element={null} />
          {/* 标签内容由 activeView 控制，路由仅用于深链兼容 */}
        </Routes>
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
