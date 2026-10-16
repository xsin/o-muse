import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri 期望固定端口 5173；构建产物输出到仓库根 dist（由 tauri.conf.json 的
// frontendDist "../dist" 指向）。
//
// 样式栈：Tailwind CSS v4（经 @tailwindcss/vite 插件）+ HeroUI 3。
// HeroUI 的样式在 src/styles/index.css 中以 `@import "@heroui/styles"` 引入。
export default defineConfig({
  plugins: [react(), tailwindcss()],
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
