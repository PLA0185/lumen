"""周期跨度（period_type）的端到端验证（§4.1）。

验证目标：
1. 迁移 0004 确实给 tasks 表加了 period_type 列，且默认值为 'none'；
2. CHECK 约束生效——非法值会被数据库拒绝；
3. 可以创建/查询带周期跨度的任务，且**没有计划时间的周期任务**
   不会出现在「今天」查询里（这是设计要点，避免每天骚扰用户）；
4. 「周任务」「月任务」等视图的查询能正确命中。

用法（应用需已关闭，脚本直接改库）：
    python tools/verify_period.py migrate-check
    python tools/verify_period.py seed
    python tools/verify_period.py check
    python tools/verify_period.py cleanup
"""
import os
import sqlite3
import sys
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

DB = Path(os.environ["APPDATA"]) / "com.pla0185.aitodo" / "aitodo.db"
MARK = "AITODO-PERIOD-E2E"

PERIODS = ["day", "week", "month", "quarter", "year"]


def iso(dt: datetime) -> str:
    u = dt.astimezone(timezone.utc)
    return u.strftime("%Y-%m-%dT%H:%M:%S.") + f"{u.microsecond // 1000:03d}Z"


def connect(readonly: bool = False) -> sqlite3.Connection:
    if readonly:
        return sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    con = sqlite3.connect(str(DB), timeout=10)
    con.execute("PRAGMA foreign_keys = ON")
    return con


def migrate_check() -> int:
    con = connect(readonly=True)
    cur = con.cursor()

    cur.execute("PRAGMA table_info(tasks)")
    cols = {r[1]: r for r in cur.fetchall()}
    if "period_type" not in cols:
        print("[失败] tasks 表没有 period_type 列——迁移 0004 未生效")
        con.close()
        return 1
    name, ctype, notnull, default, pk = cols["period_type"][1], cols["period_type"][2], \
        cols["period_type"][3], cols["period_type"][4], cols["period_type"][5]
    print(f"[通过] 存在 period_type 列：类型={ctype} NOT NULL={notnull} 默认值={default!r}")
    if default != "'none'":
        print(f"[失败] 默认值应为 'none'，实际 {default!r}")
        con.close()
        return 1

    cur.execute("SELECT value FROM app_meta WHERE key = 'schema_version'")
    ver = cur.fetchone()
    print(f"[信息] schema_version = {ver[0] if ver else '未知'}")

    # 索引
    cur.execute("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_tasks_period'")
    if cur.fetchone():
        print("[通过] 周期索引 idx_tasks_period 存在")
    else:
        print("[警告] 未找到 idx_tasks_period（不影响功能，只影响查询性能）")

    con.close()
    return 0


def seed() -> int:
    """插入覆盖全部周期类型的任务，其中**不填计划时间**，用于验证查询口径。"""
    con = connect()
    cur = con.cursor()
    now = datetime.now(timezone.utc)
    ts = iso(now)

    cur.execute("DELETE FROM tasks WHERE title LIKE ?", (f"%{MARK}%",))

    ids = {}
    for p in PERIODS:
        tid = str(uuid.uuid4())
        cur.execute(
            """INSERT INTO tasks (id, title, description, note_md, status, priority,
                                  created_at, updated_at, sort_order, actual_minutes,
                                  occurrence_kind, is_exception, sync_rev, sync_state,
                                  has_planned_time, has_due_time, is_pinned, is_favorite,
                                  period_type)
               VALUES (?,?,?,?, 'todo', 0, ?, ?, 0, 0, 'single', 0, 0, 'local', 0, 0, 0, 0, ?)""",
            (tid, f"{MARK} {p}", "", "", ts, ts, p),
        )
        ids[p] = tid

    # 另加一个"有周期 + 有计划时间=今天"的任务，验证它仍会出现在今天视图
    tid = str(uuid.uuid4())
    today_start = now.replace(hour=1, minute=0, second=0, microsecond=0)
    cur.execute(
        """INSERT INTO tasks (id, title, description, note_md, status, priority,
                              created_at, updated_at, sort_order, actual_minutes,
                              occurrence_kind, is_exception, sync_rev, sync_state,
                              planned_at, has_planned_time, has_due_time, is_pinned, is_favorite,
                              period_type)
           VALUES (?,?,?,?, 'todo', 0, ?, ?, 0, 0, 'single', 0, 0, 'local', ?, 1, 0, 0, 0, 'week')""",
        (tid, f"{MARK} week-with-plan", "", "", ts, ts, iso(today_start)),
    )
    ids["week-with-plan"] = tid

    con.commit()
    con.close()

    print(f"已插入 {len(ids)} 个测试任务（均无截止时间，除 week-with-plan 外均无计划时间）")
    for k, v in ids.items():
        print(f"  {k:16s} {v[:8]}")
    return 0


