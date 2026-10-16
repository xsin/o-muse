"""真实分轨器（Demucs Python API，独立实现）。

流程：读入音频 → 重采样到模型采样率（htdemucs 为 44.1kHz）并转成模型要求的
声道数 → ``demucs.apply.apply_model`` 真正执行推理 → 每条 stem 写成 16-bit
PCM WAV。

设备：优先 Apple MPS（``torch.backends.mps.is_available()``），否则 CPU。
MPS 上部分算子尚未实现时会自动回退 CPU 重跑一次，并把**实际生效的设备**
与回退原因一起回报（不静默吞掉）。

错误：一律转成面向用户、可操作的 ``SeparationError``（缺 ffmpeg / 格式不支持 /
模型下载失败 / 缺 torch），不把裸 traceback 抛给前端。

输出 WAV 的响度处理与 demucs CLI 的 ``--clip-mode rescale`` 一致：单条 stem
峰值超过 0dBFS 时整体等比缩放，而不是硬削波。
"""

from __future__ import annotations

import inspect
import shutil
import wave
from pathlib import Path
from typing import Callable, Optional, Sequence

import numpy as np

import constants

ProgressCallback = Callable[[float, str], None]


class SeparationError(RuntimeError):
    """分轨失败；message 面向用户且给出可操作的下一步。"""


# ── 设备选择 ─────────────────────────────────────────────


def resolve_device(prefer: Optional[str] = None) -> str:
    """返回要使用的推理设备：优先 MPS，否则 CPU。"""
    try:
        import torch
    except Exception as exc:  # pragma: no cover - 依赖缺失时才会走到
        raise SeparationError(
            "未找到 PyTorch，分轨无法运行。\n"
            "请在「设置 → 分轨运行时」点击「下载并安装」重建运行时环境。"
            f"（原始错误：{type(exc).__name__}: {exc}）"
        ) from exc

    if prefer:
        return prefer
    try:
        if torch.backends.mps.is_available():
            return "mps"
    except Exception:
        pass
    return "cpu"


def _short(exc: BaseException, limit: int = 240) -> str:
    """异常 → 单行短文本（前端展示用，避免整段 traceback）。"""
    text = " ".join(str(exc).split())
    if not text:
        text = type(exc).__name__
    return text if len(text) <= limit else text[: limit - 1] + "…"


def _emit(progress: Optional[ProgressCallback], frac: float, stage: str) -> None:
    if progress is None:
        return
    try:
        progress(max(0.0, min(1.0, float(frac))), stage)
    except Exception:  # 进度回调绝不能拖垮主流程
        pass


# ── 音频读入 ─────────────────────────────────────────────


def _missing_ffmpeg_tools() -> list[str]:
    return [name for name in ("ffmpeg", "ffprobe") if shutil.which(name) is None]


