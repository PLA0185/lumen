"""Exercise recurring creation and rule editing through the real desktop UI."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import uuid

import ui_drive as ui
from verify_remediation3 import click_sidebar, invoke_js, set_search, verify_isolated_profile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))
results: list[tuple[str, bool]] = []


def check(label: str, ok: bool) -> None:
    results.append((label, ok))
    print(f"{'✅' if ok else '❌'} {label}", flush=True)


def select(target: ui.Target, selector: str, value: str) -> None:
    target.eval("""(() => {
      const el = document.querySelector(%s);
      if (!el) throw new Error('Missing select: ' + %s);
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;
      setter.call(el, %s);
      el.dispatchEvent(new Event('change', { bubbles: true }));
    })()""" % (json.dumps(selector), json.dumps(selector), json.dumps(value)))


def real_click_visible(target: ui.Target, selector: str) -> None:
    target.eval("document.querySelector(%s).scrollIntoView({block:'center'})" % json.dumps(selector))
    ui.real_click(target, selector)


def rows(target: ui.Target, title: str) -> list[dict]:
    return target.eval(invoke_js("task_list", {"query": {
        "search": title, "statuses": [], "includeDeleted": True, "limit": 1000,
    }})) or []


def open_editor(target: ui.Target, title: str) -> None:
    click_sidebar(target, "全部任务")
    set_search(target, title)
    if not target.wait_for("!!document.querySelector('button[aria-label^=\"编辑\"]')", timeout=15):
        raise RuntimeError("Recurring task is not visible in the list")
    target.eval("""(() => {
      const b = Array.from(document.querySelectorAll('button[aria-label^="编辑"]'))
        .find(b => b.getAttribute('aria-label') === %s);
      if (!b) throw new Error('Missing task editor button');
      b.click();
    })()""" % json.dumps(f"编辑「{title}」"))
    if not target.wait_for("!!document.querySelector('#ed-title')"):
        raise RuntimeError("Task editor did not open")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expect-data-dir", required=True)
    args = parser.parse_args()
    target = ui.connect(PORT, want="main")
    allowed, actual = verify_isolated_profile(target, args)
    if not allowed:
        target.close()
        return 1

    title = f"ZZREC-{uuid.uuid4().hex[:10]}"
    series_id: str | None = None
    try:
        ui.real_click(target, 'button[title="新建可以按规则重复的任务"]')
        check("真实点击打开重复任务创建表单", target.wait_for("!!document.querySelector('#rec-name')"))
        ui.set_react_input(target, "#rec-name", title)
        select(target, '[aria-labelledby="rec-title"] select[aria-label="重复频率"]', "daily")
        ui.set_react_input(target, '[aria-labelledby="rec-title"] input[aria-label="首次发生日期"]',
                           dt.date.today().isoformat())
        check("创建前能看见规则预览", target.wait_for("!!document.querySelector('[aria-labelledby=rec-title] .rulepreview__list li')"))
        real_click_visible(target, '[aria-labelledby="rec-title"] .modal__actions button:last-child')
        closed = target.wait_for("!document.querySelector('#rec-name')", timeout=15)
        if not closed:
            print("创建错误：" + str(target.eval("document.querySelector('[aria-labelledby=rec-title] .alert--error')?.innerText")), flush=True)
        check("真实点击创建后表单关闭", closed)

        created_rows = rows(target, title)
        check("重复任务从 UI 创建且有未来发生", len(created_rows) >= 2)
        if not created_rows:
            raise RuntimeError("创建后没有找到重复任务")
        series_id = created_rows[0]["seriesId"]

        open_editor(target, title)
        real_click_visible(target, '[aria-labelledby="editor-title"] button[class*="btn--quiet"]')
        check("已有任务能打开规则编辑表单", target.wait_for("!!document.querySelector('#series-rule-title')"))
        check("范围与时区可见", bool(target.eval("""(() =>
          document.querySelectorAll('input[name="rule-scope"]').length === 2 &&
          !!document.querySelector('#series-timezone'))()""")))
        select(target, '[aria-labelledby="series-rule-title"] select[aria-label="重复频率"]', "weekly")
        select(target, '[aria-labelledby="series-rule-title"] select[aria-label="结束条件"]', "count")
        ui.set_react_input(target, '[aria-labelledby="series-rule-title"] input[aria-label="重复次数"]', "4")
        ui.set_react_input(target, "#series-timezone", "America/New_York")
        check("修改后预览可见", target.wait_for(
            "!!document.querySelector('[aria-labelledby=series-rule-title] .rulepreview__list li')"))
        real_click_visible(target, '[aria-labelledby="series-rule-title"] .modal__actions button:last-child')
        check("未来规则保存后对话框关闭", target.wait_for("!document.querySelector('#series-rule-title')", timeout=15))
        detail = target.eval(invoke_js("recurring_get", {"seriesId": series_id}))
        last = detail["segments"][-1]
        check("未来分段保留频率、次数和时区", "FREQ=WEEKLY" in last["newRrule"]
              and "COUNT=4" in last["newRrule"]
              and last["newTzid"] == "America/New_York")
        check("系列结束条件元数据同步", detail["series"]["recurrenceEndKind"] == "count"
              and detail["series"]["recurrenceCount"] == 4)

        # A second real UI pass verifies the alternate whole-series scope.
        open_editor(target, title)
        real_click_visible(target, '[aria-labelledby="editor-title"] button[class*="btn--quiet"]')
        target.eval("document.querySelectorAll('input[name=rule-scope]')[1].click()")
        select(target, '[aria-labelledby="series-rule-title"] select[aria-label="重复频率"]', "monthly")
        check("整系列新规则已进入预览", target.wait_for(
            "document.querySelector('[aria-labelledby=series-rule-title] .rulepreview code')?.textContent.includes('FREQ=MONTHLY')"))
        real_click_visible(target, '[aria-labelledby="series-rule-title"] .modal__actions button:last-child')
        check("整个系列范围通过 UI 保存", target.wait_for("!document.querySelector('#series-rule-title')", timeout=15))
        detail = target.eval(invoke_js("recurring_get", {"seriesId": series_id}))
        latest = max(detail["segments"], key=lambda segment: segment["ruleVersion"])
        check("整个系列变更新增持久化分段", len(detail["segments"]) >= 2
              and "FREQ=MONTHLY" in latest["newRrule"])
    except Exception as exc:
        check(f"重复任务 UI 验收执行失败：{type(exc).__name__}: {exc}", False)
    finally:
        if series_id:
            try:
                all_rows = rows(target, title)
                live = next((r for r in all_rows if r.get("seriesId") == series_id and not r.get("deletedAt")), None)
                if live:
                    target.eval(invoke_js("recurring_delete", {
                        "taskId": live["id"], "mode": "whole_series", "confirmHistory": True,
                    }))
                for row in all_rows:
                    target.eval(invoke_js("task_purge", {"id": row["id"]}))
                check("验收任务残留为零", len(rows(target, title)) == 0)
            except Exception as exc:
                check(f"重复任务清理失败：{exc}", False)
        target.close()

    passed = sum(ok for _, ok in results)
    print(f"重复任务实机验收：{passed}/{len(results)}，profile={actual}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
