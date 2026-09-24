"""Verify recurring data survives an actual app restart and later maintenance."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import uuid
from pathlib import Path

import ui_drive as ui
from verify_recurrence import check, open_editor, real_click_visible, rows, select
from verify_remediation3 import click_sidebar, invoke_js, set_search, verify_isolated_profile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expect-data-dir", required=True)
    parser.add_argument("--phase", required=True, choices=("seed", "after"))
    args = parser.parse_args()
    target = ui.connect(PORT, want="main")
    allowed, _ = verify_isolated_profile(target, args)
    if not allowed:
        target.close()
        return 1
    profile = Path(args.expect_data_dir)
    state_file = profile / "restart-acceptance.json"
    passed = True

    def verify(label: str, ok: bool) -> None:
        nonlocal passed
        check(label, ok)
        passed = passed and ok

    try:
        if args.phase == "seed":
            title = f"ZZREC-RESTART-{uuid.uuid4().hex[:10]}"
            ui.real_click(target, 'button[title="新建可以按规则重复的任务"]')
            ui.set_react_input(target, "#rec-name", title)
            select(target, '[aria-labelledby="rec-title"] select[aria-label="重复频率"]', "daily")
            ui.set_react_input(target,
                               '[aria-labelledby="rec-title"] input[aria-label="首次发生日期"]',
                               dt.date.today().isoformat())
            real_click_visible(target, '[aria-labelledby="rec-title"] .modal__actions button:last-child')
            verify("重启前通过 UI 创建重复任务", target.wait_for("!document.querySelector('#rec-name')"))
            current = rows(target, title)
            if not current:
                raise RuntimeError("Recurring task not found after UI creation")
            task_id, series_id = current[0]["id"], current[0]["seriesId"]
            sub_title = f"重启前子任务-{uuid.uuid4().hex[:6]}"
            sub = target.eval(invoke_js("subtask_create", {"taskId": task_id, "title": sub_title}))
            source = profile / f"restart-copy-source-{uuid.uuid4().hex[:8]}.txt"
            source.write_text("Lumen restart preservation", encoding="utf-8")
            attachment = target.eval(invoke_js("attachment_add", {
                "taskId": task_id, "sourcePath": str(source), "mode": "copied",
            }))
            verify("重启前发生有子任务和副本附件", bool(sub) and bool(attachment))
            open_editor(target, title)
            real_click_visible(target, '[aria-labelledby="editor-title"] button[class*="btn--quiet"]')
            verify("重启前通过 UI 打开规则修改", target.wait_for("!!document.querySelector('#series-rule-title')"))
            target.eval("document.querySelectorAll('input[name=rule-scope]')[1].click()")
            select(target, '[aria-labelledby="series-rule-title"] select[aria-label="重复频率"]', "weekly")
            real_click_visible(target, '[aria-labelledby="series-rule-title"] .modal__actions button:last-child')
            verify("重启前通过 UI 保存新规则", target.wait_for("!document.querySelector('#series-rule-title')"))
            state_file.write_text(json.dumps({
                "title": title, "taskId": task_id, "seriesId": series_id,
                "subtaskId": sub["id"], "attachmentId": attachment["id"],
            }), encoding="utf-8")
        else:
            saved = json.loads(state_file.read_text(encoding="utf-8"))
            task_id, series_id = saved["taskId"], saved["seriesId"]
            click_sidebar(target, "全部任务")
            set_search(target, saved["title"])
            verify("重启后界面仍能找到原发生", target.wait_for(
                "!!document.querySelector('button[aria-label^=\"编辑\"]')")
                and any(row["id"] == task_id for row in rows(target, saved["title"])))
            detail = target.eval(invoke_js("recurring_get", {"seriesId": series_id}))
            verify("重启后新规则持久化", any(
                "FREQ=WEEKLY" in (seg.get("newRrule") or "") for seg in detail["segments"]))
            # Rule editing prebuilds a year. Probe beyond that horizon so this
            # command must actually extend the series after the restart.
            future = dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=500)
            result = target.eval(invoke_js("recurring_ensure_range", {
                "rangeStartUtc": future.isoformat(timespec="milliseconds").replace("+00:00", "Z"),
                "rangeEndUtc": (future + dt.timedelta(days=30)).isoformat(
                    timespec="milliseconds").replace("+00:00", "Z"),
            }))
            verify("重启后维护按新规则扩展未来范围", isinstance(result, int) and result > 0)
            again = target.eval(invoke_js("recurring_ensure_range", {
                "rangeStartUtc": future.isoformat(timespec="milliseconds").replace("+00:00", "Z"),
                "rangeEndUtc": (future + dt.timedelta(days=30)).isoformat(
                    timespec="milliseconds").replace("+00:00", "Z"),
            }))
            verify("重复维护不生成重复发生", again == 0)
            sub = target.eval(invoke_js("subtask_list", {"taskId": task_id})) or []
            att = target.eval(invoke_js("attachment_list", {"taskId": task_id})) or []
            verify("维护后子任务和附件仍绑定原发生", any(
                item["id"] == saved["subtaskId"] for item in sub) and any(
                item["id"] == saved["attachmentId"] for item in att))
            target.eval(invoke_js("attachment_cleanup_orphans", {}))
            copied = target.eval(invoke_js("attachment_reveal", {"id": saved["attachmentId"]}))
            verify("重启及孤儿清理后副本文件仍可访问", Path(copied).is_file())
            live = next((row for row in rows(target, saved["title"]) if not row.get("deletedAt")), None)
            if live:
                target.eval(invoke_js("recurring_delete", {
                    "taskId": live["id"], "mode": "whole_series", "confirmHistory": True,
                }))
            for row in rows(target, saved["title"]):
                target.eval(invoke_js("task_purge", {"id": row["id"]}))
            verify("重启验收任务残留为零", len(rows(target, saved["title"])) == 0)
    except Exception as exc:
        verify(f"重启验收执行失败：{type(exc).__name__}: {exc}", False)
    finally:
        target.close()
    print(f"重复任务重启验收 {args.phase}: {'通过' if passed else '失败'}", flush=True)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