def _read_wav_for_model(path: Path, samplerate: int, channels: int):
    """WAV 直读（``wave`` + numpy）——本机没有 ffmpeg 时的兜底路径。"""
    import torch

    with wave.open(str(path), "rb") as wf:
        n_channels = wf.getnchannels()
        sampwidth = wf.getsampwidth()
        src_rate = wf.getframerate()
        n_frames = wf.getnframes()
        if wf.getcomptype() != "NONE":
            raise SeparationError(
                f"WAV「{path.name}」使用了压缩编码，请先转换为未压缩 PCM WAV。"
            )
        if sampwidth not in (1, 2, 4):
            raise SeparationError(
                f"WAV「{path.name}」位深为 {sampwidth * 8} bit，"
                "内置兜底读取仅支持 8/16/32 bit PCM；请安装 ffmpeg 或转换该文件。"
            )
        raw = wf.readframes(n_frames)

    dtype = {1: np.uint8, 2: np.int16, 4: np.int32}[sampwidth]
    data = np.frombuffer(raw, dtype=dtype).astype(np.float32)
    if sampwidth == 1:
        data = (data - 128.0) / 128.0
    else:
        data = data / float(1 << (8 * sampwidth - 1))
    usable = (data.shape[0] // n_channels) * n_channels
    # (samples, channels) → (channels, samples)
    data = data[:usable].reshape(-1, n_channels).T

    if src_rate != samplerate:
        from analysis import _resample  # 复用同一套重采样，避免两处口径不一致

        data = np.stack([_resample(row, src_rate, samplerate) for row in data])

    wav = torch.from_numpy(np.ascontiguousarray(data, dtype=np.float32))
    if wav.shape[0] == channels:
        return wav
    if wav.shape[0] == 1:
        return wav.expand(channels, wav.shape[-1]).contiguous()
    if wav.shape[0] > channels:
        return wav[:channels].contiguous()
    raise SeparationError(
        f"音频「{path.name}」只有 {wav.shape[0]} 个声道，模型需要 {channels} 个。"
    )


def load_audio_for_model(path: Path, samplerate: int, channels: int):
    """读入音频为 ``(channels, N)`` float32 @ ``samplerate``。

    优先 ``demucs.audio.AudioFile``（ffmpeg/ffprobe 后端，支持 mp3/flac/m4a/…），
    失败且是 WAV 时回退到 ``wave`` 直读；都不是就给可操作的报错。
    """
    try:
        from demucs.audio import AudioFile, convert_audio
    except Exception as exc:
        raise SeparationError(
            "未找到 demucs 音频模块（可能缺少 lameenc / julius 等依赖）。\n"
            "请在「设置 → 分轨运行时」重新安装运行时。"
            f"（原始错误：{type(exc).__name__}: {_short(exc)}）"
        ) from exc

    demucs_error: Optional[BaseException] = None
    try:
        af = AudioFile(path)
        src_rate = af.samplerate(0)
        wav = af.read(streams=0)
        return convert_audio(wav, src_rate, samplerate, channels).contiguous()
    except Exception as exc:  # ffmpeg/ffprobe 缺失，或格式不受支持
        demucs_error = exc

    if path.suffix.lower() in (".wav", ".wave"):
        try:
            return _read_wav_for_model(path, samplerate, channels)
        except SeparationError:
            raise
        except Exception as exc:
            demucs_error = exc

    missing = _missing_ffmpeg_tools()
    hint = (
        "本机没有找到 ffmpeg/ffprobe（"
        + "、".join(missing)
        + "），而 Demucs 解码非 WAV 音频依赖它们。"
        if missing
        else "Demucs 无法解码该文件（编码不受支持或文件损坏）。"
    )
    raise SeparationError(
        f"无法读取音频「{path.name}」：{_short(demucs_error)}\n"
        f"{hint}\n"
        "可尝试的修复：① macOS 上 `brew install ffmpeg`；"
        "② 把音频导出为未压缩 PCM WAV 后重试。"
    )


# ── WAV 输出 ─────────────────────────────────────────────


def _to_numpy(data) -> np.ndarray:
    if hasattr(data, "detach"):
        data = data.detach().cpu().numpy()
    return np.asarray(data, dtype=np.float32)


def write_wav_16(path: Path, data, samplerate: int) -> None:
    """把 ``(channels, N)`` float 波形写成 16-bit PCM WAV。

    与 demucs CLI 的 ``--clip-mode rescale`` 同口径：峰值超过 0dBFS 时整体
    等比缩放（保留 stem 内部动态），而不是硬削波。
    """
    arr = _to_numpy(data)
    if arr.ndim == 1:
        arr = arr[None, :]
    if arr.size == 0:
        raise SeparationError("分轨结果为空（模型没有输出任何采样点）。")

    peak = float(np.abs(arr).max())
    if peak > 0.0:
        arr = arr / max(1.01 * peak, 1.0)

    pcm = (np.clip(arr, -1.0, 1.0) * 32767.0).round().astype("<i2")
    interleaved = np.ascontiguousarray(pcm.T).reshape(-1)  # (samples, channels)

    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(pcm.shape[0])
        wf.setsampwidth(2)
        wf.setframerate(samplerate)
        wf.writeframes(interleaved.tobytes())


# ── 分轨器 ───────────────────────────────────────────────


class DemucsSeparator:
    """封装 Demucs 的分离逻辑（独立实现，不复制任何第三方源码）。"""

    is_demo = False
    name = "demucs"

    def __init__(self, model_name: str = constants.DEMUCS_MODEL) -> None:
        self.model_name = model_name
        self._available = False
        self._reason = ""
        self._supports_callback = False
        try:
            import torch  # noqa: F401
            from demucs.apply import apply_model
            from demucs.audio import AudioFile  # noqa: F401  能力探测（lameenc 等）
            from demucs.pretrained import get_model  # noqa: F401

            # demucs 4.1 才引入官方进度回调 `callback`；4.0.x 传了会 TypeError
            self._supports_callback = "callback" in inspect.signature(
                apply_model
            ).parameters
            self._available = True
        except Exception as exc:
            self._reason = f"{type(exc).__name__}: {_short(exc)}"

    @property
    def available(self) -> bool:
        return self._available

    @property
    def unavailable_reason(self) -> str:
        return self._reason

    def _apply(self, model, wav, device: str, progress: Optional[ProgressCallback]):
        """调用 apply_model，并把 demucs 的进度回调翻译成 0~1 进度。"""
        import torch
        from demucs.apply import apply_model

        total = max(1, int(wav.shape[-1]))
        segment = getattr(model, "segment", None) or 7.8
        segment_samples = float(segment) * float(getattr(model, "samplerate", 44100))
        state = {"frac": 0.0}

        def callback(payload) -> None:
            if not isinstance(payload, dict) or payload.get("state") != "end":
                return
            offset = float(payload.get("segment_offset") or 0.0)
            frac = min(1.0, (offset + segment_samples) / total)
            frac = max(frac, state["frac"])  # 保证单调递增
            state["frac"] = frac
            _emit(progress, 0.15 + 0.80 * frac, "separating")

        kwargs: dict = dict(
            device=device, shifts=1, split=True, overlap=0.25, progress=False
        )
        if self._supports_callback:
            kwargs["callback"] = callback

        with torch.no_grad():
            out = apply_model(model, wav[None], **kwargs)
        return out[0]

    def separate(
        self,
        input_path: str,
        output_dir: str,
        progress: Optional[ProgressCallback] = None,
    ) -> dict:
        """执行分轨并把每条 stem 写成 WAV。

        ``progress(frac, stage)`` 的 ``frac`` 覆盖本次分轨的 0~1（模型加载、
        读音频、推理、写盘）。返回 ``output_dir`` / ``stems`` / ``model`` /
        ``device`` / ``sample_rate`` / ``device_note``。
        """
        if not self._available:
            raise SeparationError(
                f"分轨引擎不可用（{self._reason}）。\n"
                "请在「设置 → 分轨运行时」点击「下载并安装」重建运行时环境。"
            )

        from demucs.pretrained import get_model

        src = Path(input_path)
        if not src.is_file():
            raise SeparationError(f"输入文件不存在：{input_path}")
        if not src.suffix:
            raise SeparationError(
                f"无法判断音频格式（文件没有扩展名）：{src.name}。"
                "请保留 .wav / .mp3 / .flac 等扩展名后重试。"
            )

        out = Path(output_dir)
        out.mkdir(parents=True, exist_ok=True)

        preferred = resolve_device()
        _emit(progress, 0.02, "loading_model")
        try:
            model = get_model(self.model_name)
        except Exception as exc:
            raise SeparationError(
                f"分轨模型「{self.model_name}」加载失败：{_short(exc)}\n"
                "首次运行需要联网下载模型权重（约 80MB）。"
                "请确认网络可用，或在「设置 → 分轨运行时」重新安装运行时。"
            ) from exc

        samplerate = int(getattr(model, "samplerate", 44100))
        channels = int(getattr(model, "audio_channels", 2))
        sources: Sequence[str] = list(model.sources)

        _emit(progress, 0.10, "reading_audio")
        try:
            wav = load_audio_for_model(src, samplerate, channels)
        except SeparationError:
            raise
        except Exception as exc:
            raise SeparationError(
                f"读取音频「{src.name}」失败：{_short(exc)}\n"
                "请确认文件未损坏；非 WAV 格式需要本机安装 ffmpeg。"
            ) from exc

        if wav.shape[-1] < samplerate // 10:
            raise SeparationError(f"音频「{src.name}」有效长度不足 0.1 秒，无法分轨。")

        _emit(progress, 0.15, "separating")
        device = preferred
        device_note: Optional[str] = None
        try:
            stems = self._apply(model, wav, device, progress)
        except SeparationError:
            raise
        except Exception as exc:
            if device == "cpu":
                raise SeparationError(
                    f"分轨推理失败：{_short(exc)}\n"
                    "可尝试：① 更新 PyTorch（`pip install -U torch`）；"
                    "② 在「设置 → 分轨运行时」重新安装运行时。"
                ) from exc
            # MPS 上仍有未实现的算子：明确告知并回退 CPU 重跑，不静默降级
            device_note = (
                f"{device.upper()} 推理失败，已自动回退 CPU：{_short(exc, 160)}"
            )
            _emit(progress, 0.15, "fallback_cpu")
            try:
                stems = self._apply(model, wav, "cpu", progress)
                device = "cpu"
            except Exception as cpu_exc:
                raise SeparationError(
                    f"分轨推理失败（{preferred.upper()} 与 CPU 均失败）："
                    f"{_short(cpu_exc)}\n"
                    "可尝试在「设置 → 分轨运行时」重新安装运行时。"
                ) from cpu_exc

        _emit(progress, 0.96, "writing_stems")
        written: list[str] = []
        for index, name in enumerate(sources):
            write_wav_16(out / f"{name}.wav", stems[index], samplerate)
            written.append(str(name))

        _emit(progress, 1.0, "separating_done")
        return {
            "output_dir": str(out),
            "stems": written,
            "model": self.model_name,
            "sample_rate": samplerate,
            "device": device,
            "device_preferred": preferred,
            "device_note": device_note,
        }
