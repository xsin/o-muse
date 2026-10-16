"""调性（key）与速度（BPM）分析 —— **纯 numpy 实现**。

为什么不用 librosa
------------------
librosa 会连带引入 numba / llvmlite / scipy / soundfile / soxr 等，
安装体积与冷启动成本远超本应用的收益，因此运行时依赖里已刻意移除它。
本模块只依赖 numpy（运行时强制安装）与标准库；音频解码复用 demucs 自带的
``demucs.audio.AudioFile``（ffmpeg 后端），并为最常用的 WAV 提供
``wave`` 直读兜底（本机没有 ffmpeg 时仍可分析 WAV）。

算法
----
调性：STFT 幅度谱 → 折叠成 12 维 chroma（按对数频率把每个频点分给最近的
      半音，并用小数偏差做双线性摊分以减少量化误差）→ 逐帧 L2 归一化后累加
      → 与 Krumhansl-Schmuckler 大/小调模板做 Pearson 相关（24 个调各算一次）
      → 取最大者，映射到 Camelot 轮编号。
速度：STFT 幅度谱 → 谱通量（spectral flux）→ onset 包络 → 自相关
      → 在 60–200 BPM 对应的 lag 区间内取主峰（乘一个中心 120 BPM 的对数
      正态先验，抑制 60/120 这类八度歧义）→ 抛物线插值细化 lag
      → 八度修正（<70 ×2、>180 ÷2）。

精度代价（诚实记录，"纯 numpy 不引入 librosa"换来的损失）
--------------------------------------------------------
- **没有 HPSS（谐波/打击分离）**：鼓点密集处 percussive 能量会污染 chroma，
  这是本方案最大的精度损失。librosa 的 chroma_cqt 通常带 HPSS 预处理。
- **重采样是 box 平均 / 线性插值**，不是多相 sinc。对 chroma（频谱形状）与
  onset（包络）影响很小，但对需要精确相位的任务不适用。
- **chroma 用 STFT 折叠而非 CQT**：8192 点 FFT 在 44.1kHz 下频率分辨率约
  5.4Hz，低于 ~90Hz 时一个半音窄于一个频点，故 chroma 下限取 65Hz(C2)
  且低频精度天然弱于 CQT。转调乐器 / 强低频 rap 上更易错。
- **没有动态规划节拍跟踪**（Ellis 2007 / librosa.beat.beat_track 的 DP）：
  只做自相关 + 先验 + 八度修正，对渐快渐慢（rubato）的曲目不可靠。
- 只分析**中段最多 60 秒**（见 ``MAX_ANALYSIS_SECONDS``），因此一首歌内部
  有转调或变速时，输出代表的是中段那 60 秒。
"""

from __future__ import annotations

import json
import math
import sys
import wave
from pathlib import Path
from typing import Callable, Optional, Tuple

import numpy as np

# ── 分析参数 ─────────────────────────────────────────────
#
# 统一重采样到 22.05kHz：chroma 只关心 65Hz–2.1kHz，onset 只关心包络，
# 22.05kHz 已远超需求，且能让 STFT 计算量减半。
ANALYSIS_SR = 22050

# 长音频只取中段 60 秒。调性/速度是**全局平稳统计量**：60 秒（约 120 拍）
# 足以稳定估计，而 10 分钟曲目全量解码会让分析慢一个数量级却几乎没有精度收益。
MAX_ANALYSIS_SECONDS = 60.0

# chroma：高频率分辨率优先，hop 大一些（chroma 不需要时间精细度）
CHROMA_N_FFT = 8192
CHROMA_HOP = 4096
CHROMA_FMIN = 65.0    # C2，低于此频点一个半音窄于一个 FFT 频点
CHROMA_FMAX = 2100.0  # ~C7

# onset / tempo：低频率分辨率可以接受，时间分辨率必须细（lag 精度直接决定 BPM 精度）
ONSET_N_FFT = 2048
ONSET_HOP = 256

# onset 包络里能量低于峰值这个比例的帧在 chroma 统计中被丢弃（近似静音）
SILENT_FRAME_RATIO = 1e-3

