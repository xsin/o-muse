import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望固定端口 5173；构建产物输出到仓库根 dist（由 tauri.conf.json 的
// frontendDist "../dist" 指向）。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2021",
  },
});
