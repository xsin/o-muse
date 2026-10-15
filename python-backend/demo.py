"""Demo 模式分轨器（Task 2）。

当 Demucs 不可用时启用：为每个 stem 生成一段静音 WAV，
便于前端与 Rust 端联调，绝不复制任何第三方实现。
"""

import wave
from pathlib import Path

import constants


class DemoSeparator:
    """生成占位静音 WAV，明确标记为 demo。"""

    is_demo = True
    name = "demo"

    def separate(self, input_path: str, output_dir: str) -> dict:
        out = Path(output_dir)
        out.mkdir(parents=True, exist_ok=True)
        sample_rate = 44100
        duration_seconds = 1
        n_frames = sample_rate * duration_seconds
        stems = []
        for name in constants.DEFAULT_STEM_NAMES:
            path = out / f"{name}.wav"
            with wave.open(str(path), "wb") as wf:
                wf.setnchannels(1)
                wf.setsampwidth(2)
                wf.setframerate(sample_rate)
                wf.writeframes(b"\x00\x00" * n_frames)
            stems.append(name)
        return {
            "output_dir": str(out),
            "stems": stems,
            "model": self.name,
        }
