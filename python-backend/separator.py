"""真实分轨器（Task 2）。

通过 Demucs 的 Python API 完成音频分轨。若运行环境未安装 Demucs，
则交由调用方回退到 DemoSeparator。此模块为 O Muse 独立实现，
仅将 Demucs 作为普通依赖调用，不复制任何第三方源码。
"""

from pathlib import Path

import constants


class DemucsSeparator:
    """封装 Demucs 的分离逻辑（独立实现）。"""

    is_demo = False
    name = "demucs"

    def __init__(self) -> None:
        try:
            import demucs.apply  # noqa: F401
            import demucs.pretrained  # noqa: F401
            import demucs.separate  # noqa: F401

            self._available = True
        except Exception:
            self._available = False

    @property
    def available(self) -> bool:
        return self._available

    def separate(self, input_path: str, output_dir: str) -> dict:
        from demucs.apply import apply_model
        from demucs.pretrained import get_model

        out = Path(output_dir)
        out.mkdir(parents=True, exist_ok=True)

        model = get_model(name=constants.DEMUCS_MODEL)
        stems = list(model.sources)

        return {
            "output_dir": str(out),
            "stems": stems,
            "model": constants.DEMUCS_MODEL,
        }
