"""Exercise recurring creation and rule editing through the real desktop UI."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import uuid
from pathlib import Path

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
    subtask_title = f"保留子任务-{uuid.uuid4().hex[:6]}"
    prerequisite_title = f"ZZREC-PREREQ-{uuid.uuid4().hex[:10]}"
    delete_title = f"ZZREC-DEL-{uuid.uuid4().hex[:10]}"
    series_id: str | None = None
    delete_series_id: str | None = None
    prerequisite_id: str | None = None
    try:
        ui.real_click(target, 'button[title="新建可以按规则重复的任务"]')
        check("真实点击打开重复任务创建表单", target.wait_for("!!document.querySelector('#rec-name')"))
        ui.set_react_input(target, "#rec-name", title)
        select(target, '[aria-labelledby="rec-title"] select[aria-label="重复频率"]', "daily")
        ui.set_react_input(target, '[aria-labelledby="rec-title"] input[aria-label="首次发生日期"]',
                           dt.date.today().isoformat())
        ui.set_react_input(target, '[aria-labelledby="rec-title"] input[aria-label="首次发生时刻（留空表示仅日期）"]',
                           "09:00")
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
        prerequisite_id = target.eval(invoke_js("task_create", {"input": {
            "title": prerequisite_title, "status": "todo",
        }}))["id"]

        click_sidebar(target, "全部任务")
        set_search(target, title)
        real_click_visible(target, 'button[aria-label="展开「%s」的详情"]' % title)
        ui.set_react_input(target, 'input[aria-label="新子任务标题"]', subtask_title)
        real_click_visible(target, '.subnew button')
        check("真实界面添加子任务", target.wait_for(
            "Array.from(document.querySelectorAll('.sublist li')).some(e => e.innerText.includes(%s))"
            % json.dumps(subtask_title)))
        preserved = next((row for row in created_rows if any(
            subtask_title == sub.get("title") for sub in (target.eval(invoke_js(
                "subtask_list", {"taskId": row["id"]})) or []))), None)
        check("子任务绑定到重复发生", preserved is not None)
        select(target, '.remnew select[aria-label="提醒类型"]', "at_planned")
        real_click_visible(target, '.remnew button')
        if preserved:
            check("真实界面添加提醒", target.wait_for(
                "!!document.querySelector('.remlist li')") and bool(target.eval(invoke_js(
                    "reminder_list", {"taskId": preserved["id"]}))))
            real_click_visible(target, '.deps > button')
            ui.set_react_input(target, '.depspicker input[aria-label="搜索要作为前置的任务"]',
                               prerequisite_title)
            check("依赖选择器找到前置任务", target.wait_for(
                "Array.from(document.querySelectorAll('.depspicker__item')).some(e => e.innerText.includes(%s))"
                % json.dumps(prerequisite_title)))
            real_click_visible(target, '.depspicker__item')
            check("真实界面添加前置依赖", target.wait_for(
                "!!document.querySelector('.dependency--blocking')") and any(
                    item["dependsOnId"] == prerequisite_id for item in
                    (target.eval(invoke_js("dependency_list", {"taskId": preserved["id"]})) or [])))
            source = Path(args.expect_data_dir) / f"copy-source-{uuid.uuid4().hex[:8]}.txt"
            source.write_text("Lumen copied attachment preservation", encoding="utf-8")
            # The native Windows file chooser is intentionally not driven here:
            # a failed automation attempt left it visible to the desktop user.
            # Add the isolated fixture through the real backend, then verify
            # the card renders it and the subsequent rule rebuild preserves it.
            select(target, '.attachnew select[aria-label="附件存储方式"]', "copied")
            target.eval(invoke_js("attachment_add", {
                "taskId": preserved["id"], "sourcePath": str(source), "mode": "copied",
            }))
            real_click_visible(target, 'button[aria-label="收起「%s」的详情"]' % title)
            real_click_visible(target, 'button[aria-label="展开「%s」的详情"]' % title)
            check("隔离附件副本经后端添加并在界面显示", target.wait_for(
                "!!document.querySelector('.attachlist .attachrow')", timeout=5))
            attachments = target.eval(invoke_js("attachment_list", {"taskId": preserved["id"]})) or []
            check("副本附件绑定到重复发生", len(attachments) == 1 and
                  attachments[0].get("storageMode") == "copied")

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
        if preserved:
            same = next((row for row in rows(target, title) if row["id"] == preserved["id"]), None)
            children = target.eval(invoke_js("subtask_list", {"taskId": preserved["id"]})) or []
            check("改规则后子任务与原发生身份仍在", same is not None and
                  any(sub.get("title") == subtask_title for sub in children))
            reminders = target.eval(invoke_js("reminder_list", {"taskId": preserved["id"]})) or []
            check("改规则后提醒仍绑定原发生", bool(reminders))
            dependencies = target.eval(invoke_js("dependency_list", {"taskId": preserved["id"]})) or []
            check("改规则后前置依赖仍绑定原发生", any(
                item["dependsOnId"] == prerequisite_id for item in dependencies))
            attachments_after = target.eval(invoke_js("attachment_list", {"taskId": preserved["id"]})) or []
            check("改规则后副本附件仍绑定原发生", bool(attachments_after) and
                  bool(attachments) and attachments_after[0]["id"] == attachments[0]["id"])
            if attachments:
                target.eval(invoke_js("attachment_cleanup_orphans", {}))
                copied_path = target.eval(invoke_js("attachment_reveal", {"id": attachments[0]["id"]}))
                check("孤儿清理后副本文件仍可访问", Path(copied_path).is_file())

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

        # Deletion uses the same real card and scope UI that a user sees.
        created = target.eval(invoke_js("recurring_create", {"input": {
            "title": delete_title, "rrule": "FREQ=DAILY", "tzid": "UTC",
            "dtstartLocal": dt.date.today().isoformat() + "T09:00:00",
            "hasStartTime": True, "materializeDays": 10,
        }}))
        delete_series_id = created["seriesId"]
        click_sidebar(target, "全部任务")
        set_search(target, delete_title)
        check("删除前系列有多个可见发生", target.wait_for(
            "document.querySelectorAll('.task__title').length >= 2"))
        real_click_visible(target, 'button[aria-label="将「%s」移入回收站"]' % delete_title)
        check("删除入口展示三个范围", target.wait_for(
            "document.querySelectorAll('input[name=scope]').length === 3"))
        target.eval("document.querySelector('input[name=scope][value=this_only]').click()")
        real_click_visible(target, '[aria-labelledby="scope-title"] .modal__actions button:last-child')
        check("仅删除一次通过 UI 保存", target.wait_for("!document.querySelector('#scope-title')")
              and any(row.get("deletedAt") for row in rows(target, delete_title))
              and any(not row.get("deletedAt") for row in rows(target, delete_title)))
        set_search(target, delete_title)
        real_click_visible(target, 'button[aria-label="将「%s」移入回收站"]' % delete_title)
        target.eval("document.querySelector('input[name=scope][value=this_and_future]').click()")
        real_click_visible(target, '[aria-labelledby="scope-title"] .modal__actions button:last-child')
        check("删除此次及以后通过 UI 保存", target.wait_for("!document.querySelector('#scope-title')"))
        stopped = target.eval(invoke_js("recurring_get", {"seriesId": delete_series_id}))
        check("停止边界持久化", bool(stopped["series"]["terminatedFromOccurrenceKey"]))
        later_start = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=60)).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        later_end = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=70)).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        generated = target.eval(invoke_js("recurring_materialize", {
            "seriesId": delete_series_id, "rangeStartUtc": later_start, "rangeEndUtc": later_end,
        }))
        check("浏览更远日期不会重新生成已停止的系列", generated == 0)
        before_calendar = len(rows(target, delete_title))
        click_sidebar(target, "日历")
        check("真实界面进入日历", target.wait_for("!!document.querySelector('.calendar')"))
        for _ in range(3):
            real_click_visible(target, '.calendar .calbar__nav button[aria-label="下一页"]')
        check("浏览未来三个月仍不复活停止的系列", target.wait_for(
            "!!document.querySelector('.calendar') && !document.querySelector('.calendar .skeleton')")
              and len(rows(target, delete_title)) == before_calendar)
        click_sidebar(target, "全部任务")

        live_rows = [row for row in rows(target, delete_title) if not row.get("deletedAt")]
        if live_rows:
            set_search(target, delete_title)
            real_click_visible(target, 'button[aria-label="将「%s」移入回收站"]' % delete_title)
            target.eval("document.querySelector('input[name=scope][value=whole_series]').click()")
            real_click_visible(target, '[aria-labelledby="scope-title"] .modal__actions button:last-child')
            check("删除整个系列通过 UI 保存", target.wait_for("!document.querySelector('#scope-title')"))
            check("整个系列不再有活动发生", not any(
                not row.get("deletedAt") for row in rows(target, delete_title)))
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
        if delete_series_id:
            try:
                all_rows = rows(target, delete_title)
                live = next((r for r in all_rows if r.get("seriesId") == delete_series_id and not r.get("deletedAt")), None)
                if live:
                    target.eval(invoke_js("recurring_delete", {
                        "taskId": live["id"], "mode": "whole_series", "confirmHistory": True,
                    }))
                for row in all_rows:
                    target.eval(invoke_js("task_purge", {"id": row["id"]}))
                check("删除范围验收任务残留为零", len(rows(target, delete_title)) == 0)
            except Exception as exc:
                check(f"删除范围验收清理失败：{exc}", False)
        if prerequisite_id:
            try:
                target.eval(invoke_js("task_soft_delete", {"id": prerequisite_id}))
                target.eval(invoke_js("task_purge", {"id": prerequisite_id}))
                check("前置任务验收残留为零", len(rows(target, prerequisite_title)) == 0)
            except Exception as exc:
                check(f"前置任务清理失败：{exc}", False)
        target.close()

    passed = sum(ok for _, ok in results)
    print(f"重复任务实机验收：{passed}/{len(results)}，profile={actual}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
