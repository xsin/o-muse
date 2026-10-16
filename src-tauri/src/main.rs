// 桌面端二进制入口（Tauri 2 要求 crate 提供 bin target）。
//
// 本 crate 的 `[lib]` 同时输出 `staticlib` / `cdylib` / `rlib`（供移动端与测试
// 使用），桌面端则由这里提供 `main`，调用 lib 中的 `run()`。
//
// 注意：此前缺少本文件，导致 `cargo build --bins` 无匹配目标、
// Tauri CLI 无法定位主二进制，`.app` / `.dmg` 打包从 Task 6 起一直失败
// （"can't open main binary .../o-muse"）。这是真实打包阻断问题。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    o_muse_lib::run();
}
