// 插件扫描核心（v1.1 Task 7/8；v1.2 增补空间管理与安全移动）
//
// 只读发现 + 只读元数据提取。**绝不 dlopen / LoadLibrary** 宿主插件二进制
// （tdd.md §3.3 全局禁止）——所有信息来自 plist 文本与文件头字节。
//
// v1.2 `move_plugin` 是唯一的"写"模块：把插件真实文件搬到其它卷、在原位置
// 留软链，并提供"移入废纸篓"的安全删除。

pub mod cache;
pub mod custom;
pub mod discover;
pub mod extract;
pub mod model;
pub mod move_plugin;

pub use cache::{extract_with_cache, open_in_memory, CacheStats};
pub use discover::{default_sources, default_sources_vst3, discover_plugins, discover_plugins_par};
pub use extract::extract_all;
pub use model::{PluginFormat, PluginInfo, ScanSource};
pub use move_plugin::{execute_move, plan_move, trash_plugin, undo_move, MovePlan};
