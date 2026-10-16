"""``analysis.py`` 的自测脚本。

只依赖 **numpy**（不需要 demucs / torch / ffmpeg）：用 numpy 合成已知调性与
速度的音频，写成 WAV 后走完整的 ``detect_key`` / ``detect_tempo`` / ``analyze``
公开接口，断言检测结果。

运行::

    python3 python-backend/tests/test_analysis.py

退出码 0 = 全部通过，1 = 有失败项。
"""

from __future__ import annotations

import sys
import tempfile
import traceback
import wave
from pathlib import Path

import numpy as np

# 让脚本在任意工作目录下都能 import 到 python-backend/analysis.py
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import analysis  # noqa: E402  （路径注入后才能导入）

SR = 22050
CLICK_BPM = 120.0


# ── 合成工具 ─────────────────────────────────────────────


def write_wav(path: Path, data: np.ndarray, sr: int = SR) -> None:
    """把 float 波形写成 16-bit PCM WAV（与 separator.py 的输出格式一致）。"""
    peak = float(np.abs(data).max()) if data.size else 0.0
    if peak > 0:
        data = data / peak * 0.9
    pcm = (np.clip(data, -1.0, 1.0) * 32767.0).round().astype("<i2")
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sr)
        wf.writeframes(pcm.tobytes())


def make_click_track(bpm: float = CLICK_BPM, seconds: float = 12.0, sr: int = SR) -> np.ndarray:
    """等间隔的短促打击音（宽频瞬态，谱通量峰非常干净）。"""
    n = int(seconds * sr)
    x = np.zeros(n, dtype=np.float64)
    click_len = int(0.03 * sr)
    t = np.arange(click_len) / sr
    rng = np.random.default_rng(20240516)
    click = rng.standard_normal(click_len) * np.exp(-t * 60.0)
    click += np.sin(2.0 * np.pi * 1000.0 * t) * np.exp(-t * 60.0)

    period = 60.0 / bpm
    pos = 0.0
    while pos < seconds:
        i = int(pos * sr)
        if i >= n:
            break
        seg = click[: min(click_len, n - i)]
        x[i : i + seg.shape[0]] += seg
        pos += period
    return x


def midi_to_freq(midi: int) -> float:
    return 440.0 * 2.0 ** ((midi - 69) / 12.0)


def make_arpeggio(
    midi_notes: list[int],
    seconds: float = 8.0,
    note_seconds: float = 0.5,
    sr: int = SR,
    harmonics: tuple[float, ...] = (1.0, 0.5, 0.25),
) -> np.ndarray:
    """把给定音高循环成琶音；带少量泛音，比纯正弦更接近真实乐器。"""
    n = int(seconds * sr)
    x = np.zeros(n, dtype=np.float64)
    pos = 0.0
    k = 0
    while pos < seconds:
        f = midi_to_freq(midi_notes[k % len(midi_notes)])
        length = min(int(note_seconds * sr), n - int(pos * sr))
        if length <= 0:
            break
        t = np.arange(length) / sr
        env = np.minimum(1.0, t / 0.01) * np.exp(-t * 1.2)
        tone = np.zeros(length, dtype=np.float64)
        for h, amp in enumerate(harmonics, start=1):
            tone += amp * np.sin(2.0 * np.pi * f * h * t)
        i = int(pos * sr)
        x[i : i + length] += tone * env
        pos += note_seconds
        k += 1
    return x


def make_silence(seconds: float = 3.0, sr: int = SR) -> np.ndarray:
    return np.zeros(int(seconds * sr), dtype=np.float64)


# ── 极简断言框架 ─────────────────────────────────────────

_RESULTS: list[tuple[bool, str]] = []


