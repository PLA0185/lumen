"""数据库结构验证脚本（阶段 1 验收用）。

用途：独立于应用进程，直接检查 SQLite 文件，确认迁移真实执行、
约束真实生效。不依赖任何第三方库（仅标准库 sqlite3）。

用法：
    python tools/verify_db.py [数据库路径]
"""
import os
import sqlite3
import sys
from pathlib import Path

DEFAULT_DB = Path(os.environ["APPDATA"]) / "com.pla0185.aitodo" / "aitodo.db"

# 迁移必须创建的表（与 migrations/ 一一对应）
EXPECTED_TABLES = [
    "app_meta",
    "attachments",
    "categories",
    "focus_sessions",
    "projects",
    "reminders",
    "settings",
    "subtasks",
    "tags",
    "task_dependencies",
    "task_series",
    "task_series_segments",
    # migration 0002：单次跳过记录（§5「单次取消显示为该次跳过」）
    "task_series_skips",
    "task_tags",
    "tasks",
]

# 关键唯一索引：重复实例"稳定身份"的保证（§5）
EXPECTED_UNIQUE_INDEXES = [
    "idx_tasks_occurrence",
    "idx_projects_name_alive",
    "idx_tags_name_alive",
    "idx_categories_name_alive",
]


def main() -> int:
    db_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DB

    if not db_path.exists():
        print(f"[失败] 数据库不存在：{db_path}")
        return 1

    print(f"数据库：{db_path}")
    print(f"文件大小：{db_path.stat().st_size} 字节")
    for suffix in ("-wal", "-shm"):
        side = Path(str(db_path) + suffix)
        if side.exists():
            print(f"  伴随文件 {suffix}：{side.stat().st_size} 字节")

    # 用只读方式打开，避免干扰正在运行的应用
    con = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    cur = con.cursor()

    print()
    cur.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [r[0] for r in cur.fetchall()]
    print(f"表数量：{len(tables)}")
    for t in tables:
        print(f"  - {t}")

    missing = [t for t in EXPECTED_TABLES if t not in tables]
    print()
    if missing:
        print(f"[失败] 缺少表：{missing}")
    else:
        print(f"[通过] 全部 {len(EXPECTED_TABLES)} 张预期表均已创建")

    cur.execute("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")
    indexes = [r[0] for r in cur.fetchall()]
    missing_idx = [i for i in EXPECTED_UNIQUE_INDEXES if i not in indexes]
    print(f"索引数量：{len(indexes)}")
    if missing_idx:
        print(f"[失败] 缺少关键索引：{missing_idx}")
    else:
        print(f"[通过] 关键唯一索引齐备（含重复实例稳定身份索引）")

    cur.execute("SELECT name FROM sqlite_master WHERE type='trigger' ORDER BY name")
    triggers = [r[0] for r in cur.fetchall()]
    print(f"触发器数量：{len(triggers)}：{triggers}")

    print()
    cur.execute("PRAGMA journal_mode")
    print(f"journal_mode = {cur.fetchone()[0]}（期望 wal）")
    cur.execute("PRAGMA user_version")
    print(f"user_version = {cur.fetchone()[0]}")

    cur.execute("SELECT key, value FROM app_meta ORDER BY key")
    print("app_meta：")
    for k, v in cur.fetchall():
        print(f"  {k} = {v}")

    cur.execute("SELECT version, description, success FROM _sqlx_migrations ORDER BY version")
    print("已应用迁移：")
    for version, desc, success in cur.fetchall():
        flag = "成功" if success else "失败"
        print(f"  {version} {desc} [{flag}]")

    print()
    cur.execute("SELECT COUNT(*) FROM tasks")
    print(f"任务数：{cur.fetchone()[0]}")

    con.close()
    return 0 if not missing and not missing_idx else 1


if __name__ == "__main__":
    raise SystemExit(main())
