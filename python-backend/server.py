"""O Muse 分轨后端服务（Task 2，独立实现）。

FastAPI 服务，承载音频分轨能力：
- POST   /separate          提交分轨任务，返回 job_id
- GET    /status/{job_id}   以 SSE 推送任务进度
- GET    /stems/{job_id}    返回已完成分轨文件的清单
- GET    /health            健康检查（Rust 端启动后轮询）

分轨引擎默认使用 Demucs（MIT）；未安装时自动回退到 demo 模式，
生成占位静音 WAV，保证整条调用链可联调。不复制任何第三方源码。
"""

import threading
import uuid
from pathlib import Path

import uvicorn
from fastapi import FastAPI, HTTPException
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

from demo import DemoSeparator
from separator import DemucsSeparator

import constants

app = FastAPI(title="O Muse Stem Separation Backend")

_jobs: dict[str, dict] = {}
_jobs_lock = threading.Lock()

_engine: DemoSeparator | DemucsSeparator


def _get_engine():
    """优先返回可用的 Demucs 引擎，否则回退 demo。"""
    real = DemucsSeparator()
    if real.available:
        return real
    return DemoSeparator()


_engine = _get_engine()


class SeparateRequest(BaseModel):
    file_path: str


def _run_job(job_id: str, input_path: str) -> None:
    """后台线程：执行分轨并更新任务状态。"""
    with _jobs_lock:
        job = _jobs[job_id]
    try:
        job["status"] = "processing"
        job["message"] = "separating"
        result = _engine.separate(input_path, str(constants.OUTPUT_DIR / job_id))
        job["stems"] = result["stems"]
        job["output_dir"] = result["output_dir"]
        job["is_demo"] = _engine.is_demo
        job["progress"] = 100.0
        job["status"] = "demo_done" if _engine.is_demo else "done"
    except Exception as exc:  # noqa: BLE001
        job["status"] = "error"
        job["message"] = str(exc)


@app.get("/health")
def health():
    return {"status": "ok", "engine": _engine.name, "demo": _engine.is_demo}


@app.post("/separate")
def separate(req: SeparateRequest):
    if not Path(req.file_path).exists():
        raise HTTPException(status_code=400, detail="file not found")
    job_id = uuid.uuid4().hex
    with _jobs_lock:
        _jobs[job_id] = {
            "job_id": job_id,
            "status": "queued",
            "progress": 0.0,
            "message": None,
            "stems": [],
            "output_dir": None,
            "is_demo": _engine.is_demo,
        }
    threading.Thread(target=_run_job, args=(job_id, req.file_path), daemon=True).start()
    return {"job_id": job_id}


def _event_stream(job_id: str):
    """以 SSE 逐帧推送状态，直到任务终态。"""
    import json
    import time

    terminal = {"done", "demo_done", "error"}
    while True:
        with _jobs_lock:
            job = _jobs.get(job_id)
        if job is None:
            yield f"data: {json.dumps({'job_id': job_id, 'status': 'error', 'message': 'not found'})}\n\n"
            return
        payload = {
            "job_id": job["job_id"],
            "status": job["status"],
            "progress": job["progress"],
            "message": job["message"],
            "demo": job["is_demo"],
            "stems": job["stems"],
        }
        yield f"data: {json.dumps(payload)}\n\n"
        if job["status"] in terminal:
            return
        time.sleep(0.3)


@app.get("/status/{job_id}")
def status(job_id: str):
    with _jobs_lock:
        if job_id not in _jobs:
            raise HTTPException(status_code=404, detail="job not found")
    return StreamingResponse(_event_stream(job_id), media_type="text/event-stream")


@app.get("/stems/{job_id}")
def stems(job_id: str):
    with _jobs_lock:
        job = _jobs.get(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="job not found")
    if job["status"] not in {"done", "demo_done"}:
        raise HTTPException(status_code=409, detail="not ready")
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