# 调性置信度的 softmax 温度：相关系数差 0.02 即视为"非常确定"
KEY_SOFTMAX_TEMPERATURE = 0.02

# 自相关的对数正态节拍先验（中心 120 BPM），抑制 60↔120 八度歧义
TEMPO_PRIOR_CENTER = 120.0
TEMPO_PRIOR_WIDTH = 0.9  # 以 octave 为单位的 sigma

TEMPO_SEARCH_MIN = 60.0
TEMPO_SEARCH_MAX = 200.0
TEMPO_OCTAVE_LOW = 70.0   # < 70 BPM → ×2
TEMPO_OCTAVE_HIGH = 180.0  # > 180 BPM → ÷2

PITCH_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

# Krumhansl-Schmuckler 调性轮廓（音级 0 = 主音）
KS_MAJOR = np.array(
    [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88],
    dtype=np.float64,
)
KS_MINOR = np.array(
    [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17],
    dtype=np.float64,
)

# Camelot 轮（以音级索引 0=C, 9=A 为锚点；C major=8B、A minor=8A 为本表自洽结果）
#
# 大调：C=8B G=9B D=10B A=11B E=12B B=1B F#=2B Db=3B Ab=4B Eb=5B Bb=6B F=7B
# 小调：A=8A E=9A B=10A F#=11A C#=12A G#=1A D#=2A A#=3A F=4A C=5A G=6A D=7A
CAMELOT_MAJOR = [
    "8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B",
]
CAMELOT_MINOR = [
    "5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A",
]

ProgressCallback = Callable[[float, str], None]


class AudioLoadError(RuntimeError):
    """音频无法读取时抛出；message 面向用户且给出可操作的下一步。"""


# ── 音频读取 ─────────────────────────────────────────────

_PCM_DTYPES = {1: np.uint8, 2: np.int16, 4: np.int32}


