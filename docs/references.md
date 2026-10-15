# 外部参考记录

本文件记录 Music Rig 开发过程中参考过的外部项目。所有参考均限于
"研究功能与设计思路"，未复制任何源代码。本文件用于证明独立实现
过程，不具有法律约束力，但保留以备查证。

---

## StemDeck
- **URL**: https://github.com/stemdeckapp/stemdeck
- **许可证**: Apache License 2.0
- **参考方面**:
  - 多轨波形编辑器的交互设计（缩放、静音、独奏、推子的用户流程）
  - Python sidecar 进程的生命周期管理思路（启动、健康检查、退出清理）
  - 后端 watchdog 机制的架构模式
- **使用方式**: 仅作为设计参考，未复制任何代码。实现完全独立。
- **拉取 commit（已删除，仅存档）**: 5f9405ca68ac31b31ae07ac416141d4d1f7b5f3a
- **拉取日期**: 2026-10-15

---

## Demucs
- **URL**: https://github.com/facebookresearch/demucs
- **许可证**: MIT License
- **使用方式**: 作为 Python 依赖直接安装使用（`pip install demucs`）。
  本项目的分轨能力完全来自 Demucs 模型，StemDeck 只是 Demucs 的一个
  使用范例。
- **依赖引入位置**: python-backend/requirements.txt

---

## KLM - Kontakt Library Manager
- **URL**: https://github.com/cloinse/klm
- **许可证**: MIT License
- **参考方面**:
  - Kontakt 库元数据的解析逻辑（Service Center XML、plist、JSON 的结构）
  - 安全文件操作模型（不安装后台服务、不调用 sudo、直接写用户偏好）
- **使用方式**: 仅作为逻辑参考，用 Rust 独立重写。未复制 KLM 的 Flutter/Dart 源码。

---

## Tauri
- **URL**: https://github.com/tauri-apps/tauri
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: 作为桌面应用框架直接依赖。

---

## 其他依赖
（后续新增依赖时，在此追加记录，注明 URL、许可证、用途。）

### FastAPI
- **URL**: https://github.com/fastapi/fastapi
- **许可证**: MIT License
- **使用方式**: Python 分轨后端（Task 2）的 Web 框架，提供 `/separate`、`/status`、`/stems`、`/health` 接口。作为依赖直接安装（`python-backend/requirements.txt`）。

### Uvicorn
- **URL**: https://github.com/encode/uvicorn
- **许可证**: BSD License
- **使用方式**: 运行 FastAPI 分轨后端的 ASGI 服务器（`python-backend/requirements.txt`）。

### librosa
- **URL**: https://github.com/librosa/librosa
- **许可证**: ISC License
- **使用方式**: 音频分析辅助库（`python-backend/requirements.txt`），Task 2 暂未直接使用，预留给后续响度/标准化处理。

### pyloudnorm
- **URL**: https://github.com/csteinmetz1/pyloudnorm
- **许可证**: MIT License
- **使用方式**: 响度测量辅助库（`python-backend/requirements.txt`），预留给后续分轨后响度对齐。

### reqwest
- **URL**: https://github.com/seanmonstar/reqwest
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: Rust 端向 Python sidecar 发起 blocking HTTP 请求的客户端（Task 2，`src-tauri/Cargo.toml`）。

### quick-xml
- **URL**: https://github.com/tafia/quick-xml
- **许可证**: MIT License
- **使用方式**: Rust 端解析 Native Instruments Service Center XML（`<Application>` 条目）的 XML 解析库（Task 3，`src-tauri/Cargo.toml`）。

### plist
- **URL**: https://github.com/rustsec/plist-rs
- **许可证**: MIT License
- **使用方式**: Rust 端解析 Kontakt 偏好 plist（已注册库与路径）的 plist 解析库（Task 3，`src-tauri/Cargo.toml`）。
