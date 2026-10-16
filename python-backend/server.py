"""O Muse 分轨后端服务（独立实现）。

FastAPI 服务，承载音频分轨与曲目分析能力：

- POST   /separate        提交分轨任务，返回 job_id
- POST   /analyze         提交**仅分析**任务（调性 + BPM），返回 job_id
- GET    /status/{job_id} 查询任务状态（JSON：进度、阶段、分轨清单、分析结果、设备）
- GET    /stems/{job_id}  返回已完成分轨文件的清单
- GET    /health          健康检查（Rust 端启动后轮询）

状态机保持不变：``queued → processing → done | demo_done | error``。
``/status`` 的响应体在原有字段（job_id / status / progress / message / demo /
stems）之上新增 ``stage`` / ``analysis`` / ``device`` / ``kind`` / ``device_note``。

分轨引擎默认使用 Demucs（MIT），未安装时自动回退到 demo 模式（占位静音 WAV），
保证整条调用链可联调。分析引擎是纯 numpy 实现（见 ``analysis.py``）。
不复制任何第三方源码。
"""

import inspect
import threading
import uuid
from pathlib import Path
from typing import Callable

import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

import analysis
import constants
from analysis import AudioLoadError
from demo import DemoSeparator
from separator import DemucsSeparator, SeparationError

app = FastAPI(title="O Muse Stem Separation Backend")

_jobs: dict[str, dict] = {}
_jobs_lock = threading.Lock()

_engine: "DemoSeparator | DemucsSeparator"


def _get_engine():
    """优先返回可用的 Demucs 引擎，否则回退 demo。"""
    real = DemucsSeparator()
    if real.available:
        return real
    return DemoSeparator()


_engine = _get_engine()


class FileRequest(BaseModel):
    """``/separate`` 与 ``/analyze`` 共用的请求体。"""

    file_path: str


# ── 任务状态维护 ─────────────────────────────────────────


def _update(job_id: str, **fields) -> None:
    with _jobs_lock:
        job = _jobs.get(job_id)
        if job is not None:
            job.update(fields)


def _progress_callback(job_id: str, low: float, high: float) -> Callable:
    """把引擎回报的 0~1 进度线性映射到任务进度的 [low, high] 区间。"""

    def cb(frac: float, stage: str) -> None:
        try:
            value = max(0.0, min(1.0, float(frac)))
        except (TypeError, ValueError):
            return
        _update(
            job_id,
            progress=round(low + (high - low) * value, 1),
            stage=stage,
            status="processing",
        )

    return cb


def _engine_accepts_progress() -> bool:
    """demo.py 不在本次改动范围内：按签名决定是否传 ``progress``。"""
    try:
        params = inspect.signature(_engine.separate).parameters
    except (TypeError, ValueError):  # pragma: no cover
        return False
    return "progress" in params


def _new_job(file_path: str, kind: str) -> str:
    job_id = uuid.uuid4().hex
    with _jobs_lock:
        _jobs[job_id] = {
            "job_id": job_id,
            "kind": kind,
            "status": "queued",
            "stage": "queued",
            "progress": 0.0,
            "message": None,
            "stems": [],
            "output_dir": None,
            "is_demo": False,
            "analysis": None,
            "device": None,
            "device_note": None,
        }
    threading.Thread(
        target=_run_job, args=(job_id, file_path, kind), daemon=True
    ).start()
    return job_id


# ── 任务执行 ─────────────────────────────────────────────


def _run_job(job_id: str, input_path: str, kind: str) -> None:
    """后台线程：执行分轨或分析，并持续更新任务状态。"""
    try:
        _update(job_id, status="processing", stage="queued", progress=1.0)
        if kind == "analyze":
            _run_analysis_only(job_id, input_path)
        else:
            _run_separation(job_id, input_path)
    except (SeparationError, AudioLoadError) as exc:
        # 这两类异常的 message 已经是面向用户的可操作文案
        _update(job_id, status="error", stage="failed", message=str(exc))
    except Exception as exc:  # noqa: BLE001
        _update(
            job_id,
            status="error",
            stage="failed",
            message=f"{type(exc).__name__}: {exc}",
        )


