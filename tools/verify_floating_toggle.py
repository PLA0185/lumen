"""验收：顶栏的「悬浮窗」开关。

用户反馈"悬浮窗在哪开启，我都找不到"——原来的入口只有
「设置 → 窗口 → 显示悬浮窗」和托盘菜单，都不在主界面上。
这里验证新的顶栏一键开关：真实鼠标点击 → 窗口真的出现/消失 → 按钮状态跟着变。
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(__import__("os").environ.get("LUMEN_CDP_PORT", "9222"))
SEL = ".topbar__actions button[title*='悬浮']"


def state(m: ui.Target) -> dict:
    raw = m.eval(
        "(async () => JSON.stringify(await window.__TAURI_INTERNALS__.invoke('window_floating_state')))()"
    )
    return json.loads(raw) if raw else {}


def main() -> int:
    m = ui.connect(PORT, want="main", timeout=60)
    ok = True

    if not m.eval(f"!!document.querySelector(\"{SEL}\")"):
        print("❌ 顶栏没有找到悬浮窗开关")
        m.close()
        return 1
    label = m.eval(f"document.querySelector(\"{SEL}\").innerText")
    print(f"✅ 顶栏存在悬浮窗开关，文案：{label!r}", flush=True)

    before = state(m)
    print(f"   初始：enabled={before.get('enabled')} visible={before.get('visible')}", flush=True)

    # 先确保是关闭状态，再点开
    if before.get("enabled"):
        ui.real_click(m, SEL)
        time.sleep(1.5)
    closed = state(m)
    print(f"   关闭后：enabled={closed.get('enabled')}", flush=True)

    ui.real_click(m, SEL)
    time.sleep(2.0)
    opened = state(m)
    pressed = m.eval(f"document.querySelector(\"{SEL}\").getAttribute('aria-pressed')")
    print(
        f"✅ 真实鼠标点击后：enabled={opened.get('enabled')} visible={opened.get('visible')}"
        f" aria-pressed={pressed}",
        flush=True,
    )
    ok = ok and opened.get("enabled") is True and opened.get("visible") is True and pressed == "true"

    # 悬浮窗真的能被连上（说明窗口确实创建出来了）
    try:
        f = ui.connect(PORT, want="floating", timeout=20)
        print("✅ 悬浮窗窗口确实存在，可以连接并渲染", flush=True)
        f.close()
    except ui.CdpError as e:
        print(f"❌ 连不上悬浮窗：{e}")
        ok = False

    # 再点一次应能隐藏
    ui.real_click(m, SEL)
    time.sleep(1.5)
    hidden = state(m)
    print(f"✅ 再点一次：enabled={hidden.get('enabled')}", flush=True)
    ok = ok and hidden.get("enabled") is False

    toast = m.eval("Array.from(document.querySelectorAll('.toast')).map(e => e.innerText)")
    print(f"   界面提示：{toast}", flush=True)

    m.close()
    print("\n" + ("全部通过" if ok else "存在未通过项"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
