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
