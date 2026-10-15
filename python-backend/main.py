"""Music Rig 分轨后端入口（Task 2）。

作为 Tauri sidecar 启动的 FastAPI 服务。独立实现，仅将 Demucs
（MIT）作为普通 Python 依赖调用。Demucs 缺失时自动回退 demo 模式。
"""

from server import main

if __name__ == "__main__":
    main()
