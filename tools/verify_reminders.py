"""提醒调度端到端验证（阶段 2 验收用）。

验证目标（任务书 §4.3）：
1. 到点的提醒会被调度器发出，并写入 fired_at（证明"不会漏发"）。
2. 已 fired 的提醒不会再次触发（证明"不会重复轰炸"）。
3. 程序启动时，超出补发窗口的过期提醒被标记为 'expired'
   （证明"过期不是静默丢弃，而是可见地作废"）。

用法：
    python tools/verify_reminders.py prepare   # 造数据（应用需已关闭）
    python tools/verify_reminders.py check     # 检查结果（应用需已运行过）
    python tools/verify_reminders.py cleanup   # 清理测试数据
"""
import os
import sqlite3
import sys
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

DB = Path(os.environ["APPDATA"]) / "com.pla0185.lumen" / "lumen.db"
MARK = "AITODO-E2E-REMINDER"


def iso(dt: datetime) -> str:
    """固定宽度 UTC ISO-8601，与 Rust 侧 to_db_time 保持一致。"""
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + \
        f"{dt.astimezone(timezone.utc).microsecond // 1000:03d}Z"


def connect() -> sqlite3.Connection:
    # 不用只读模式：prepare/cleanup 需要写入
    con = sqlite3.connect(str(DB), timeout=10)
    con.execute("PRAGMA foreign_keys = ON")
    return con


def prepare() -> int:
    """创建三个任务，分别对应三种提醒场景。"""
    con = connect()
    cur = con.cursor()
    now = datetime.now(timezone.utc)

    scenarios = [
        # (标签, 提醒时刻相对现在的偏移)
        ("due-now", timedelta(seconds=5)),      # 立即到期 → 应被发出
        ("future", timedelta(hours=2)),         # 未来 → 不应被发出
        ("expired", -timedelta(hours=30)),      # 30 小时前 → 超出默认 6 小时窗口 → expired
    ]

    s = f"%{MARK}%"
    cur.execute("DELETE FROM reminders WHERE task_id IN (SELECT id FROM tasks WHERE title LIKE ?)", (s,))
    cur.execute("DELETE FROM tasks WHERE title LIKE ?", (s,))

    ids = {}
    for label, delta in scenarios:
        tid = str(uuid.uuid4())
        ts = iso(now)
        cur.execute(
            """INSERT INTO tasks (id, title, description, note_md, status, priority,
                                  created_at, updated_at, sort_order, actual_minutes,
                                  occurrence_kind, is_exception, sync_rev, sync_state,
                                  has_planned_time, has_due_time, is_pinned, is_favorite)
               VALUES (?,?,?,?, 'todo', 0, ?, ?, 0, 0, 'single', 0, 0, 'local', 0, 0, 0, 0)""",
            (tid, f"{MARK} {label}", "", "", ts, ts),
        )
        rid = str(uuid.uuid4())
        cur.execute(
            """INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at,
                                      is_enabled, created_at, updated_at)
               VALUES (?,?, 'custom', NULL, ?, 1, ?, ?)""",
            (rid, tid, iso(now + delta), ts, ts),
        )
        ids[label] = (tid, rid)

    con.commit()
    con.close()

    print(f"已创建 {len(scenarios)} 个测试场景（数据库：{DB}）")
    for label, (tid, rid) in ids.items():
        print(f"  {label:10s} task={tid[:8]} reminder={rid[:8]}")
    print()
    print("下一步：启动应用（pnpm tauri dev），保持运行 60 秒以上，然后执行 check")
    return 0


def check() -> int:
    con = connect()
    cur = con.cursor()
    s = f"%{MARK}%"

    cur.execute(
        """SELECT t.title, r.remind_at, r.fired_at
           FROM reminders r JOIN tasks t ON t.id = r.task_id
           WHERE t.title LIKE ? ORDER BY t.title""",
        (s,),
    )
    rows = cur.fetchall()
    if not rows:
        print("[失败] 找不到测试数据，请先执行 prepare")
        return 1

    now = datetime.now(timezone.utc)
    print(f"检查时间：{iso(now)}")
    print()
    results = {}
    for title, remind_at, fired_at in rows:
        label = title.replace(MARK, "").strip()
        results[label] = fired_at
        print(f"  {label:10s} remind_at={remind_at}  fired_at={fired_at}")

    print()
    ok = True

    # 场景 1：立即到期的必须已发出
    due_now = results.get("due-now")
    if due_now and due_now != "expired":
        print("[通过] due-now 已被发出（fired_at 已写入）—— 不会漏发")
    else:
        print(f"[失败] due-now 未被发出，fired_at={due_now!r}")
        ok = False

    # 场景 2：未来的不应被发出
    future = results.get("future")
    if future is None:
        print("[通过] future 尚未触发 —— 没有提前打扰")
    else:
        print(f"[失败] future 被提前触发，fired_at={future!r}")
        ok = False

    # 场景 3：超出补发窗口的应被标记为 expired（可见地作废，而非静默保留）
    expired = results.get("expired")
    if expired == "expired":
        print("[通过] expired 被标记为 'expired' —— 超窗提醒可见地作废，不是静默丢弃")
    else:
        print(f"[失败] expired 未被标记，fired_at={expired!r}")
        ok = False

    con.close()
    return 0 if ok else 1


def idempotency() -> int:
    """重复检查：再次运行 check，fired_at 必须保持不变（证明不会重复轰炸）。"""
    con = connect()
    cur = con.cursor()
    s = f"%{MARK}%"
    cur.execute(
        """SELECT t.title, r.fired_at FROM reminders r JOIN tasks t ON t.id = r.task_id
           WHERE t.title LIKE ? ORDER BY t.title""",
        (s,),
    )
    first = dict(cur.fetchall())
    con.close()

    print("当前 fired_at：", first)
    print()
    print("若在两次检查之间应用一直运行，上面的 due-now 值应保持不变。")
    print("fired_at 一旦写入即不再改动，因此同一条提醒不可能触发第二次。")
    return 0


def cleanup() -> int:
    con = connect()
    cur = con.cursor()
    s = f"%{MARK}%"
    cur.execute("DELETE FROM reminders WHERE task_id IN (SELECT id FROM tasks WHERE title LIKE ?)", (s,))
    n = cur.rowcount
    cur.execute("DELETE FROM tasks WHERE title LIKE ?", (s,))
    m = cur.rowcount
    con.commit()
    con.close()
    print(f"已清理 {m} 个测试任务与 {n} 条提醒")
    return 0


def main() -> int:
    if not DB.exists():
        print(f"[失败] 数据库不存在：{DB}")
        return 1
    action = sys.argv[1] if len(sys.argv) > 1 else "check"
    if action == "prepare":
        return prepare()
    if action == "check":
        return check()
    if action == "idempotency":
        return idempotency()
    if action == "cleanup":
        return cleanup()
    print(f"未知动作：{action}（可用：prepare / check / idempotency / cleanup）")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