def _read_wav_window(
    path: Path, start_sec: float, max_seconds: Optional[float]
) -> Tuple[np.ndarray, int, float]:
    """用标准库 ``wave`` 直读 WAV（只依赖 numpy + 标准库，不需要 ffmpeg）。

    返回 ``(mono_float32, samplerate, full_duration_sec)``；只读取需要的窗口。
    """
    with wave.open(str(path), "rb") as wf:
        n_channels = wf.getnchannels()
        sampwidth = wf.getsampwidth()
        framerate = wf.getframerate()
        n_frames = wf.getnframes()
        comptype = wf.getcomptype()
        duration = n_frames / float(framerate) if framerate else 0.0

        if comptype != "NONE":
            raise AudioLoadError(
                f"WAV 使用压缩编码（{comptype}），请先转换为未压缩 PCM WAV。"
            )
        if framerate <= 0 or n_frames <= 0:
            raise AudioLoadError("WAV 文件没有可读的音频帧。")

        start = int(max(0.0, start_sec) * framerate)
        start = max(0, min(start, max(0, n_frames - 1)))
        count = n_frames - start
        if max_seconds:
            count = min(count, int(max_seconds * framerate))
        if count <= 0:
            raise AudioLoadError("WAV 文件指定的分析窗口为空。")

        wf.setpos(start)
        raw = wf.readframes(count)

    if sampwidth == 3:
        # 24-bit little-endian PCM：numpy 无原生类型，手工展开为 int32
        b = np.frombuffer(raw, dtype=np.uint8)
        b = b[: (b.shape[0] // 3) * 3].reshape(-1, 3).astype(np.int32)
        val = b[:, 0] | (b[:, 1] << 8) | (b[:, 2] << 16)
        val = np.where(val >= (1 << 23), val - (1 << 24), val)
        data = (val / float(1 << 23)).astype(np.float32)
    else:
        dtype = _PCM_DTYPES.get(sampwidth)
        if dtype is None:
            raise AudioLoadError(
                f"不支持的 WAV 位深：{sampwidth * 8} bit（仅支持 8/16/24/32 bit PCM）。"
            )
        data = np.frombuffer(raw, dtype=dtype).astype(np.float32)
        if sampwidth == 1:
            # 8-bit WAV 是无符号的
            data = (data - 128.0) / 128.0
        else:
            data = data / float(1 << (8 * sampwidth - 1))

    if n_channels > 1:
        usable = (data.shape[0] // n_channels) * n_channels
        data = data[:usable].reshape(-1, n_channels).mean(axis=1)

    return np.ascontiguousarray(data, dtype=np.float32), framerate, duration


def _read_demucs_window(
    path: Path, start_sec: float, max_seconds: Optional[float], target_sr: int
) -> Tuple[np.ndarray, int, float]:
    """用 ``demucs.audio.AudioFile``（ffmpeg/ffprobe 后端）解码任意格式。"""
    from demucs.audio import AudioFile  # 延迟导入：本模块不硬依赖 torch

    af = AudioFile(path)
    duration = float(af.duration)
    # streams=0 → 单流，返回 (channels, samples)，并在 ffmpeg 侧完成重采样
    wav = af.read(
        seek_time=start_sec if start_sec > 0 else None,
        duration=max_seconds,
        streams=0,
        samplerate=target_sr,
        channels=1,
    )
    arr = wav.detach().cpu().numpy() if hasattr(wav, "detach") else np.asarray(wav)
    while arr.ndim > 1:
        arr = arr[0]
    return np.ascontiguousarray(arr, dtype=np.float32), target_sr, duration


def _resample(x: np.ndarray, src_sr: int, dst_sr: int) -> np.ndarray:
    """整数倍时用 box 平均（天然抗混叠），否则退化为线性插值。"""
    if src_sr == dst_sr or x.size == 0:
        return x
    if src_sr > dst_sr and src_sr % dst_sr == 0:
        factor = src_sr // dst_sr
        n = (x.shape[0] // factor) * factor
        if n:
            return x[:n].reshape(-1, factor).mean(axis=1).astype(np.float32)
    n_out = max(1, int(round(x.shape[0] * dst_sr / float(src_sr))))
    dst_t = np.linspace(0.0, x.shape[0] - 1, n_out)
    return np.interp(dst_t, np.arange(x.shape[0], dtype=np.float64), x).astype(
        np.float32
    )


def _probe_duration(path: Path) -> Optional[float]:
    """尽力探测完整时长（不受"只取中段"影响），失败返回 None。"""
    try:
        from demucs.audio import AudioFile

        return float(AudioFile(path).duration)
    except Exception:
        pass
    if path.suffix.lower() == ".wav":
        try:
            with wave.open(str(path), "rb") as wf:
                rate = wf.getframerate()
                return wf.getnframes() / float(rate) if rate else None
        except Exception:
            pass
    return None


def load_audio_mono(
    audio_path: str,
    target_sr: int = ANALYSIS_SR,
    max_seconds: Optional[float] = MAX_ANALYSIS_SECONDS,
) -> Tuple[np.ndarray, int, float]:
    """读入音频为单声道 float32，只取**中段**最多 ``max_seconds`` 秒。

    返回 ``(mono, samplerate, duration_sec)``。``duration_sec`` 是整首曲子的
    时长（不是被截取窗口的长度）；探测不到时退化为窗口长度。

    解码优先级：WAV 直读（无外部依赖）→ demucs/ffmpeg（支持 mp3/flac/m4a…）。
    两者都不可用时抛出带修复建议的 ``AudioLoadError``。
    """
    path = Path(audio_path)
    if not path.is_file():
        raise AudioLoadError(f"文件不存在：{audio_path}")

    duration = _probe_duration(path)
    start_sec = 0.0
    if duration and max_seconds and duration > max_seconds:
        start_sec = (duration - max_seconds) / 2.0

    errors = []

    if path.suffix.lower() in (".wav", ".wave"):
        try:
            mono, sr, wav_duration = _read_wav_window(path, start_sec, max_seconds)
            return _resample(mono, sr, target_sr), target_sr, (
                duration or wav_duration or len(mono) / float(sr or 1)
            )
        except AudioLoadError as exc:
            raise
        except Exception as exc:  # wave 只支持 PCM；浮点 WAV 等落到 demucs
            errors.append(f"WAV 直读失败（{type(exc).__name__}: {exc}）")

    try:
        mono, sr, demucs_duration = _read_demucs_window(
            path, start_sec, max_seconds, target_sr
        )
        return mono, sr, (duration or demucs_duration or 0.0)
    except Exception as exc:
        errors.append(f"demucs/ffmpeg 解码失败（{type(exc).__name__}: {exc}）")

    raise AudioLoadError(
        "无法读取音频 "
        f"「{path.name}」：{'; '.join(errors)}\n"
        "可尝试的修复：① 安装 ffmpeg（macOS: `brew install ffmpeg`），"
        "它也是 Demucs 解码 mp3/flac/m4a 的必需组件；"
        "② 把音频导出为未压缩 PCM WAV 后重试。"
    )


# ── 频谱工具 ─────────────────────────────────────────────


def _stft_magnitude(
    x: np.ndarray, n_fft: int, hop: int, block: int = 256
) -> np.ndarray:
    """分块计算 STFT 幅度谱，避免长音频一次性持有整段复数矩阵。"""
    if x.shape[0] < n_fft:
        x = np.pad(x, (0, n_fft - x.shape[0]))
    n_frames = 1 + (x.shape[0] - n_fft) // hop
    window = np.hanning(n_fft).astype(np.float32)
    out = np.empty((n_frames, n_fft // 2 + 1), dtype=np.float32)
    base = np.arange(n_fft)[None, :]
    for start in range(0, n_frames, block):
        stop = min(start + block, n_frames)
        idx = base + hop * np.arange(start, stop)[:, None]
        out[start:stop] = np.abs(np.fft.rfft(x[idx] * window, axis=1))
    return out


def _chroma_matrix(mag: np.ndarray, freqs: np.ndarray) -> np.ndarray:
    """(frames, bins) 幅度谱 → (frames, 12) chroma，按小数音分双线性摊分。"""
    mask = (freqs >= CHROMA_FMIN) & (freqs <= CHROMA_FMAX)
    if not mask.any():
        return np.zeros((mag.shape[0], 12), dtype=np.float64)

    f = freqs[mask].astype(np.float64)
    m = mag[:, mask].astype(np.float64)

    midi = 69.0 + 12.0 * np.log2(f / 440.0)
    nearest = np.rint(midi).astype(np.int64)
    frac = midi - nearest
    lo = np.mod(nearest, 12)
    hi = np.mod(lo + np.where(frac >= 0.0, 1, -1), 12)
    w_hi = np.abs(frac)
    w_lo = 1.0 - w_hi

    n_frames, n_bins = m.shape
    chroma = np.zeros((n_frames, 12), dtype=np.float64)
    frame_idx = np.broadcast_to(np.arange(n_frames)[:, None], (n_frames, n_bins))
    lo_idx = np.broadcast_to(lo[None, :], (n_frames, n_bins))
    hi_idx = np.broadcast_to(hi[None, :], (n_frames, n_bins))
    np.add.at(chroma, (frame_idx, lo_idx), m * w_lo[None, :])
    np.add.at(chroma, (frame_idx, hi_idx), m * w_hi[None, :])
    return chroma


def chroma_vector(mono: np.ndarray, sr: int) -> np.ndarray:
    """音频 → 12 维 chroma（L1 归一化，和为 1；静音返回全 0）。"""
    mag = _stft_magnitude(mono, CHROMA_N_FFT, CHROMA_HOP)
    if mag.shape[0] == 0:
        return np.zeros(12, dtype=np.float64)
    freqs = np.fft.rfftfreq(CHROMA_N_FFT, 1.0 / sr)
    chroma = _chroma_matrix(mag, freqs)

    energy = chroma.sum(axis=1)
    peak = float(energy.max()) if energy.size else 0.0
    if peak <= 0.0:
        return np.zeros(12, dtype=np.float64)
    # 丢掉近似静音的帧：它们的 chroma 是噪声，会把统计拉平
    chroma = chroma[energy > SILENT_FRAME_RATIO * peak]
    if chroma.shape[0] == 0:
        return np.zeros(12, dtype=np.float64)

    # 逐帧 L2 归一化：让每个时间片对最终 chroma 等权，避免高潮段落主导
    norm = np.linalg.norm(chroma, axis=1, keepdims=True)
    chroma = chroma / np.maximum(norm, 1e-12)
    vec = chroma.sum(axis=0)
    total = vec.sum()
    return vec / total if total > 0 else vec


def onset_envelope(mono: np.ndarray, sr: int) -> Optional[np.ndarray]:
    """谱通量 → onset 包络（去均值、单位方差）；无法估计时返回 None。"""
    mag = _stft_magnitude(mono, ONSET_N_FFT, ONSET_HOP)
    if mag.shape[0] < 4:
        return None
    flux = np.maximum(0.0, np.diff(mag, axis=0)).sum(axis=1)
    flux = flux - flux.mean()
    std = float(flux.std())
    if std < 1e-12:
        return None
    return (flux / std).astype(np.float64)


# ── 调性 ─────────────────────────────────────────────────


def key_from_chroma(chroma: np.ndarray) -> dict:
    """12 维 chroma → Krumhansl-Schmuckler 最大相关调性。"""
    chroma = np.asarray(chroma, dtype=np.float64).reshape(-1)
    if chroma.shape[0] != 12 or not np.isfinite(chroma).all() or chroma.sum() <= 0:
        return {"key": "C", "mode": "major", "camelot": "8B", "confidence": 0.0}

    c = chroma - chroma.mean()
    c_norm = float(np.linalg.norm(c))

    scores = np.empty(24, dtype=np.float64)
    for tonic in range(12):
        for m_idx, profile in enumerate((KS_MAJOR, KS_MINOR)):
            p = np.roll(profile, tonic)
            p = p - p.mean()
            denom = c_norm * float(np.linalg.norm(p))
            scores[m_idx * 12 + tonic] = (
                float(np.dot(c, p)) / denom if denom > 1e-12 else 0.0
            )

    best = int(np.argmax(scores))
    mode = "major" if best < 12 else "minor"
    tonic = best % 12

    # 置信度：对 24 个相关系数做 softmax（温度 0.02），取最优调的后验概率，
    # 线性映射到 [0,1]（1/24 表示对 24 个调完全没有偏好）。
    z = (scores - scores.max()) / KEY_SOFTMAX_TEMPERATURE
    p = np.exp(z)
    p /= p.sum()
    conf = (float(p.max()) - 1.0 / 24.0) / (1.0 - 1.0 / 24.0)

    return {
        "key": PITCH_NAMES[tonic],
        "mode": mode,
        "camelot": (CAMELOT_MAJOR if mode == "major" else CAMELOT_MINOR)[tonic],
        "confidence": round(float(np.clip(conf, 0.0, 1.0)), 4),
    }


# ── 速度 ─────────────────────────────────────────────────


def _autocorrelation(x: np.ndarray) -> np.ndarray:
    """Wiener-Khinchin 自相关；归一化到 ac[0] = 1（对去均值信号即相关系数）。"""
    n = x.shape[0]
    size = 1
    while size < 2 * n:
        size <<= 1
    spec = np.fft.rfft(x, size)
    ac = np.fft.irfft(spec * np.conj(spec), size)[:n]
    if ac[0] > 1e-12:
        ac = ac / ac[0]
    return ac


def tempo_from_onset(env: Optional[np.ndarray], fps: float) -> dict:
    """onset 包络 → BPM（自相关 + 对数正态先验 + 抛物线插值 + 八度修正）。"""
    if env is None or env.shape[0] < 8 or fps <= 0:
        return {"bpm": 0.0, "confidence": 0.0}

    ac = _autocorrelation(env)

    lag_min = max(1, int(math.floor(60.0 * fps / TEMPO_SEARCH_MAX)))
    lag_max = min(ac.shape[0] - 1, int(math.ceil(60.0 * fps / TEMPO_SEARCH_MIN)))
    if lag_max <= lag_min:
        return {"bpm": 0.0, "confidence": 0.0}

    lags = np.arange(lag_min, lag_max + 1, dtype=np.float64)
    bpms = 60.0 * fps / lags
    # 对数正态节拍先验：中心 120 BPM。没有它，自相关在 lag 与 2·lag 上几乎
    # 一样高，60/120 这类八度歧义会随机翻车。
    prior = np.exp(
        -0.5 * (np.log2(bpms / TEMPO_PRIOR_CENTER) / TEMPO_PRIOR_WIDTH) ** 2
    )
    score = ac[lags.astype(np.int64)] * prior
    peak = int(np.argmax(score))
    lag = lags[peak]

    # 抛物线插值细化峰值位置：整数 lag 下 120 BPM 对应 43.07 帧，
    # 不插值会直接带来 ~1% 的量化误差，且会加剧八度误判。
    lag_ref = float(lag)
    i = int(lag)
    if 0 < i < ac.shape[0] - 1:
        y0, y1, y2 = float(ac[i - 1]), float(ac[i]), float(ac[i + 1])
        denom = y0 - 2.0 * y1 + y2
        if abs(denom) > 1e-12:
            delta = 0.5 * (y0 - y2) / denom
            lag_ref = i + float(np.clip(delta, -0.5, 0.5))

    bpm = 60.0 * fps / lag_ref
    if bpm < TEMPO_OCTAVE_LOW:
        bpm *= 2.0
    elif bpm > TEMPO_OCTAVE_HIGH:
        bpm /= 2.0

    # 置信度 = 所选周期上的自相关系数（未加先验）。它就是"信号与自身平移一个
    # 周期后的相关性"，对干净节拍接近 1，对无节奏素材接近 0，含义明确。
    conf = float(np.clip(ac[i], 0.0, 1.0))

    return {"bpm": round(float(bpm), 2), "confidence": round(conf, 4)}


# ── 对外 API ─────────────────────────────────────────────


def _decode(
    audio_path: str, progress: Optional[ProgressCallback] = None
) -> Tuple[np.ndarray, int, float]:
    if progress:
        progress(0.05, "reading_audio")
    return load_audio_mono(audio_path)


def detect_key(audio_path: str) -> dict:
    """检测调性 → ``{"key","mode","camelot","confidence"}``。"""
    mono, sr, _ = _decode(audio_path)
    return key_from_chroma(chroma_vector(mono, sr))


def detect_tempo(audio_path: str) -> dict:
    """检测速度 → ``{"bpm","confidence"}``。"""
    mono, sr, _ = _decode(audio_path)
    return tempo_from_onset(onset_envelope(mono, sr), sr / float(ONSET_HOP))


def analyze(
    audio_path: str, progress: Optional[ProgressCallback] = None
) -> dict:
    """调性 + 速度 + 时长 → ``{"key","tempo","duration_sec"}``。

    只解码一次，两个检测共用同一段中频音频（``progress`` 为可选进度回调，
    签名 ``(0~1, stage_key)``，服务端用它推进任务进度）。
    """
    mono, sr, duration = _decode(audio_path, progress)
    if progress:
        progress(0.4, "analyzing_key")
    key = key_from_chroma(chroma_vector(mono, sr))
    if progress:
        progress(0.75, "analyzing_tempo")
    tempo = tempo_from_onset(onset_envelope(mono, sr), sr / float(ONSET_HOP))
    if progress:
        progress(1.0, "done")
    return {"key": key, "tempo": tempo, "duration_sec": round(float(duration), 3)}


def main(argv: list[str]) -> int:
    """命令行入口：``python3 analysis.py <audio>``，输出 JSON。"""
    if len(argv) != 2:
        print("usage: python3 analysis.py <audio-file>", file=sys.stderr)
        return 2
    try:
        print(json.dumps(analyze(argv[1]), ensure_ascii=False, indent=2))
    except AudioLoadError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