def check() -> int:
    con = connect(readonly=True)
    cur = con.cursor()
    ok = True

    # 1) 每个周期类型都应能查到。
    #    注意排除 week-with-plan：它同样是 period_type='week'，
    #    但它是为验证"有计划时间时仍进今天视图"而故意多插的，不是计数目标。
    for p in PERIODS:
        cur.execute(
            """SELECT COUNT(*) FROM tasks
               WHERE deleted_at IS NULL AND period_type = ?
                 AND title LIKE ? AND title NOT LIKE ?""",
            (p, f"%{MARK}%", "%week-with-plan%"),
        )
        n = cur.fetchone()[0]
        if n == 1:
            print(f"[通过] period_type='{p}' 可查询，命中 {n} 条")
        else:
            print(f"[失败] period_type='{p}' 命中 {n} 条，期望 1")
            ok = False

    # 1b) 带计划时间的周期任务应能被单独查到
    cur.execute(
        """SELECT COUNT(*) FROM tasks
           WHERE deleted_at IS NULL AND period_type = 'week' AND title LIKE ?""",
        ("%week-with-plan%",),
    )
    n_plan = cur.fetchone()[0]
    if n_plan == 1:
        print("[通过] 带计划时间的周期任务可单独查到（周期与计划时间可共存）")
    else:
        print(f"[失败] week-with-plan 命中 {n_plan} 条，期望 1")
        ok = False

    # 2) 关键设计要点：没有计划时间的周期任务**不应**出现在「今天」查询里
    day_start = datetime.now(timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)
    day_end = day_start + timedelta(days=1)
    cur.execute(
        """SELECT COUNT(*) FROM tasks
           WHERE deleted_at IS NULL AND title LIKE ?
             AND planned_at IS NOT NULL AND planned_at >= ? AND planned_at < ?""",
        (f"%{MARK}%", iso(day_start), iso(day_end)),
    )
    today_hits = cur.fetchone()[0]
    # 期望只有 week-with-plan 那一条命中
    if today_hits == 1:
        print(f"[通过] 「今天」查询命中 {today_hits} 条——仅限**有计划时间**的那条；")
        print("       没有计划时间的周期任务不会出现在今天视图（避免每天骚扰）")
    else:
        print(f"[失败] 「今天」查询命中 {today_hits} 条，期望 1")
        ok = False

    # 3) 非法值必须被 CHECK 约束拒绝
    con2 = connect()
    try:
        con2.execute(
            """INSERT INTO tasks (id, title, status, created_at, updated_at, sort_order,
                                  actual_minutes, occurrence_kind, is_exception, sync_rev, sync_state,
                                  has_planned_time, has_due_time, is_pinned, is_favorite, period_type)
               VALUES (?,?, 'todo', ?, ?, 0, 0, 'single', 0, 0, 'local', 0, 0, 0, 0, 'weekly')""",
            (str(uuid.uuid4()), f"{MARK} illegal", iso(datetime.now(timezone.utc)),
             iso(datetime.now(timezone.utc))),
        )
        con2.commit()
        print("[失败] 非法 period_type='weekly' 竟被接受——CHECK 约束未生效")
        ok = False
    except sqlite3.IntegrityError as e:
        print(f"[通过] 非法 period_type 被拒绝：{e}")
    finally:
        con2.close()

    con.close()
    return 0 if ok else 1


def cleanup() -> int:
    con = connect()
    cur = con.cursor()
    cur.execute("DELETE FROM tasks WHERE title LIKE ?", (f"%{MARK}%",))
    n = cur.rowcount
    con.commit()
    con.close()
    print(f"已清理 {n} 个测试任务")
    return 0


if __name__ == "__main__":
    if not DB.exists():
        print(f"[失败] 数据库不存在：{DB}")
        raise SystemExit(1)
    action = sys.argv[1] if len(sys.argv) > 1 else "check"
    if action == "migrate-check":
        raise SystemExit(migrate_check())
    if action == "seed":
        raise SystemExit(seed())
    if action == "check":
        raise SystemExit(check())
    if action == "cleanup":
        raise SystemExit(cleanup())
    print(f"未知动作：{action}")
    raise SystemExit(1)
