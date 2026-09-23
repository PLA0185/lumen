"""悬浮窗实机验收：按钮、透明度、大小、完整编辑。

## 这份脚本专门验"用户报告过的问题"

用户实测反馈：悬浮窗上的「置顶」「鼠标穿透」「关闭」三个按钮点了没反应。
诊断结论是**拖动区域把 mousedown 抢走了**——顶栏既是拖动区又放着按钮，
按下按钮时先触发了 `startDragging()`，`click` 永远不会发生。

因此本脚本用 CDP 的 **Input 域派发真实鼠标事件**（而不是 `element.click()`），
只有这样才能复现并验证这个问题确实修好了。同时验证：
- 不透明度滑块拖动后接口值与持久化值都变了；
- 右下角把手存在，且尺寸接口能把窗口真正改掉；
- 尺寸改到上下限时**不出现显示 bug**（无横向溢出、把手仍可见、列表仍可滚动）；
- 悬浮窗里能打开与主窗口相同的完整编辑表单并成功保存。

前提：Lumen 正在运行，且启动时设置了
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`。
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = 9222
results: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail: str = "") -> bool:
    results.append((name, ok, detail))
    print(f"{'✅' if ok else '❌'} {name}" + (f" —— {detail}" if detail else ""), flush=True)
    return ok


def floating_state(f: ui.Target) -> dict:
    raw = f.eval(
        "(async () => JSON.stringify(await window.__TAURI_INTERNALS__.invoke('window_floating_state')))()"
    )
    return json.loads(raw) if raw else {}


def button_index(f: ui.Target, label_part: str) -> int:
    labels = f.eval(
        "Array.from(document.querySelectorAll('.floating__btn')).map(b => b.getAttribute('aria-label'))"
    ) or []
    for i, l in enumerate(labels):
        if l and label_part in l:
            return i
    return -1


def layout_report(f: ui.Target) -> dict:
    """收集"显示 bug"的直接证据：横向溢出、纵向溢出、把手与列表可见性。"""
    return f.eval(
        """
        (() => {
          const de = document.documentElement;
          const grip = document.querySelector('.floating__resize');
          const list = document.querySelector('.floating__list');
          const foot = document.querySelector('.floating__foot');
          const gr = grip ? grip.getBoundingClientRect() : null;
          return {
            viewport: [de.clientWidth, de.clientHeight],
            hOverflow: de.scrollWidth - de.clientWidth,
            vOverflow: de.scrollHeight - de.clientHeight,
            gripInside: gr ? (gr.right <= de.clientWidth + 1 && gr.bottom <= de.clientHeight + 1) : false,
            listScrollable: list ? list.scrollHeight > list.clientHeight : null,
            footVisible: foot ? foot.getBoundingClientRect().bottom <= de.clientHeight + 1 : false,
            headerVisible: (() => {
              const h = document.querySelector('.floating__head');
              return h ? h.getBoundingClientRect().top >= -1 : false;
            })(),
          };
        })()
        """
    ) or {}


def main() -> int:
    print("连接悬浮窗…", flush=True)
    main_win = ui.connect(PORT, want="main", timeout=60)
    # 确保悬浮窗处于显示状态
    main_win.eval(
        "(async () => { await window.__TAURI_INTERNALS__.invoke('window_apply_action', { action: 'show_floating' }); return true; })()"
    )
    time.sleep(1.5)
    f = ui.connect(PORT, want="floating", timeout=30)

    st = floating_state(f)
    check("悬浮窗可读运行态", bool(st), json.dumps(st, ensure_ascii=False))

    # ---------------------------------------------------------------- 按钮
    # 1) 置顶：真实鼠标点击（会先经过 mousedown —— 之前就是这里被拖拽抢走）
    idx = button_index(f, "置顶")
    if idx < 0:
        check("置顶按钮存在", False, "找不到按钮")
    else:
        before = floating_state(f).get("alwaysOnTop")
        ui.real_click(f, ".floating__btn", idx)
        time.sleep(1.2)
        after = floating_state(f).get("alwaysOnTop")
        check(
            "真实鼠标点击「置顶」能切换状态",
            before != after,
            f"{before} → {after}",
        )
        # 再点一次切回来，避免影响后续用例
        ui.real_click(f, ".floating__btn", button_index(f, "置顶") if button_index(f, "置顶") >= 0 else idx)
        time.sleep(1.2)

    # 2) 穿透：开 → 确认后端状态；再从托盘侧关闭（脚本直接调接口，
    #    因为穿透本身就意味着窗口收不到鼠标事件，这是设计使然）
    idx = button_index(f, "穿透") if button_index(f, "穿透") >= 0 else button_index(f, "开启鼠标穿透")
    if idx < 0:
        check("穿透按钮存在", False, "找不到按钮")
    else:
        ui.real_click(f, ".floating__btn", idx)
        time.sleep(1.2)
        on = floating_state(f).get("clickThrough")
        check("真实鼠标点击能开启鼠标穿透", on is True, f"clickThrough={on}")
        f.eval(
            "(async () => { await window.__TAURI_INTERNALS__.invoke('window_apply_action', { action: 'toggle_floating_click_through' }); return true; })()"
        )
        time.sleep(1.2)
        off = floating_state(f).get("clickThrough")
        check("穿透可从托盘/设置侧关闭", off is False, f"clickThrough={off}")

    # 3) 关闭（隐藏）：真实点击后窗口不可见
    idx = button_index(f, "隐藏")
    if idx < 0:
        check("关闭按钮存在", False, "找不到按钮")
    else:
        ui.real_click(f, ".floating__btn", idx)
        time.sleep(1.5)
        hidden = floating_state(f).get("visible")
        check("真实鼠标点击「关闭」能隐藏悬浮窗", hidden is False, f"visible={hidden}")
        main_win.eval(
            "(async () => { await window.__TAURI_INTERNALS__.invoke('window_apply_action', { action: 'show_floating' }); return true; })()"
        )
        time.sleep(1.5)

    f.close()
    f = ui.connect(PORT, want="floating", timeout=30)

    # ------------------------------------------------------------ 不透明度
    has_slider = f.eval("!!document.querySelector('.floating__opacity input[type=range]')")
    check("悬浮窗内提供不透明度滑块", bool(has_slider))
    if has_slider:
        # 用 JS 设值 + 派发 input（等价于拖动滑块），再等节流窗口过去
        f.eval(
            """
            (() => {
              const el = document.querySelector('.floating__opacity input[type=range]');
              const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
              setter.call(el, '55');
              el.dispatchEvent(new Event('input', { bubbles: true }));
              el.dispatchEvent(new Event('change', { bubbles: true }));
              return el.value;
            })()
            """
        )
        time.sleep(1.5)
        st = floating_state(f)
        check(
            "拖动不透明度后持久化生效",
            abs((st.get("opacity") or 0) - 0.55) < 0.01,
            f"opacity={st.get('opacity')}",
        )
        css = f.eval("getComputedStyle(document.querySelector('.floating')).opacity")
        check("界面按新不透明度渲染", abs(float(css) - 0.55) < 0.02, f"CSS opacity={css}")

        # 越界输入应被后端收敛到下限而不是变成 0
        f.eval(
            """
            (() => {
              const el = document.querySelector('.floating__opacity input[type=range]');
              const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
              setter.call(el, '1');
              el.dispatchEvent(new Event('input', { bubbles: true }));
              return true;
            })()
            """
        )
        time.sleep(1.5)
        st = floating_state(f)
        check(
            "低于下限的不透明度被收敛（不会变成全透明）",
            (st.get("opacity") or 0) >= 0.25,
            f"opacity={st.get('opacity')}",
        )
        # 调回 100% 便于后续观察
        f.eval(
            """
            (() => {
              const el = document.querySelector('.floating__opacity input[type=range]');
              const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
              setter.call(el, '100');
              el.dispatchEvent(new Event('input', { bubbles: true }));
              return true;
            })()
            """
        )
        time.sleep(1.0)

    # ---------------------------------------------------------------- 尺寸
    has_grip = f.eval("!!document.querySelector('.floating__resize')")
    check("右下角有缩放手柄", bool(has_grip))

    sizes = []
    for w, h in [(600, 520), (260, 200), (900, 700)]:
        f.eval(
            "(async () => { await window.__TAURI_INTERNALS__.invoke('window_set_floating_size', { width: %d, height: %d }); return true; })()"
            % (w, h)
        )
        time.sleep(1.2)
        inner = f.eval(
            "(async () => { const w = await window.__TAURI_INTERNALS__.invoke('window_floating_state'); return [w.width, w.height]; })()"
        )
        rep = layout_report(f)
        sizes.append((w, h, inner, rep))
        no_bug = (
            rep.get("hOverflow", 1) <= 1
            and rep.get("gripInside") is True
            and rep.get("footVisible") is True
            and rep.get("headerVisible") is True
        )
        check(
            f"尺寸 {w}×{h} 生效且无显示 bug",
            no_bug,
            f"实际 {inner}；横向溢出 {rep.get('hOverflow')}px、把手在窗口内={rep.get('gripInside')}、"
            f"底栏可见={rep.get('footVisible')}、顶栏可见={rep.get('headerVisible')}",
        )

    # 超出上下限的值必须被收敛，且窗口尺寸随之被纠正
    f.eval(
        "(async () => { await window.__TAURI_INTERNALS__.invoke('window_set_floating_size', { width: 20, height: 20 }); return true; })()"
    )
    time.sleep(1.2)
    small = floating_state(f)
    rep = layout_report(f)
    check(
        "过小尺寸被收敛到下限且界面仍可用",
        (small.get("width") or 0) >= 260 and rep.get("hOverflow", 1) <= 1,
        f"width={small.get('width')} height={small.get('height')} 横向溢出={rep.get('hOverflow')}px",
    )

    f.eval(
        "(async () => { await window.__TAURI_INTERNALS__.invoke('window_set_floating_size', { width: 5000, height: 5000 }); return true; })()"
    )
    time.sleep(1.2)
    big = floating_state(f)
    check(
        "过大尺寸被收敛到上限",
        (big.get("width") or 0) <= 1400 and (big.get("height") or 0) <= 1800,
        f"width={big.get('width')} height={big.get('height')}",
    )

    # 恢复到适合编辑的尺寸
    f.eval(
        "(async () => { await window.__TAURI_INTERNALS__.invoke('window_set_floating_size', { width: 380, height: 520 }); return true; })()"
    )
    time.sleep(1.2)

    # ------------------------------------------------------------ 完整编辑
    edit_btn = f.eval("!!document.querySelector('.floating__item-btn')")
    check("列表项提供「打开完整编辑」按钮", bool(edit_btn))

    if edit_btn:
        f.eval("document.querySelector('.floating__item-btn').click(); true")
        time.sleep(1.0)
        editor = f.eval(
            """
            (() => {
              const m = document.querySelector('.modal');
              if (!m) return null;
              return {
                title: (document.querySelector('.modal__title') || {}).innerText || '',
                fields: document.querySelectorAll('.modal input, .modal textarea, .modal select').length,
                hOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
              };
            })()
            """
        )
        check(
            "悬浮窗内能打开完整编辑表单",
            bool(editor) and editor.get("fields", 0) >= 8,
            json.dumps(editor, ensure_ascii=False) if editor else "未打开",
        )

        # 真的改一个字段并保存：这条路径此前会因 SQL 拼装错误而整体失败
        if editor:
            new_title = "验收-悬浮窗完整编辑已保存"
            f.eval(
                """
                (() => {
                  const el = document.querySelector('.modal input');
                  if (!el) throw new Error('找不到标题输入框');
                  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
                  setter.call(el, %s);
                  el.dispatchEvent(new Event('input', { bubbles: true }));
                  return el.value;
                })()
                """
                % json.dumps(new_title)
            )
            time.sleep(0.4)
            f.eval(
                """
                (() => {
                  const btn = Array.from(document.querySelectorAll('.modal__actions button'))
                    .find(b => b.innerText.includes('保存'));
                  if (!btn) throw new Error('找不到保存按钮');
                  btn.click();
                  return true;
                })()
                """
            )
            time.sleep(2.0)
            still_open = f.eval("!!document.querySelector('.modal')")
            err = f.eval("document.querySelector('.modal .alert--error, .floating__error')?.innerText")
            titles = f.eval(
                "Array.from(document.querySelectorAll('.floating__text')).map(e => e.innerText)"
            )
            check(
                "悬浮窗里保存改动能落库（表单关闭且标题更新）",
                (not still_open) and (new_title in (titles or [])),
                f"表单仍打开={still_open}；错误={err}；标题={titles}",
            )

    f.close()
    main_win.close()

    passed = sum(1 for _, ok, _ in results if ok)
    print(f"\n通过 {passed}/{len(results)} 项")
    failed = [n for n, ok, _ in results if not ok]
    if failed:
        print("未通过：" + "；".join(failed))
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
