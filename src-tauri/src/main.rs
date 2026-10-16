// 桌面端二进制入口（Tauri 2 要求 crate 提供 bin target）。
//
// 本 crate 的 `[lib]` 同时输出 `staticlib` / `cdylib` / `rlib`（供移动端与测试
// 使用），桌面端则由这里提供 `main`，调用 lib 中的 `run()`。
//
// 注意：此前缺少本文件，导致 `cargo build --bins` 无匹配目标、
// Tauri CLI 无法定位主二进制，`.app` / `.dmg` 打包从 Task 6 起一直失败
// （"can't open main binary .../o-muse"）。这是真实打包阻断问题。
//
// 提权助手复用同一个主二进制：`osascript … with administrator privileges`
// 会以 root 再次执行 `<app>/Contents/MacOS/o-muse`（开发态为 `target/debug/o-muse`），
// 此时参数形如 `--privileged-helper <request.json> <sha256>`。这里在**启动 Tauri 之前**
// 拦截该参数，只做一次受校验的写入/删除后退出（`std::process::exit` 只是退出码，
// 不是启动外部进程）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == o_muse_lib::core::kontakt::privileged::HELPER_FLAG {
        std::process::exit(o_muse_lib::core::kontakt::privileged::helper_entry(
            &args[2], &args[3],
        ));
    }
    o_muse_lib::run();
}
