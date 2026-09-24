"""Cross-view smoke test against a disposable Lumen profile started by run_acceptance.py."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import time
import uuid

import ui_drive as ui
from verify_remediation3 import click_sidebar, invoke_js, set_search, verify_isolated_profile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))
results: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    results.append((name, ok, detail))
    print(f"{'✅' if ok else '❌'} {name}" + (f" —— {detail}" if detail else ""), flush=True)


def wait_value(target: ui.Target, expression: str, timeout: float = 20) -> bool:
    return bool(target.wait_for(expression, timeout=timeout))


def query_task(target: ui.Target, title: str) -> dict | None:
    rows = target.eval(invoke_js("task_list", {
        "query": {"search": title, "includeDeleted": True, "statuses": [], "limit": 50}
    })) or []
    return next((row for row in rows if row.get("title") == title), None)


def real_drag(target: ui.Target, source: str, destination: str) -> dict:
    """Exercise the WebView's HTML drag path with CDP mouse events."""
    boxes = target.eval("""(() => {
      const a = document.querySelector(%s);
      const b = document.querySelector(%s);
      if (!a || !b) throw new Error('拖拽源或目标不存在');
      a.scrollIntoView({ block: 'center', inline: 'nearest' });
      b.scrollIntoView({ block: 'center', inline: 'nearest' });
      const p = a.getBoundingClientRect(), q = b.getBoundingClientRect();
      return { ax: p.left + p.width / 2, ay: p.top + p.height / 2,
               bx: q.left + q.width / 2, by: q.top + q.height / 2 };
    })()""" % (json.dumps(source), json.dumps(destination)))
    target.call("Input.dispatchMouseEvent", {
        "type": "mouseMoved", "x": boxes["ax"], "y": boxes["ay"],
    })
    target.call("Input.dispatchMouseEvent", {
        "type": "mousePressed", "x": boxes["ax"], "y": boxes["ay"],
        "button": "left", "buttons": 1, "clickCount": 1,
    })
    for step in range(1, 13):
        t = step / 12
        target.call("Input.dispatchMouseEvent", {
            "type": "mouseMoved",
            "x": boxes["ax"] + (boxes["bx"] - boxes["ax"]) * t,
            "y": boxes["ay"] + (boxes["by"] - boxes["ay"]) * t,
            "button": "left", "buttons": 1,
        })
        time.sleep(0.04)
    # The calendar paints a drop hint on dragover. Let that new DOM settle
    # and send one final move so the browser accepts the current drop target.
    time.sleep(0.25)
    target.call("Input.dispatchMouseEvent", {
        "type": "mouseMoved", "x": boxes["bx"], "y": boxes["by"],
        "button": "left", "buttons": 1,
    })
    time.sleep(0.1)
    target.call("Input.dispatchMouseEvent", {
        "type": "mouseReleased", "x": boxes["bx"], "y": boxes["by"],
        "button": "left", "buttons": 0, "clickCount": 1,
    })
    return boxes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expect-data-dir", required=True)
    args = parser.parse_args()
    main_win = ui.connect(PORT, want="main")
    allowed, actual = verify_isolated_profile(main_win, args)
    if not allowed:
        return 1

    title = f"ZZARCH-{uuid.uuid4().hex[:10]}"
    recurring_title = f"ZZARCH-REC-{uuid.uuid4().hex[:10]}"
    task_id: str | None = None
    series_id: str | None = None
    backup_path: str | None = None
    extra_id: str | None = None
    floating: ui.Target | None = None
    try:
        # Open floating first. QuickAdd must publish across windows after save.
        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('.topbar button'))
            .find(e => e.innerText.includes('悬浮窗'));
          if (!b) throw new Error('悬浮窗按钮不存在');
          if (b.getAttribute('aria-pressed') !== 'true') b.click();
          return true;
        })()""")
        floating = ui.connect(PORT, want="floating", timeout=20)
        check("悬浮窗已打开", bool(floating))

        main_win.eval("""(() => {
          const b = document.querySelector('button[title^="新建任务"]');
          if (!b) throw new Error('新建按钮不存在'); b.click(); return true;
        })()""")
        ui.set_react_input(main_win, ".quickadd__input", title)
        ui.set_react_input(main_win, '.quickadd input[type="date"]', dt.date.today().isoformat())
        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('.quickadd button'))
            .find(e => e.innerText.trim() === '添加');
          if (!b) throw new Error('QuickAdd 添加按钮不存在'); b.click(); return true;
        })()""")
        for _ in range(40):
            row = query_task(main_win, title)
            if row:
                task_id = str(row["id"])
                break
            time.sleep(0.25)
        check("QuickAdd 真实创建任务", bool(task_id), title)
        if not task_id:
            return 1
        check("Main 显示 QuickAdd 结果", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.task__title')).some(e => e.innerText === {json.dumps(title)})"))
        check("Floating 接收跨窗口 data change", wait_value(floating,
            f"Array.from(document.querySelectorAll('.floating__text')).some(e => e.innerText.includes({json.dumps(title)}))"))

        click_sidebar(main_win, "看板")
        check("Board 显示新任务", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.boardcard__title')).some(e => e.innerText === {json.dumps(title)})"))
        real_drag(main_win, ".boardcol .boardcard", '.boardcol[aria-label^="进行中"]')
        check("Board 真实拖拽更新状态", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.boardcol[aria-label^=\"进行中\"] .boardcard__title'))"
            f".some(e => e.innerText === {json.dumps(title)})") and
            query_task(main_win, title).get("status") == "doing")
        click_sidebar(main_win, "日历")
        check("Calendar 显示今日计划", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.calchip__title')).some(e => e.innerText === {json.dumps(title)})"))
        before_plan = query_task(main_win, title)["plannedAt"]
        # The next visible day is in the same rendered grid, even across month boundaries.
        target_day_label = main_win.eval(f"""(() => {{
          const chip = Array.from(document.querySelectorAll('.calchip'))
            .find(e => e.querySelector('.calchip__title')?.innerText === {json.dumps(title)});
          if (!chip) throw new Error('日历拖拽源不存在');
          chip.dataset.acceptanceSource = 'yes';
          const cells = Array.from(document.querySelectorAll('.calcell'));
          const i = cells.indexOf(chip.closest('.calcell'));
          if (i < 0 || !cells[i + 1]) throw new Error('日历目标日期不存在');
          cells[i + 1].dataset.acceptanceTarget = 'yes';
          return cells[i + 1].getAttribute('aria-label').split('，')[0];
        }})()""")
        real_drag(main_win, '[data-acceptance-source="yes"]', '[data-acceptance-target="yes"]')
        changed_plan = wait_value(main_win,
            f"Array.from(document.querySelectorAll('.calchip')).some(e => "
            f"e.querySelector('.calchip__title')?.innerText === {json.dumps(title)} && "
            f"e.closest('.calcell')?.getAttribute('aria-label')?.startsWith({json.dumps(target_day_label)}))",
            timeout=4)
        after_plan = query_task(main_win, title).get("plannedAt")
        check("Calendar 真实拖拽改期", changed_plan and after_plan != before_plan,
              f"{before_plan} -> {after_plan}; target={target_day_label}")
        click_sidebar(main_win, "全部任务")
        set_search(main_win, title)
        check("Main 接收拖拽后的状态与日期", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.task__title')).some(e => e.innerText === {json.dumps(title)})") and
            query_task(main_win, title).get("status") == "doing")
        set_search(main_win, "")

        exported = main_win.eval(invoke_js("backup_export", {"path": None}))
        backup_path = exported["path"]
        check("隔离数据已生成备份", exported["stats"]["tasks"] >= 1, backup_path)
        extra_title = f"ZZARCH-EXTRA-{uuid.uuid4().hex[:8]}"
        extra = main_win.eval(invoke_js("task_create", {"input": {"title": extra_title}}))
        extra_id = extra["id"]
        click_sidebar(main_win, "看板")
        check("恢复前附加任务可见", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.boardcard__title')).some(e => e.innerText === {json.dumps(extra_title)})"))
        click_sidebar(main_win, "设置")
        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('[role=tab]'))
            .find(e => e.innerText === '数据与备份');
          if (!b) throw new Error('数据与备份标签不存在');
          b.click();
        })()""")
        check("设置页找到隔离备份", wait_value(main_win,
            "!!document.querySelector('.backuplist .backuprow button[title=\"查看这份备份的内容\"]')"))
        main_win.eval("""document.querySelector('.backuplist .backuprow button[title="查看这份备份的内容"]').click()""")
        check("备份恢复预览已加载", wait_value(main_win, "!!document.querySelector('.preview')"))
        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('.preview button'))
            .find(e => e.innerText.includes('恢复这份备份'));
          if (!b) throw new Error('恢复入口不存在'); b.click();
        })()""")
        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('.preview button'))
            .find(e => e.innerText.includes('确认恢复'));
          if (!b || b.disabled) throw new Error('确认恢复不可用'); b.click();
        })()""")
        check("恢复后提示重启运行时设置", wait_value(main_win,
            "Array.from(document.querySelectorAll('[role=status]')).some(e => e.innerText.includes('数据已恢复'))"))
        extra_id = None  # Restore replaced this task; no deletion is needed.
        click_sidebar(main_win, "看板")
        check("Backup restore 失效组织及任务视图", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.boardcard__title')).some(e => e.innerText === {json.dumps(title)})") and
            not main_win.eval(invoke_js("task_count", {"query": {"search": extra_title, "statuses": []}}))["total"])

        created = main_win.eval(invoke_js("recurring_create", {"input": {
            "title": recurring_title,
            "rrule": "FREQ=DAILY",
            "tzid": "UTC",
            "dtstartLocal": dt.date.today().isoformat() + "T09:00:00",
            "hasStartTime": True,
            "materializeDays": 5,
        }}))
        series_id = str(created["seriesId"])
        check("重复任务创建并物化", created["createdCount"] >= 2, str(created))
        click_sidebar(main_win, "全部任务")
        set_search(main_win, recurring_title)
        visible = wait_value(main_win,
            f"Array.from(document.querySelectorAll('.task__title')).some(e => e.innerText === {json.dumps(recurring_title)})")
        check("重复任务在主列表显示", visible)
        main_win.eval(f"""(() => {{
          const b = document.querySelector('button[aria-label="编辑「{recurring_title}」"]');
          if (!b) throw new Error('重复任务编辑按钮不存在'); b.click(); return true;
        }})()""")
        check("重复任务编辑器打开", wait_value(main_win, "!!document.querySelector('#ed-title')"))
        ui.set_react_input(main_win, "#ed-title", recurring_title + "-one")
        time.sleep(0.2)
        main_win.eval("""(() => {
          const dialog = document.querySelector('[aria-labelledby="editor-title"]');
          const b = Array.from(dialog.querySelectorAll('.modal__actions button'))
            .find(e => e.innerText.trim() === '保存');
          if (!b) throw new Error('任务保存按钮不存在'); b.click(); return true;
        })()""")
        check("编辑时弹出三范围选择", wait_value(main_win,
            "!!document.querySelector('#scope-title') && "
            "document.querySelectorAll('input[name=scope]').length === 3"))
        main_win.eval("document.querySelector('input[name=scope][value=this_only]').click()")
        main_win.eval("""(() => {
          const dialog = document.querySelector('[aria-labelledby="scope-title"]');
          const b = dialog.querySelector('.modal__actions button:last-child');
          if (!b || b.disabled) throw new Error('范围确认不可用'); b.click(); return true;
        })()""")
        check("仅此次修改已落库", wait_value(main_win,
            f"Array.from(document.querySelectorAll('.task__title')).some(e => e.innerText === {json.dumps(recurring_title + '-one')})"))
        set_search(main_win, recurring_title + "-one")
        main_win.eval(f"""(() => {{
          const b = document.querySelector('button[aria-label="将「{recurring_title}-one」移入回收站"]');
          if (!b) throw new Error('重复任务删除按钮不存在'); b.click(); return true;
        }})()""")
        check("删除时弹出三范围选择", wait_value(main_win,
            "!!document.querySelector('#scope-title') && "
            "document.querySelectorAll('input[name=scope]').length === 3"))
        main_win.eval("document.querySelector('input[name=scope][value=this_only]').click()")
        main_win.eval("document.querySelector('[aria-labelledby=scope-title] .modal__actions button:last-child').click()")
        check("仅删除此次后系列仍有其它发生", wait_value(main_win,
            f"!document.querySelector('#scope-title')") and bool(main_win.eval(invoke_js("task_count", {
                "query": {"search": recurring_title, "statuses": []}
            }))["total"]))
    except Exception as exc:  # report, then always clean up
        check("跨视图验收执行", False, f"{type(exc).__name__}: {exc}")
    finally:
        if task_id:
            try:
                main_win.eval(invoke_js("task_soft_delete", {"id": task_id}))
                main_win.eval(invoke_js("task_purge", {"id": task_id}))
            except Exception as exc:
                check("测试任务清理调用", False, str(exc))
        if extra_id:
            try:
                main_win.eval(invoke_js("task_soft_delete", {"id": extra_id}))
                main_win.eval(invoke_js("task_purge", {"id": extra_id}))
            except Exception as exc:
                check("附加任务清理调用", False, str(exc))
        if series_id:
            try:
                rows = main_win.eval(invoke_js("task_list", {
                    "query": {"search": recurring_title, "includeDeleted": True,
                              "statuses": [], "limit": 100}
                })) or []
                live = next((row for row in rows if row.get("seriesId") == series_id
                             and not row.get("deletedAt")), None)
                if live:
                    main_win.eval(invoke_js("recurring_delete", {
                        "taskId": live["id"], "mode": "whole_series", "confirmHistory": True
                    }))
                for row in rows:
                    try:
                        main_win.eval(invoke_js("task_purge", {"id": row["id"]}))
                    except Exception as exc:
                        check("重复测试实例清理", False, str(exc))
            except Exception as exc:
                check("重复系列清理调用", False, str(exc))
        try:
            residue = main_win.eval(invoke_js("task_count", {
                "query": {"search": "ZZARCH-", "includeDeleted": True, "statuses": []}
            }))
            check("测试任务残留为 0", residue["total"] == 0, str(residue))
            rec_residue = main_win.eval(invoke_js("task_count", {
                "query": {"search": recurring_title, "includeDeleted": True, "statuses": []}
            }))
            check("重复测试实例残留为 0", rec_residue["total"] == 0, str(rec_residue))
        except Exception as exc:
            check("测试任务残留检查", False, str(exc))
        if floating:
            floating.close()
        main_win.close()

    passed = sum(ok for _, ok, _ in results)
    print(f"架构实机验收：{passed}/{len(results)}，profile={actual}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
