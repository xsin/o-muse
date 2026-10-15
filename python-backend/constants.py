"""分轨服务的常量配置（独立实现，不依赖任何第三方项目的私有配置）。"""

from pathlib import Path

# 服务监听地址（Rust 端 sidecar.rs 与之保持一致）。
HOST = "127.0.0.1"
PORT = 8787

# 分轨结果输出根目录（相对于本文件）。
OUTPUT_DIR = Path(__file__).resolve().parent / "stems"

# Demucs 默认 4  stem 命名；实际以磁盘产出为准。
DEFAULT_STEM_NAMES = ["vocals", "drums", "bass", "other"]

# Demucs 模型名（htdemucs 为官方默认 4-stem 模型）。
DEMUCS_MODEL = "htdemucs"

# 健康检查轮询上限（秒），仅用于日志提示。
HEALTH_TIMEOUT_SECONDS = 30
