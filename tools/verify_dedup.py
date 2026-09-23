"""新增一条「刚刚到期」的提醒，用于验证去重（不重复轰炸）。

在应用**运行中**执行本脚本：插入一条 remind_at 略早于现在的提醒。
调度器下一个 tick 应发出并写入 fired_at；此后无论再经过多少个 tick，
fired_at 都不应再变化——这就是 §4.3「避免重复轰炸」的核心保证。

用法：
    python tools/verify_reminders.py insert-now
"""
import os
import sqlite3
import sys
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

DB = Path(os.environ["APPDATA"]) / "com.pla0185.lumen" / "lumen.db"
MARK = "AITODO-DEDUP"


def iso(dt: datetime) -> str:
    u = dt.astimezone(timezone.utc)
    return u.strftime("%Y-%m-%dT%H:%M:%S.") + f"{u.microsecond // 1000:03d}Z"


def main() -> int:
    con = sqlite3.connect(str(DB), timeout=10)
    con.execute("PRAGMA foreign_keys = ON")
    cur = con.cursor()
    now = datetime.now(timezone.utc)
    ts = iso(now)

    tid = str(uuid.uuid4())
    cur.execute(
        """INSERT INTO tasks (id, title, description, note_md, status, priority,
                              created_at, updated_at, sort_order, actual_minutes,
                              occurrence_kind, is_exception, sync_rev, sync_state,
                              has_planned_time, has_due_time, is_pinned, is_favorite)
           VALUES (?,?,?,?, 'todo', 0, ?, ?, 0, 0, 'single', 0, 0, 'local', 0, 0, 0, 0)""",
        (tid, f"{MARK} 去重验证", "", "", ts, ts),
    )
    rid = str(uuid.uuid4())
    cur.execute(
        """INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at,
                                  is_enabled, created_at, updated_at)
           VALUES (?,?, 'custom', NULL, ?, 1, ?, ?)""",
        (rid, tid, iso(now - timedelta(seconds=2)), ts, ts),
    )
    con.commit()
    con.close()

    print(f"已插入「刚刚到期」的提醒 {rid[:8]}（任务 {tid[:8]}）")
    print("它应在下一个调度 tick（≤30 秒）被发出。")
    print("记录当前的 fired_at=空，之后重复执行 check-dedup 观察它是否变化。")
    return 0


def check_dedup() -> int:
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    cur = con.cursor()
    cur.execute(
        """SELECT r.id, r.fired_at FROM reminders r JOIN tasks t ON t.id = r.task_id
           WHERE t.title LIKE ?""",
        (f"%{MARK}%",),
    )
    rows = cur.fetchall()
    con.close()
    if not rows:
        print("[失败] 找不到去重验证数据")
        return 1
    for rid, fired in rows:
        print(f"  reminder={rid[:8]}  fired_at={fired}")
    return 0


def cleanup() -> int:
    con = sqlite3.connect(str(DB), timeout=10)
    cur = con.cursor()
    cur.execute(
        "DELETE FROM reminders WHERE task_id IN (SELECT id FROM tasks WHERE title LIKE ?)",
        (f"%{MARK}%",),
    )
    cur.execute("DELETE FROM tasks WHERE title LIKE ?", (f"%{MARK}%",))
    n = cur.rowcount
    con.commit()
    con.close()
    print(f"已清理 {n} 个去重测试任务")
    return 0


if __name__ == "__main__":
    action = sys.argv[1] if len(sys.argv) > 1 else "insert-now"
    if action == "insert-now":
        raise SystemExit(main())
    if action == "check-dedup":
        raise SystemExit(check_dedup())
    if action == "cleanup":
        raise SystemExit(cleanup())
    print(f"未知动作：{action}")
    raise SystemExit(1)
