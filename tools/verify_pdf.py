"""PDF 导出的实机验收（任务书 §6「PDF 可读导出」）。

## 为什么必须在真机上跑

导出走的是**应用自身 WebView2 内核的 `PrintToPdf`**——它打印的是"当前页面"。
因此有三件事只有真机能确认：

1. 打印报告确实被渲染出来，而不是把主界面或空白页导出去；
2. 导出后界面能恢复（不会卡在打印视图）；
3. **中文能被正确提取**——这正是放弃纯 Rust PDF 库的原因
   （那些库的内置字体是 WinAnsi 单字节编码，中文会被静默丢弃）。

## 已经踩过的两个坑（脚本里都留了处理）

- **报告必须挂在 `.app` 之外**：导出时用 `body[data-print='on'] .app { display:none }`
  隐藏主界面，报告若在 `.app` 里会一起被隐藏，结果是一张 1 KB 的空白 PDF。
- **保存对话框无法被 JS 拦截**：`__TAURI_INTERNALS__.invoke` 是不可配置属性，
  `Object.defineProperty` 与直接赋值都会失败（已实测：`Cannot redefine property`）。
  所以只能老老实实驱动真实的系统对话框——用 `SendInput` 发一次真实回车
  （`PostMessage`/`BM_CLICK` 对 DirectUI 绘制的按钮无效），
  然后去系统默认目录（文档）里按前缀找刚生成的文件。

用法：python tools/verify_pdf.py
"""

from __future__ import annotations

import ctypes
import os
import sys
import time
from ctypes import wintypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))
OUT = Path(os.environ.get("TEMP", ".")) / "lumen-pdf-verify.pdf"

user32 = ctypes.WinDLL("user32", use_last_error=True)


def find_dialog(title_part: str) -> int:
    """按标题片段找可见的顶层窗口（保存对话框）"""
    hits: list[int] = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, _l):
        if user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(n + 1)
            user32.GetWindowTextW(hwnd, buf, n + 1)
            if title_part in buf.value:
                hits.append(hwnd)
        return True

    user32.EnumWindows(cb, 0)
    return hits[0] if hits else 0


def press_enter() -> None:
    """用 `SendInput` 发一次真实回车（会送到当前前台窗口）。"""
    KEYEVENTF_KEYUP = 0x0002
    VK_RETURN = 0x0D

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [
            ("wVk", wintypes.WORD),
            ("wScan", wintypes.WORD),
            ("dwFlags", wintypes.DWORD),
            ("time", wintypes.DWORD),
            ("dwExtraInfo", ctypes.POINTER(wintypes.ULONG)),
        ]

    class INPUT(ctypes.Structure):
        class _U(ctypes.Union):
            _fields_ = [("ki", KEYBDINPUT)]

        _anonymous_ = ("u",)
        _fields_ = [("type", wintypes.DWORD), ("u", _U)]

    for flags in (0, KEYEVENTF_KEYUP):
        inp = INPUT(type=1, u=INPUT._U(ki=KEYBDINPUT(VK_RETURN, 0, flags, 0, None)))
        user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        time.sleep(0.1)


def documents_dir() -> Path:
    """系统"文档"目录：保存对话框的默认位置（本机被重定向到 D:\\文档）"""
    buf = ctypes.create_unicode_buffer(260)
    # CSIDL_PERSONAL = 5
    if ctypes.windll.shell32.SHGetFolderPathW(None, 5, None, 0, buf) == 0:
        return Path(buf.value)
    return Path(os.path.expanduser("~")) / "Documents"


def force_foreground(hwnd: int) -> bool:
    """把窗口强行提到前台。

    后台进程调用 `SetForegroundWindow` 会被 Windows 拒绝，于是 `SendInput`
    的回车会打到别的窗口上。标准绕法是把当前线程的输入队列**临时附加**到
    前台窗口所属线程，此时调用才被允许。
    """
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    fg = user32.GetForegroundWindow()
    if fg == hwnd:
        return True
    fg_thread = user32.GetWindowThreadProcessId(fg, None)
    cur_thread = kernel32.GetCurrentThreadId()
    attached = False
    if fg_thread and fg_thread != cur_thread:
        attached = bool(user32.AttachThreadInput(cur_thread, fg_thread, True))
    try:
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
        user32.SetFocus(hwnd)
    finally:
        if attached:
            user32.AttachThreadInput(cur_thread, fg_thread, False)
    time.sleep(0.3)
    return user32.GetForegroundWindow() == hwnd


