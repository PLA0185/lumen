"""截图工具：通过 CDP 抓取窗口真实渲染结果。

用途是**用眼睛确认视觉改动**（例如整套图标是否统一），
而不是靠猜。截图保存到 `docs/screenshots/`。
"""

from __future__ import annotations

import base64
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(__import__("os").environ.get("LUMEN_CDP_PORT", "9222"))
OUT_DIR = Path(__file__).resolve().parent.parent / "docs" / "screenshots"


def shoot(target: ui.Target, name: str, full: bool = False) -> Path:
    res = target.call(
        "Page.captureScreenshot",
        {"format": "png", "captureBeyondViewport": full},
    )
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / name
    path.write_bytes(base64.b64decode(res["data"]))
    return path


def main() -> int:
    which = sys.argv[1] if len(sys.argv) > 1 else "main"
    name = sys.argv[2] if len(sys.argv) > 2 else f"ui-{which}.png"
    t = ui.connect(PORT, want=which, timeout=30)
    p = shoot(t, name)
    print(f"已保存 {p}（{p.stat().st_size} 字节）")
    t.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