def check(ok: bool, label: str, detail: str = "") -> None:
    _RESULTS.append((bool(ok), label))
    print(f"  [{'PASS' if ok else 'FAIL'}] {label}" + (f" — {detail}" if detail else ""))


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="omuse-analysis-test-"))
    print(f"temp dir: {tmp}")
    print(f"numpy {np.__version__} / python {sys.version.split()[0]}")

    # ── 1. 120 BPM 点击音轨 → BPM 误差 < 5% ──────────────
    print("\n[1] click track @ 120 BPM")
    p = tmp / "click120.wav"
    write_wav(p, make_click_track(CLICK_BPM))
    tempo = analysis.detect_tempo(str(p))
    err = abs(tempo["bpm"] - CLICK_BPM) / CLICK_BPM
    print(f"  detect_tempo -> {tempo}")
    check(err < 0.05, f"BPM within 5% of {CLICK_BPM:.0f}", f"got {tempo['bpm']}, err {err * 100:.2f}%")
    check(0.0 <= tempo["confidence"] <= 1.0, "BPM confidence in [0,1]", f"{tempo['confidence']}")

    # ── 2. C 大调琶音（C-E-G-C）→ C major ────────────────
    print("\n[2] C major arpeggio (C4-E4-G4-C5)")
    p = tmp / "c_major.wav"
    write_wav(p, make_arpeggio([60, 64, 67, 72]))
    key = analysis.detect_key(str(p))
    print(f"  detect_key -> {key}")
    check(key["key"] == "C" and key["mode"] == "major", "detected C major", f"got {key['key']} {key['mode']}")
    check(key["camelot"] == "8B", "Camelot 8B for C major", f"got {key['camelot']}")
    check(0.0 <= key["confidence"] <= 1.0, "key confidence in [0,1]", f"{key['confidence']}")

    # ── 3. 其它 BPM ──────────────────────────────────────
    print("\n[3] click tracks @ 90 / 140 BPM")
    for bpm in (90.0, 140.0):
        p = tmp / f"click{int(bpm)}.wav"
        write_wav(p, make_click_track(bpm))
        got = analysis.detect_tempo(str(p))
        err = abs(got["bpm"] - bpm) / bpm
        check(err < 0.05, f"BPM within 5% of {bpm:.0f}", f"got {got['bpm']}, err {err * 100:.2f}%")

    # ── 4. 其它调性 ──────────────────────────────────────
    print("\n[4] other keys")
    for label, notes, want_key, want_mode, want_camelot in (
        ("G major (G3-B3-D4-G4)", [55, 59, 62, 67], "G", "major", "9B"),
        ("A minor (A3-C4-E4-A4)", [57, 60, 64, 69], "A", "minor", "8A"),
        ("E minor (E3-G3-B3-E4)", [52, 55, 59, 64], "E", "minor", "9A"),
    ):
        p = tmp / f"key_{want_key}{want_mode}.wav"
        write_wav(p, make_arpeggio(notes))
        got = analysis.detect_key(str(p))
        check(
            got["key"] == want_key and got["mode"] == want_mode,
            f"{label} -> {want_key} {want_mode}",
            f"got {got['key']} {got['mode']} (camelot {got['camelot']}, conf {got['confidence']})",
        )
        check(got["camelot"] == want_camelot, f"{label} -> Camelot {want_camelot}", f"got {got['camelot']}")

    # ── 5. analyze() 端到端（含完整时长） ─────────────────
    print("\n[5] analyze() end-to-end")
    p = tmp / "full.wav"
    write_wav(p, make_arpeggio([60, 64, 67, 72], seconds=9.0))
    res = analysis.analyze(str(p))
    print(f"  analyze -> {res}")
    check(set(res) == {"key", "tempo", "duration_sec"}, "analyze returns key/tempo/duration_sec", str(sorted(res)))
    check(abs(res["duration_sec"] - 9.0) < 0.1, "duration_sec ~ 9.0s", f"got {res['duration_sec']}")
    check(res["key"]["key"] == "C" and res["key"]["mode"] == "major", "analyze key = C major", str(res["key"]))

    # ── 6. 静音不崩溃 ────────────────────────────────────
    print("\n[6] silence / degenerate input")
    p = tmp / "silence.wav"
    write_wav(p, make_silence())
    silent_key = analysis.detect_key(str(p))
    silent_tempo = analysis.detect_tempo(str(p))
    check(isinstance(silent_key.get("confidence"), float), "silence key returns dict", str(silent_key))
    check(silent_tempo["confidence"] == 0.0, "silence tempo confidence == 0", str(silent_tempo))
    try:
        analysis.detect_key(str(tmp / "missing.wav"))
        check(False, "missing file raises AudioLoadError")
    except analysis.AudioLoadError as exc:
        check(True, "missing file raises AudioLoadError", str(exc).splitlines()[0])

    # ── 7. 长音频只取中段 60 秒 ──────────────────────────
    print("\n[7] long file analysed on the middle 60s window")
    long_path = tmp / "long.wav"
    write_wav(long_path, make_click_track(CLICK_BPM, seconds=8.0))
    mono, sr, duration = analysis.load_audio_mono(str(long_path))
    check(sr == analysis.ANALYSIS_SR, f"resampled to {analysis.ANALYSIS_SR} Hz", f"got {sr}")
    check(abs(duration - 8.0) < 0.1, "duration reported from file header", f"got {duration}")
    check(mono.ndim == 1, "mono output", f"shape {mono.shape}")

    # 真正超过 60 秒的素材：时长报完整值，但只分析中段
    long60 = tmp / "long75.wav"
    write_wav(long60, make_click_track(CLICK_BPM, seconds=75.0))
    mono60, sr60, dur60 = analysis.load_audio_mono(str(long60))
    check(abs(dur60 - 75.0) < 0.2, "75s file reports full duration", f"got {dur60:.2f}")
    check(
        mono60.shape[0] <= int(analysis.MAX_ANALYSIS_SECONDS * sr60) + 1,
        f"only {analysis.MAX_ANALYSIS_SECONDS:.0f}s window decoded",
        f"{mono60.shape[0] / sr60:.1f}s decoded",
    )
    res60 = analysis.analyze(str(long60))
    check(
        abs(res60["tempo"]["bpm"] - CLICK_BPM) / CLICK_BPM < 0.05
        and abs(res60["duration_sec"] - 75.0) < 0.2,
        "75s file: BPM still 120, duration 75s",
        f"bpm {res60['tempo']['bpm']}, duration {res60['duration_sec']}",
    )

    failed = [label for ok, label in _RESULTS if not ok]
    print(f"\n=== {len(_RESULTS) - len(failed)}/{len(_RESULTS)} checks passed ===")
    if failed:
        for label in failed:
            print(f"  FAILED: {label}")
        return 1
    print("ALL PASS")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception:  # noqa: BLE001 - 自测脚本：把 traceback 打全再退出 1
        traceback.print_exc()
        raise SystemExit(1)
