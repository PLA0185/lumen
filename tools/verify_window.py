"""窗口配置注入与检查（阶段 4 实机验收用）。

用于在没有界面的情况下模拟"用户上次把界面设置得很隐蔽"，
验证启动时的安全恢复是否真的生效（任务书 §8 的安全底线：
不得让用户必须删除配置文件才能找回窗口）。

用法：
    python tools/verify_window.py set-floating-on   # 模拟上次开着悬浮窗
    python tools/verify_window.py set-click-through # 模拟上次开着穿透
    python tools/verify_window.py set-hidden-all    # 模拟所有可见入口都关掉
    python tools/verify_window.py show              # 打印当前配置
    python tools/verify_window.py check-recovered   # 验证启动后是否已恢复
    python tools/verify_window.py reset             # 清空为默认配置
"""
import json
import os
import sqlite3
import sys
from pathlib import Path

DB = Path(os.environ["APPDATA"]) / "com.pla0185.aitodo" / "aitodo.db"
KEY = "window_config"

DEFAULT = {
    "mainAlwaysOnTop": False,
    "mainShowInTaskbar": True,
    "closeAction": "tray",
    "floatingEnabled": False,
    "floatingAlwaysOnTop": True,
    "floatingClickThrough": False,
    "floatingOpacity": 1.0,
    "floatingShowInTaskbar": False,
    "floatingX": None,
    "floatingY": None,
    "trayEnabled": True,
    "shortcutEnabled": True,
    "shortcutToggle": "CmdOrCtrl+Alt+A",
    "shortcutQuickAdd": "CmdOrCtrl+Alt+N",
    "shortcutToday": "CmdOrCtrl+Alt+D",
}


def connect() -> sqlite3.Connection:
    con = sqlite3.connect(str(DB), timeout=10)
    con.execute("PRAGMA foreign_keys = ON")
    return con


def write_cfg(cfg: dict) -> None:
    con = connect()
    con.execute(
        "INSERT INTO settings (key, value_json, updated_at) VALUES (?, ?, datetime('now')) "
        "ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        (KEY, json.dumps(cfg, ensure_ascii=False)),
    )
    con.commit()
    con.close()


def read_cfg() -> dict | None:
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    cur = con.cursor()
    cur.execute("SELECT value_json FROM settings WHERE key = ?", (KEY,))
    row = cur.fetchone()
    con.close()
    return json.loads(row[0]) if row else None


def merged(overrides: dict) -> dict:
    cfg = read_cfg() or dict(DEFAULT)
    cfg.update(overrides)
    return cfg


def main() -> int:
    if not DB.exists():
        print(f"[失败] 数据库不存在：{DB}")
        return 1

    action = sys.argv[1] if len(sys.argv) > 1 else "show"

    if action == "show":
        cfg = read_cfg()
        print(json.dumps(cfg, ensure_ascii=False, indent=2) if cfg else "(尚未写入 window_config)")
        return 0

    if action == "set-floating-on":
        write_cfg(merged({"floatingEnabled": True}))
        print("已写入：floatingEnabled = true（下次启动应恢复悬浮窗）")
        return 0

    if action == "set-click-through":
        write_cfg(merged({"floatingEnabled": True, "floatingClickThrough": True}))
        print("已写入：floatingEnabled = true 且 floatingClickThrough = true")
        print("预期：启动后穿透应被安全恢复关闭（floatingClickThrough 变回 false），")
        print("      但 floatingEnabled 保持 true（悬浮窗仍显示）")
        return 0

    if action == "set-hidden-all":
        # 模拟最坏情况：任务栏图标隐藏 + 托盘关闭 + 主窗口置顶但不可见
        # 注意：启动时的 safe_recovery 会把主窗口显示出来，但不会改托盘/任务栏
        write_cfg(
            merged(
                {
                    "mainShowInTaskbar": False,
                    "trayEnabled": False,
                    "shortcutEnabled": True,
                    "floatingEnabled": True,
                    "floatingClickThrough": True,
                }
            )
        )
        print("已写入：任务栏图标隐藏 + 托盘关闭 + 悬浮窗穿透开启")
        print("预期：启动后主窗口仍会显示（safe_recovery 保证），穿透被关闭，")
        print("      并且快捷键仍是可用的恢复入口")
        return 0

    if action == "check-recovered":
        cfg = read_cfg()
        if cfg is None:
            print("[失败] 没有 window_config")
            return 1
        ok = True
        print(f"floatingClickThrough = {cfg.get('floatingClickThrough')}")
        if cfg.get("floatingClickThrough"):
            print("[失败] 穿透未被安全恢复关闭——用户可能无法点击悬浮窗")
            ok = False
        else:
            print("[通过] 穿透已在启动时关闭")

        if cfg.get("floatingEnabled"):
            print("[通过] 悬浮窗仍按用户意愿启用")

        # 恢复路径检查
        has = bool(cfg.get("trayEnabled")) or bool(cfg.get("shortcutEnabled"))
        print(f"托盘={cfg.get('trayEnabled')} 快捷键={cfg.get('shortcutEnabled')} → 恢复路径={has}")
        if not has:
            print("[失败] 没有任何恢复路径")
            ok = False
        else:
            print("[通过] 至少存在一条恢复路径")

        return 0 if ok else 1

    if action == "reset":
        write_cfg(dict(DEFAULT))
        print("已重置为默认窗口配置")
        return 0

    print(f"未知动作：{action}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
