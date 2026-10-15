// 插件扫描核心（v1.1 Task 7/8）
//
// 只读发现 + 只读元数据提取。**绝不 dlopen / LoadLibrary** 宿主插件二进制
// （tdd.md §3.3 全局禁止）——所有信息来自 plist 文本与文件头字节。

pub mod cache;
pub mod custom;
pub mod discover;
pub mod extract;
pub mod model;

pub use cache::{extract_with_cache, open_in_memory, CacheStats};
pub use discover::{default_sources, discover_plugins, discover_plugins_par};
pub use extract::extract_all;
pub use model::{PluginFormat, PluginInfo, ScanSource};
