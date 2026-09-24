"""Verify Board, Calendar, and FloatingToday limits in a disposable profile."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import time
import uuid

import ui_drive as ui
from verify_remediation3 import click_sidebar, invoke_js, verify_isolated_profile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))
COUNT = 2000
results: list[tuple[str, bool]] = []


def check(label: str, ok: bool, detail: str = "") -> None:
    results.append((label, ok))
    print(f"{'✅' if ok else '❌'} {label}" + (f" —— {detail}" if detail else ""), flush=True)


def ids_for_prefix(main: ui.Target, prefix: str) -> list[str]:
    query = {"search": prefix, "includeDeleted": True, "statuses": []}
    total = main.eval(invoke_js("task_count", {"query": query}))["total"]
    ids: list[str] = []
    for offset in range(0, total, 250):
        rows = main.eval(invoke_js("task_list", {"query": {
            **query, "limit": 250, "offset": offset, "sortBy": "created"
        }})) or []
        ids.extend(row["id"] for row in rows if row["title"].startswith(prefix))
    return list(dict.fromkeys(ids))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expect-data-dir", required=True)
    args = parser.parse_args()
    main_win = ui.connect(PORT, want="main")
    allowed, actual = verify_isolated_profile(main_win, args)
    if not allowed:
        return 1
    prefix = f"ZZSCALE-{uuid.uuid4().hex[:8]}-"
    midnight = dt.datetime.combine(dt.date.today(), dt.time.min).astimezone()
    planned = midnight.astimezone(dt.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
    floating: ui.Target | None = None
    try:
        for base in range(0, COUNT, 200):
            batch = min(200, COUNT - base)
            js = """(async () => {
              const ids = [];
              for (let i = 0; i < %d; i++) {
                const row = await window.__TAURI_INTERNALS__.invoke('task_create', {
                  input: { title: %s + String(%d + i), plannedAt: %s, hasPlannedTime: false }
                });
                ids.push(row.id);
              }
              return ids;
            })()""" % (batch, json.dumps(prefix), base, json.dumps(planned))
            got = main_win.eval(js) or []
            check(f"seed {base + batch}/{COUNT}", len(got) == batch)

        click_sidebar(main_win, "看板")
        board_ready = main_win.wait_for(
            "!!document.querySelector('.board__more') && "
            f"document.querySelector('.board__more').innerText.includes('{COUNT}')", timeout=20)
        check("Board 显示真实总数和加载更多", bool(board_ready),
              str(main_win.eval("document.querySelector('.board__more')?.innerText")))

        click_sidebar(main_win, "日历")
        calendar_ready = main_win.wait_for(
            "Array.from(document.querySelectorAll('.calendar [role=status]'))"
            f".some(e => e.innerText.includes('{COUNT}') && e.innerText.includes('前 1000'))",
            timeout=20,
        )
        check(f"Calendar 超过上限时明示 1000/{COUNT}", bool(calendar_ready),
              str(main_win.eval("document.querySelector('.calendar [role=status]')?.innerText")))

        main_win.eval("""(() => {
          const b = Array.from(document.querySelectorAll('.topbar button'))
            .find(e => e.innerText.includes('悬浮窗'));
          if (!b) throw new Error('悬浮窗按钮不存在'); b.click(); return true;
        })()""")
        floating = ui.connect(PORT, want="floating", timeout=20)
        floating_ready = floating.wait_for(
            "document.querySelectorAll('.floating__item').length === 100 && "
            "Array.from(document.querySelectorAll('.floating__hint'))"
            f".some(e => e.innerText.includes('共 {COUNT} 项'))", timeout=20)
        check(f"Floating 显示前 100/共 {COUNT} 的真实总数", bool(floating_ready),
              str(floating.eval("document.querySelector('.floating__hint')?.innerText")))

        click_sidebar(main_win, "全部任务")
        main_win.wait_for("!!document.querySelector('.task .task__body')", timeout=20)
        main_win.eval("document.querySelector('.task .task__body').click()")
        main_win.wait_for("!!document.querySelector('.task__detail .deps')", timeout=10)
        main_win.eval("document.querySelector('.task__detail .deps button').click()")
        initial = main_win.wait_for(
            f"document.querySelector('.depspicker .setgroup__hint')?.innerText.includes('50 / {COUNT}')",
            timeout=15,
        )
        check("DependencyEditor 分页加载候选", bool(initial))
        target_title = f"{prefix}{COUNT - 1}"
        ui.set_react_input(main_win, '.depspicker input[aria-label="搜索要作为前置的任务"]', target_title)
        found = main_win.wait_for(
            "document.querySelector('.depspicker .setgroup__hint')?.innerText.includes('1 / 1') && "
            f"Array.from(document.querySelectorAll('.depspicker__item')).some(e => e.innerText === {json.dumps(target_title)})",
            timeout=15,
        )
        check("DependencyEditor 服务端搜索命中末尾任务", bool(found), target_title)
    except Exception as exc:
        check("规模验收执行", False, f"{type(exc).__name__}: {exc}")
    finally:
        try:
            ids = ids_for_prefix(main_win, prefix)
            for base in range(0, len(ids), 100):
                chunk = ids[base:base + 100]
                main_win.eval(invoke_js("task_bulk", {"input": {"ids": chunk, "action": "delete"}}))
                js = """(async () => {
                  let removed = 0;
                  for (const id of %s) {
                    await window.__TAURI_INTERNALS__.invoke('task_purge', { id });
                    removed++;
                  }
                  return removed;
                })()""" % json.dumps(chunk)
                main_win.eval(js)
            left = ids_for_prefix(main_win, prefix)
            check("规模测试残留为 0", len(left) == 0, f"{len(left)} 条")
        except Exception as exc:
            check("规模测试清理", False, f"{type(exc).__name__}: {exc}")
        if floating:
            floating.close()
        main_win.close()
    passed = sum(ok for _, ok in results)
    print(f"规模实机验收：{passed}/{len(results)}，profile={actual}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
