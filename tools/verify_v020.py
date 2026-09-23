"""Lumen 0.2.0 实机验收：驱动真实界面验证本轮新增能力。

运行前提：Lumen 正在运行，且启动时设置了
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`。

设计原则（与任务书 §11 一致）：**只认实测到的 DOM/数据库结果**，
不把"代码看起来对"当成验收通过。每条断言都会打印实际观察到的值。
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


def card_titles(main: ui.Target) -> list[str]:
    return main.eval(
        "Array.from(document.querySelectorAll('.task .task__title')).map(e => e.innerText)"
    ) or []


def wait_for_title(main: ui.Target, title: str, timeout: float = 12.0) -> bool:
    """等某张卡片出现。

    界面的刷新是异步的（写库 → reload → 重渲染），固定 sleep 会写成
    "有时过有时不过"的脆弱测试，所以统一改成轮询。
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        if title in card_titles(main):
            return True
        time.sleep(0.3)
    return False


def click_all_tasks(main: ui.Target) -> None:
    """点一次「全部任务」——顺带触发列表 reload（清理后需要它来刷新界面）"""
    main.eval(
        """
        (() => {
          const btn = Array.from(document.querySelectorAll('.sidebar button'))
            .find(b => b.innerText.includes('全部任务'));
          if (btn) btn.click();
          return true;
        })()
        """
    )
    time.sleep(0.8)


def add_task(main: ui.Target, title: str, date: str | None = None) -> bool:
    """用界面上的快速添加建一条任务；返回是否真的出现在列表里。"""
    if not ui.text_of(main, ".quickadd"):
        ui.click(main, ".topbar__actions button[title^='新建任务']")
        time.sleep(0.4)
    ui.set_react_input(main, ".quickadd__input", title)
    time.sleep(0.2)
    if date:
        ui.set_react_input(main, "input[aria-label='计划执行日期']", date)
        time.sleep(0.3)
    ui.press_key(main, ".quickadd__input", "Enter")
    return wait_for_title(main, title)


