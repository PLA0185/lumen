"""自动更新的**完整跨版本升级**验收（此前唯一没验到的一环）。

流程：
1. 启动**已安装的旧版**（0.2.0），打开「设置 → 关于 → 软件更新」；
2. 点「检查更新」→ 期望界面显示「发现新版本 0.2.1」（真实 GitHub 端点）；
3. 点「下载并安装」→ 下载 → **minisign 签名校验** → 静默运行 NSIS 安装器；
4. 应用退出后，脚本检查安装目录里的可执行文件版本是否已变成 0.2.1，
   并重新启动它确认新界面（新图标）真的生效。

这个脚本会**真的把本机的 Lumen 升级掉**，属于破坏性验收，请在有旧版可升时运行。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))
EXE = Path(os.environ["LOCALAPPDATA"]) / "Lumen" / "lumen.exe"
EXPECT_NEW = os.environ.get("LUMEN_EXPECT_NEW", "0.2.1")


def installed_version() -> str:
    """从文件版本资源读已安装程序的版本号"""
    ps = (
        "$v = (Get-Item '%s').VersionInfo; "
        '"$($v.FileMajorPart).$($v.FileMinorPart).$($v.FileBuildPart)"' % EXE
    )
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True
    )
    return out.stdout.strip()


def open_update_panel(m: ui.Target) -> None:
    m.eval(
        """
        (() => {
          const s = Array.from(document.querySelectorAll('.sidebar button'))
            .find(b => b.innerText.includes('设置'));
          if (s) s.click();
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
    time.sleep(1.0)


def panel_text(m: ui.Target) -> str:
    return (
        m.eval(
            """
        (() => {
          const g = Array.from(document.querySelectorAll('.setgroup'))
            .find(x => x.innerText.includes('软件更新'));
          return g ? g.innerText : '';
        })()
        """
        )
        or ""
    )


def main() -> int:
    print(f"升级前已安装版本：{installed_version()}", flush=True)

    m = ui.connect(PORT, want="main", timeout=60)
    open_update_panel(m)

    # 允许再点一次（面板打开时会自动查一次，可能已经拿到结果）
    m.eval(
        """
        (() => {
          const g = Array.from(document.querySelectorAll('.setgroup'))
            .find(x => x.innerText.includes('软件更新'));
          const btn = Array.from(g.querySelectorAll('button'))
            .find(b => b.innerText.includes('检查更新'));
          if (btn) btn.click();
          return true;
        })()
        """
    )

    text = ""
    for _ in range(45):
        time.sleep(1.0)
        text = panel_text(m)
        if "发现新版本" in text or "已是最新版本" in text or "无法" in text:
            break

    flat = text.replace("\n", " | ")
    if "发现新版本" not in text:
        print(f"❌ 没有发现新版本，面板显示：{flat[:240]}")
        m.close()
        return 1
    ver = re.search(r"发现新版本\s*([0-9.]+)", text)
    print(f"✅ 检查更新发现新版本 {ver.group(1) if ver else '?'}", flush=True)

    # 点「下载并安装」
    clicked = m.eval(
        """
        (() => {
          const g = Array.from(document.querySelectorAll('.setgroup'))
            .find(x => x.innerText.includes('软件更新'));
          const btn = Array.from(g.querySelectorAll('button'))
            .find(b => b.innerText.includes('下载并安装'));
          if (!btn) return false;
          btn.click();
          return true;
        })()
        """
    )
    if not clicked:
        print("❌ 找不到「下载并安装」按钮")
        m.close()
        return 1
    print("已触发下载并安装，等待安装器接管……", flush=True)

    # 应用会被安装器结束，调试连接随之断开
    for _ in range(120):
        time.sleep(1.0)
        try:
            if m.eval("1", await_promise=False) != 1:
                break
        except Exception:
            print("   应用进程已退出（安装器接管）", flush=True)
            break

    # 等安装完成：文件版本变成新版本
    after = installed_version()
    for _ in range(60):
        if after == EXPECT_NEW:
            break
        time.sleep(2.0)
        after = installed_version()
    if after != EXPECT_NEW:
        print(f"❌ 升级后版本仍是 {after}（期望 {EXPECT_NEW}）")
        return 1
    print(f"✅ 安装目录里的版本已变成 {after}", flush=True)

    # 重新启动，确认新界面可用
    env = dict(os.environ)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={PORT}"
    subprocess.Popen([str(EXE)], env=env)
    time.sleep(14)
    m2 = ui.connect(PORT, want="main", timeout=60)
    ver_ui = m2.eval(
        "(() => { const e = document.querySelector('.sidebar__version'); return e ? e.innerText : null; })()"
    )
    icons = m2.eval(
        "(() => ({ nav: document.querySelectorAll('.nav-item__icon svg').length,"
        " emoji: /[\\u2600-\\u27BF\\uD83C-\\uDBFF]/.test(document.querySelector('.sidebar').innerText) }))()"
    )
    print(f"✅ 升级后启动成功，侧边栏版本号：{ver_ui}；图标统计：{icons}", flush=True)
    m2.close()

    ok = ver_ui == f"v{EXPECT_NEW}" and icons.get("nav", 0) >= 17 and not icons.get("emoji", True)
    print(("✅" if ok else "❌") + " 新版本界面正确（侧边栏全部为 SVG 图标、无 emoji）", flush=True)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
