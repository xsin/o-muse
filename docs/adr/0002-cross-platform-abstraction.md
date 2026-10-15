# ADR-0002: 跨平台路径/目录抽象（为 v1.5 Windows 铺路）

- **状态**：已采纳（2026-10-15）
- **决策人**：用户（指令）
- **相关**：plan Task 3 / Task 7 原生路径

## 背景

v1 仅 macOS（MVP），但 v1.5 计划推 Windows。原始 plan 中 Task 3/Task 7 的路径全部硬编码 macOS 约定：

- macOS：`~/Library/Application Support/Native Instruments/...`、`~/Library/Preferences/com.native-instruments.Kontakt*.plist`、`/Library/Audio/Plug-Ins/{Components,VST,VST3,CLAP}`、`~/Library/Audio/Plug-Ins/...`、`/Library/Application Support/Avid/Audio/Plug-Ins/`、`.component/.vst/.vst3/.aaxplugin/.clap` 扩展名。
- Windows（v1.5）：`C:\Program Files\Common Files\VST3\`、用户级 `%APPDATA%\...`、`.vst3/.dll` 等。

若 v1 直接写死 macOS 路径，v1.5 需大面积重构。

## 决策

v1 即引入**薄 Platform Abstraction 层**（建议 `src-tauri/src/core/platform_paths.rs`），用 `cfg(target_os = "macos" | "windows")` 分隔，对外暴露与 OS 无关的函数：

- `native_instruments_service_center_dirs() -> Vec<PathBuf>`
- `kontakt_preferences_glob() -> String` / 解析器
- `default_plugin_scan_sources() -> Vec<ScanSource>`（Task 7 的 `discover.rs` 改调此层）
- 扩展名匹配表按 OS 归并

v1 仅实现 **macOS 分支**；Windows 分支以 `todo!()` / TODO 注释占位，并在 TDD 标注 v1.5 接入点。

## 理由

- 代价极小（一个薄模块 + 改两处调用），换来 v1.5 几乎无重构。
- 符合"现在做平台抽象"的用户指令。

## 权衡 / 风险

- 薄抽象若过度设计会引入复杂度；约束为"仅封装路径来源，不封装行为"。
- v1 不编译 Windows target，Windows 分支需 `#[cfg(target_os="windows")]` 保证 v1 macOS 编译不被影响。

## 后果

- Task 3 的解析函数签名不变，但默认路径来源改为 `platform_paths`。
- Task 7 `default_sources()` 改为委托 `platform_paths::default_plugin_scan_sources()`。
- TDD 记录 `platform_paths` 模块契约与 v1.5 TODO 清单。
