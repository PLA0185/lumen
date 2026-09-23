"""自动更新的实机验收：对着**真实的 GitHub Release 端点**检查更新。

验证链路：设置 → 关于 → 软件更新 → 检查更新 → 拉取
`https://github.com/PLA0185/lumen/releases/latest/download/latest.json`
→ 解析 → 比对版本号 → 界面给出结论。

因为当前安装的就是最新版，预期结果是「已是最新版本」——这恰好证明
"能拿到清单并且版本号比对正确"。要验证"发现新版本"，把端点换成
更高版本的清单即可（`tools/serve_update.py` 提供本地假清单，
调试构建允许 http 端点）。
"""

from __future__ import annotations

import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = 9222


def main() -> int:
    m = ui.connect(PORT, want="main", timeout=60)

    # 打开设置 → 关于
    m.eval(
        """
        (() => {
          const btn = Array.from(document.querySelectorAll('.sidebar button'))
            .find(b => b.innerText.includes('设置'));
          if (btn) btn.click();
          return true;
        })()
        """
    )
    time.sleep(1.0)
    m.eval(
        """
        (() => {
          const tab = Array.from(document.querySelectorAll('.tab, .tabs button'))
            .find(b => b.innerText.trim().startsWith('关于'));
          if (tab) tab.click();
          return true;
        })()
        """
    )
    time.sleep(1.2)

    panel = m.eval(
        """
        (() => {
          const g = Array.from(document.querySelectorAll('.setgroup'))
            .find(x => x.innerText.includes('软件更新'));
          return g ? g.innerText : null;
        })()
        """
    )
    print("软件更新面板：", (panel or "未找到").replace("\n", " | "), flush=True)
    if not panel:
        print("❌ 找不到「软件更新」面板")
        m.close()
        return 1

    clicked = m.eval(
        """
        (() => {
          const g = Array.from(document.querySelectorAll('.setgroup'))
            .find(x => x.innerText.includes('软件更新'));
          const btn = Array.from(g.querySelectorAll('button'))
            .find(b => b.innerText.includes('检查更新'));
          if (!btn) return false;
          btn.click();
          return true;
        })()
        """
    )
    if not clicked:
        print("❌ 找不到「检查更新」按钮")
        m.close()
        return 1
    print("已点击「检查更新」，等待结果…", flush=True)

    result = None
    for _ in range(40):
        time.sleep(1.0)
        text = m.eval(
            """
            (() => {
              const g = Array.from(document.querySelectorAll('.setgroup'))
                .find(x => x.innerText.includes('软件更新'));
              return g ? g.innerText : '';
            })()
            """
        ) or ""
        if "已是最新版本" in text:
            result = ("latest", text)
            break
        if "发现新版本" in text:
            result = ("available", text)
            break
        m = m
        if re.search(r"无法连接|失败|错误|签名|拒绝", text) and "检查中" not in text:
            result = ("failed", text)
            break

    m.close()
    if result is None:
        print("❌ 超时：没有拿到检查结果")
        return 1

    kind, text = result
    flat = text.replace("\n", " | ")
    if kind == "latest":
        print(f"✅ 对着真实端点检查更新成功，界面显示「已是最新版本」", flush=True)
        print(f"   {flat[:200]}", flush=True)
        return 0
    if kind == "available":
        print(f"✅ 端点返回了更高版本，界面显示「发现新版本」", flush=True)
        print(f"   {flat[:200]}", flush=True)
        return 0

    print(f"❌ 检查更新失败：{flat[:300]}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