def main() -> int:
    if OUT.exists():
        OUT.unlink()

    m = ui.connect(PORT, want="main", timeout=60)
    titles = (
        m.eval("Array.from(document.querySelectorAll('.task .task__title')).map(e => e.innerText)")
        or []
    )
    print(f"当前列表 {len(titles)} 条：{titles[:4]}", flush=True)
    if not titles:
        print("❌ 列表为空：先在界面上建几条任务再跑这个脚本")
        return 2

    docs = documents_dir()
    before = {p.name: p.stat().st_mtime for p in docs.glob("lumen-tasks-*.pdf")} if docs.exists() else {}
    started = time.time()

    ui.real_click(m, ".topbar__actions button[title^='把当前列表导出为 PDF']")

    dlg = 0
    for _ in range(30):
        dlg = find_dialog("导出 PDF")
        if dlg:
            break
        time.sleep(0.5)
    if not dlg:
        print("❌ 没找到保存对话框")
        m.close()
        return 1
    print("✅ 弹出了系统保存对话框", flush=True)

    front = force_foreground(dlg)
    print(f"   已把对话框提到前台：{front}", flush=True)
    # 对话框是这次点击创建出来的，SendInput 的回车会落到它身上
    press_enter()
    time.sleep(1.0)
    if user32.IsWindow(dlg):  # 某些情况下第一次回车只是让文件名框失焦
        force_foreground(dlg)
        press_enter()

    produced: Path | None = None
    for _ in range(60):
        cands = [
            p
            for p in docs.glob("lumen-tasks-*.pdf")
            if p.stat().st_mtime >= started - 2 and before.get(p.name) != p.stat().st_mtime
        ]
        if cands:
            newest = max(cands, key=lambda p: p.stat().st_mtime)
            if newest.stat().st_size > 4096:
                produced = newest
                break
        time.sleep(0.5)
    if produced is None:
        toast = m.eval("Array.from(document.querySelectorAll('.toast')).map(e => e.innerText)")
        print(f"❌ 超时：没有生成 PDF；界面提示：{toast}")
        m.close()
        return 1

    OUT.write_bytes(produced.read_bytes())
    size = OUT.stat().st_size
    head = OUT.read_bytes()[:8]
    print(f"✅ 生成了 PDF：{produced.name}（{size} 字节，magic={head[:5]!r}）", flush=True)
    if size < 5000:
        print("   ⚠ 文件偏小，很可能是空白页——检查打印报告是否被父容器一起隐藏了")

    # 界面必须恢复：打印视图消失、任务列表回来
    time.sleep(1.0)
    back = m.eval(
        "(() => ({ report: !!document.querySelector('#print-report'), tasks: document.querySelectorAll('.task').length }))()"
    )
    restored = (not back.get("report")) and back.get("tasks", 0) > 0
    print(f"{'✅' if restored else '❌'} 导出后界面已恢复 —— {back}", flush=True)

    # 中文可读性：从 PDF 里抽文本
    chinese_ok = False
    try:
        from pypdf import PdfReader

        reader = PdfReader(str(OUT))
        text = "\n".join((p.extract_text() or "") for p in reader.pages)
        has_header = "任务清单" in text
        # 提取出的文本会在标点周围插入空格，因此按"去掉空白后包含"判断
        squeezed = "".join(text.split())
        sample = [t for t in titles if "".join(t.split()) in squeezed]
        print(f"{'✅' if has_header else '❌'} PDF 里能提取出中文标题「Lumen 任务清单」", flush=True)
        print(
            f"{'✅' if sample else '❌'} PDF 里能提取出任务标题 —— 命中 {len(sample)}/{len(titles)}：{sample[:3]}",
            flush=True,
        )
        print(f"   页数：{len(reader.pages)}；文本前 100 字：{text[:100]!r}", flush=True)
        chinese_ok = has_header and bool(sample)
    except Exception as e:  # noqa: BLE001
        print(f"❌ PDF 文本提取失败：{e}")

    # 清理测试产物，别把用户的文档目录当垃圾场
    try:
        produced.unlink()
        print(f"（已删除测试文件 {produced}）", flush=True)
    except OSError:
        pass

    m.close()
    return 0 if (restored and chinese_ok) else 1


if __name__ == "__main__":
    sys.exit(main())