def _run_analysis_only(job_id: str, input_path: str) -> None:
    data = analysis.analyze(input_path, progress=_progress_callback(job_id, 2.0, 100.0))
    _update(
        job_id,
        analysis=data,
        status="done",
        stage="done",
        progress=100.0,
    )


def _run_separation(job_id: str, input_path: str) -> None:
    kwargs = {}
    if _engine_accepts_progress():
        kwargs["progress"] = _progress_callback(job_id, 1.0, 88.0)
    result = _engine.separate(
        input_path, str(constants.OUTPUT_DIR / job_id), **kwargs
    )
    _update(
        job_id,
        stems=result["stems"],
        output_dir=result["output_dir"],
        is_demo=_engine.is_demo,
        device=result.get("device"),
        device_note=result.get("device_note"),
        progress=88.0,
        stage="analyzing",
    )

    # 分轨成功后顺带做一次调性/BPM 分析：同一首歌没必要让用户点两次。
    # 分析失败不影响分轨结果，只记下 message（status 仍为终态成功）。
    try:
        data = analysis.analyze(
            input_path, progress=_progress_callback(job_id, 88.0, 99.0)
        )
        _update(job_id, analysis=data)
    except Exception as exc:  # noqa: BLE001
        _update(job_id, message=f"分析失败（不影响分轨结果）：{exc}")

    _update(
        job_id,
        progress=100.0,
        stage="done",
        status="demo_done" if _engine.is_demo else "done",
    )


# ── 路由 ─────────────────────────────────────────────────


@app.get("/health")
def health():
    return {"status": "ok", "engine": _engine.name, "demo": _engine.is_demo}


@app.post("/separate")
def separate(req: FileRequest):
    if not Path(req.file_path).exists():
        raise HTTPException(status_code=400, detail="file not found")
    return {"job_id": _new_job(req.file_path, "separate")}


@app.post("/analyze")
def analyze(req: FileRequest):
    """仅做调性 + BPM 分析，不执行分轨（秒级返回）。"""
    if not Path(req.file_path).exists():
        raise HTTPException(status_code=400, detail="file not found")
    return {"job_id": _new_job(req.file_path, "analyze")}


@app.get("/status/{job_id}")
def status(job_id: str):
    """任务状态（JSON）。

    注意：此前这里以 SSE 推送，但 Rust 端 ``task::status`` 用
    ``resp.json()`` 反序列化，SSE 帧（``data: {...}\\n\\n``）永远解析失败。
    这里改为一次性 JSON，并顺带补上新字段。
    """
    with _jobs_lock:
        job = _jobs.get(job_id)
        if job is None:
            raise HTTPException(status_code=404, detail="job not found")
        return {
            "job_id": job["job_id"],
            "kind": job["kind"],
            "status": job["status"],
            "stage": job["stage"],
            "progress": job["progress"],
            "message": job["message"],
            "demo": job["is_demo"],
            "stems": list(job["stems"]),
            "analysis": job["analysis"],
            "device": job["device"],
            "device_note": job["device_note"],
        }


@app.get("/stems/{job_id}")
def stems(job_id: str):
    with _jobs_lock:
        job = _jobs.get(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="job not found")
    if job["status"] not in {"done", "demo_done"}:
        raise HTTPException(status_code=409, detail="not ready")

    # 仅分析任务没有输出目录：直接返回空清单，而不是让 Path(None) 崩掉
    if not job.get("output_dir"):
        return {"job_id": job_id, "path": "", "files": []}

    base = Path(job["output_dir"])
    files = []
    for name in job["stems"]:
        p = base / f"{name}.wav"
        if p.exists():
            files.append({"name": name, "path": str(p), "size": p.stat().st_size})
    return {"job_id": job_id, "path": str(base), "files": files}


def main() -> None:
    uvicorn.run(app, host=constants.HOST, port=constants.PORT, log_level="info")


if __name__ == "__main__":
    main()