def main() -> int:
    print("连接主窗口…", flush=True)
    main_win = ui.connect(PORT, want="main", timeout=60)

    # ---------------------------------------------------------------- 启动
    booted = main_win.wait_for("document.querySelector('.app')", timeout=30)
    check("主窗口加载完成", booted)
    if not booted:
        return 2

    sidebar = ui.text_of(main_win, ".sidebar") or ""
    check("侧边栏渲染出主导航", "今天" in sidebar and "设置" in sidebar, sidebar.splitlines()[:3])

    # 切到「全部任务」：默认视图是「今天」，而没有计划日期的任务不会出现在今天
    # （这是刻意的口径，不是 bug），所以要在全部任务里做增删改的断言。
    click_all_tasks(main_win)
    check("可切换到「全部任务」视图", "全部任务" in (ui.text_of(main_win, ".topbar__title") or ""))

    # 清掉上一轮残留的同名任务，保证断言只看本轮。
    # 注意：这里走的是原始 IPC，**不会自带界面刷新**，所以删完要再点一次
    # 导航触发 reload，否则 DOM 里还是旧列表。
    main_win.eval(
        """
        (async () => {
          const inv = window.__TAURI_INTERNALS__.invoke;
          const rows = await inv('task_list', { query: { limit: 500 } });
          for (const t of rows) {
            if (t.title.startsWith('验收-')) await inv('task_soft_delete', { id: t.id });
          }
          return true;
        })()
        """
    )
    click_all_tasks(main_win)
    cleaned = False
    for _ in range(20):
        if not [t for t in card_titles(main_win) if t.startswith("验收-")]:
            cleaned = True
            break
        click_all_tasks(main_win)
    check("清理上一轮残留任务", cleaned, f"列表：{card_titles(main_win)}")

    # ------------------------------------------------- 新建任务（空库首条）
    # 这一条同时在验证本轮修复的真实缺陷：全新数据库里创建第一条任务
    # 曾因 sortValue 类型不匹配而直接失败。
    if not ui.text_of(main_win, ".quickadd"):
        ui.click(main_win, ".topbar__actions button[title^='新建任务']")
        time.sleep(0.4)

    has_quickadd = main_win.wait_for("document.querySelector('.quickadd__input')", timeout=5)
    check("「新建」能打开快速添加表单", has_quickadd)

    first_title = "验收-第一条任务"
    if not wait_for_title(main_win, first_title, timeout=1):
        add_task(main_win, first_title)
    check("新建的第一条任务出现在列表里", wait_for_title(main_win, first_title), f"当前列表：{card_titles(main_win)}")

    # ------------------------------------------------------------ 任务复制
    idx = card_titles(main_win).index(first_title) if first_title in card_titles(main_win) else -1
    if idx >= 0:
        main_win.eval(
            """
            (() => {
              const card = document.querySelectorAll('.task')[%d];
              const btn = Array.from(card.querySelectorAll('.task__actions button'))
                .find(b => (b.getAttribute('title') || '').startsWith('复制'));
              if (!btn) throw new Error('卡片上没有复制按钮');
              btn.click();
              return true;
            })()
            """
            % idx
        )
        dup_ok = wait_for_title(main_win, f"{first_title}（副本）")
        check("复制任务生成「（副本）」", dup_ok, f"当前列表：{card_titles(main_win)}")
    else:
        check("复制任务生成「（副本）」", False, "找不到源任务卡片")

    # ---------------------------------------------------------- 拖拽排序
    # 造两条排序目标
    for extra in ["验收-排序A", "验收-排序B"]:
        if extra not in card_titles(main_win):
            add_task(main_win, extra)
    check(
        "两条排序目标都已就位",
        "验收-排序A" in card_titles(main_win) and "验收-排序B" in card_titles(main_win),
        f"当前列表：{card_titles(main_win)}",
    )

    sortable = main_win.eval(
        "!!document.querySelector('.task[draggable=\"true\"]')"
    )
    grip = main_win.eval("!!document.querySelector('.task__grip')")
    check("任务卡片可拖拽且带拖拽手柄", bool(sortable) and bool(grip), f"draggable={sortable} grip={grip}")

    before = card_titles(main_win)
    if "验收-排序B" in before and "验收-排序A" in before:
        # 三个事件之间要留出时间：React 的状态更新是异步的，
        # 同一个 JS 任务里连发 dragstart/dragover/dragend 时，
        # dragend 上的闭包还是旧状态（真实拖拽天然跨帧，不会有这个问题）。
        main_win.eval(
            """
            (() => {
              const cards = Array.from(document.querySelectorAll('.task'));
              const titleOf = c => c.querySelector('.task__title').innerText;
              const a = cards.find(c => titleOf(c) === '验收-排序B');
              if (!a) return 'notfound';
              a.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: new DataTransfer() }));
              return 'ok';
            })()
            """
        )
        time.sleep(0.4)
        main_win.eval(
            """
            (() => {
              const cards = Array.from(document.querySelectorAll('.task'));
              const titleOf = c => c.querySelector('.task__title').innerText;
              const b = cards.find(c => titleOf(c) === '验收-排序A');
              if (!b) return 'notfound';
              b.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }));
              return 'ok';
            })()
            """
        )
        time.sleep(0.4)
        main_win.eval(
            """
            (() => {
              const cards = Array.from(document.querySelectorAll('.task'));
              const titleOf = c => c.querySelector('.task__title').innerText;
              const a = cards.find(c => titleOf(c) === '验收-排序B');
              if (!a) return 'notfound';
              a.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: new DataTransfer() }));
              return 'ok';
            })()
            """
        )
        time.sleep(1.5)
        after = card_titles(main_win)
        ok = (
            "验收-排序B" in after
            and "验收-排序A" in after
            and after.index("验收-排序B") < after.index("验收-排序A")
        )
        check(
            "拖拽后顺序真的改变（B 移到 A 之前）",
            ok,
            f"拖前 A@{before.index('验收-排序A')} B@{before.index('验收-排序B')} → "
            f"拖后 A@{after.index('验收-排序A')} B@{after.index('验收-排序B')}",
        )
    else:
        check("拖拽后顺序真的改变（B 移到 A 之前）", False, f"排序目标缺失：{before}")

    # ------------------------------------------------- 悬浮窗就地编辑任务
    # 悬浮窗只显示「计划时间在今天」的任务，所以先建一条带今天日期的
    today_title = "验收-悬浮窗任务"
    today = time.strftime("%Y-%m-%d")
    if today_title not in card_titles(main_win):
        add_task(main_win, today_title, date=today)

    main_win.eval(
        """
        (async () => {
          const inv = window.__TAURI_INTERNALS__.invoke;
          await inv('window_apply_action', { action: 'show_floating' });
          return true;
        })()
        """
    )
    time.sleep(1.5)

    try:
        floating = ui.connect(PORT, want="floating", timeout=30)
    except ui.CdpError as e:
        check("悬浮窗可连接并显示今日任务", False, str(e))
        floating = None

    if floating is not None:
        found = floating.wait_for(
            f"Array.from(document.querySelectorAll('.floating__text')).some(e => e.innerText === {json.dumps(today_title)})",
            timeout=20,
        )
        check("悬浮窗显示今天的新建任务", found)

        if found:
            # 双击标题 → 就地改名 → 回车
            floating.eval(
                """
                (() => {
                  const el = Array.from(document.querySelectorAll('.floating__text'))
                    .find(e => e.innerText === %s);
                  if (!el) throw new Error('找不到标题');
                  el.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
                  return true;
                })()
                """
                % json.dumps(today_title)
            )
            has_input = floating.wait_for("document.querySelector('.floating__edit')", timeout=5)
            check("双击标题进入就地编辑", has_input)

            if has_input:
                new_title = "验收-悬浮窗改名成功"
                ui.set_react_input(floating, ".floating__edit", new_title)
                time.sleep(0.2)
                ui.press_key(floating, ".floating__edit", "Enter")
                renamed_here = floating.wait_for(
                    f"Array.from(document.querySelectorAll('.floating__text')).some(e => e.innerText === {json.dumps(new_title)})",
                    timeout=10,
                )
                # 主窗口应通过 tasks-changed 广播同步到改名结果
                synced = False
                for _ in range(30):
                    if new_title in card_titles(main_win):
                        synced = True
                        break
                    time.sleep(0.3)
                check("悬浮窗内改名生效", renamed_here)
                check("主窗口自动同步悬浮窗的改动（跨窗口广播）", synced, f"主窗口列表：{card_titles(main_win)}")

        # 悬浮窗的置顶 / 穿透按钮存在
        tools = floating.eval(
            "Array.from(document.querySelectorAll('.floating__btn')).map(b => b.getAttribute('aria-label'))"
        )
        check(
            "悬浮窗提供置顶 / 穿透 / 隐藏按钮",
            tools is not None and len(tools) >= 3,
            str(tools),
        )

        # 就地新增
        if floating.eval("!!document.querySelector('.floating__add-input')"):
            add_title = "验收-悬浮窗直接添加"
            ui.set_react_input(floating, ".floating__add-input", add_title)
            time.sleep(0.2)
            ui.press_key(floating, ".floating__add-input", "Enter")
            added = floating.wait_for(
                f"Array.from(document.querySelectorAll('.floating__text')).some(e => e.innerText === {json.dumps(add_title)})",
                timeout=10,
            )
            check("悬浮窗内可直接添加今天的任务", added)
        else:
            check("悬浮窗内可直接添加今天的任务", False, "找不到底部输入框")
        floating.close()

    main_win.close()

    # --------------------------------------------------------------- 汇总
    passed = sum(1 for _, ok, _ in results if ok)
    print(f"\n通过 {passed}/{len(results)} 项")
    failed = [n for n, ok, _ in results if not ok]
    if failed:
        print("未通过：" + "；".join(failed))
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
