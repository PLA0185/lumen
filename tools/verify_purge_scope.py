"""回收站清空范围的实机验收（第三轮修正）。

## 复现的缺陷

界面上「永久删除 N 项」的 N 来自**当前筛选条件**下的后端计数
（在回收站里搜个词就只剩匹配的那几条），而后端原先执行的是无条件的
`DELETE FROM tasks WHERE deleted_at IS NOT NULL`。

结果：搜索状态下点清空，弹窗说「永久删除 30 项」，实际删掉整个回收站。

## 这个脚本怎么证明修好了

造两组回收站任务：

- **实验组**：标题含 `ZZPURGE`（30 条）—— 会被搜索命中；
- **对照组**：标题含 `ZZKEEP`（30 条）—— 不该被这次操作碰到。

然后在回收站里搜索 `ZZPURGE`，断言：

1. 按钮与弹窗里的数字都是 **30**（不是 60）；
2. 弹窗明确写出"只删除筛选结果里的 30 项、回收站共 60 项、其余会保留"；
3. 真正执行后，对照组 30 条**仍在**回收站里。

第 3 条在修复前必然失败——这正是这条验收的价值。

## ⚠️ 一次真实事故（本脚本第一版踩的坑，务必别再踩）

第一版用"替换 `window.confirm` → 点按钮 → 立刻恢复 `window.confirm`"来读弹窗文案。
但按钮的处理函数是 **async** 的（先 `await` 一次计数查询，之后才弹 confirm），
于是：

1. 同步恢复到原生 `confirm`；
2. 异步流程随后调用**原生** confirm —— 无头环境下它被自动接受（返回 true）；
3. 脚本此时已经进入 `finally` 并清空了搜索框，于是那次删除变成了
   **无条件清空回收站**：用户原有的 18 条回收站任务被永久删除。

数据后来从迁移前快照里逐行恢复（见 `docs/work-log.md` 第 9 轮）。
现在的做法是**全程接管** `window.confirm`：

- 脚本一开始就把 `window.confirm` 换成自己的实现，默认返回 `false`（阻止一切真实删除）；
- 只有明确要执行删除的那一步，才把模式切到 `accept`，执行完立刻切回 `block`；
- 直到脚本结束才还原原生 confirm。

**任何会点"确认类"按钮的验收脚本都应采用这个模式**：
异步 UI 里没有"读过就算完"的同步拦截。

## 安全

- 只操作自己造的 `ZZPURGE-` / `ZZKEEP-` 前缀数据，结束时全部清理；
- 前置断言不通过就绝不下发删除，因此这个脚本可以安全地在任何版本上运行；
- 结束时断言用户原有数据未被改动。

用法：`python tools/verify_purge_scope.py`
前提：Lumen 正在运行，且启动时设置了
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`。
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))

GROUP = 30  # 实验组与对照组各造多少条
HIT_PREFIX = "ZZPURGE"
MISS_PREFIX = "ZZKEEP"

results: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail: str = "") -> bool:
    results.append((name, ok, detail))
    print(f"{'✅' if ok else '❌'} {name}" + (f" —— {detail}" if detail else ""), flush=True)
    return ok


def invoke_js(cmd: str, args: dict) -> str:
    return "window.__TAURI_INTERNALS__.invoke(%s, %s)" % (
        json.dumps(cmd),
        json.dumps(args, ensure_ascii=False),
    )


def count(main: ui.Target, query: dict) -> int:
    return main.eval(invoke_js("task_count", {"query": query}))["total"]


def create_tasks(main: ui.Target, prefix: str, n: int) -> list[str]:
    js = """
    (async () => {
      const ids = [];
      for (let i = 1; i <= %d; i++) {
        const t = await window.__TAURI_INTERNALS__.invoke('task_create', {
          input: { title: %s + '-' + i, status: 'todo' }
        });
        ids.push(t.id);
      }
      await window.__TAURI_INTERNALS__.invoke('task_bulk', {
        input: { ids: ids, action: 'delete' }   // 直接进回收站
      });
      return ids;
    })()
    """ % (
        n,
        json.dumps(prefix),
    )
    return main.eval(js)


def purge_ids(main: ui.Target, ids: list[str], batch: int = 50) -> int:
    removed = 0
    for i in range(0, len(ids), batch):
        chunk = ids[i : i + batch]
        js = """
        (async () => {
          let ok = 0;
          await Promise.all(%s.map(async (id) => {
            try { await window.__TAURI_INTERNALS__.invoke('task_purge', { id }); ok++; }
            catch (e) { /* 已经不在回收站里就忽略 */ }
          }));
          return ok;
        })()
        """ % json.dumps(chunk)
        removed += int(main.eval(js) or 0)
    return removed


def click_sidebar(main: ui.Target, label: str) -> None:
    main.eval(
        """
        (() => {
          const btn = Array.from(document.querySelectorAll('.sidebar button'))
            .find(b => b.innerText.includes(%s));
          if (!btn) throw new Error('侧边栏没有：' + %s);
          btn.click();
          return true;
        })()
        """
        % (json.dumps(label), json.dumps(label))
    )
    time.sleep(1.0)


def set_search(main: ui.Target, text: str) -> None:
    ui.set_react_input(main, 'input[type="search"]', text)
    time.sleep(1.5)


# ---------------------------------------------------------------------------
# confirm 接管：全程不撒手，直到脚本结束
# ---------------------------------------------------------------------------

ARM_CONFIRM = """
(() => {
  window.__lumen_confirm_text = null;
  window.__lumen_confirm_mode = 'block';
  if (!window.__lumen_orig_confirm) {
    window.__lumen_orig_confirm = window.confirm;
  }
  window.confirm = (msg) => {
    window.__lumen_confirm_text = msg;
    return window.__lumen_confirm_mode === 'accept';
  };
  return true;
})()
"""


def arm_confirm(main: ui.Target) -> None:
    """接管 window.confirm，默认拒绝（阻止一切真实删除）。"""
    main.eval(ARM_CONFIRM)


def set_confirm_mode(main: ui.Target, mode: str) -> None:
    main.eval("window.__lumen_confirm_mode = %s; true" % json.dumps(mode))


def release_confirm(main: ui.Target) -> None:
    main.eval(
        """
        (() => {
          if (window.__lumen_orig_confirm) window.confirm = window.__lumen_orig_confirm;
          return true;
        })()
        """
    )


def click_danger(main: ui.Target) -> None:
    main.eval(
        """
        (() => {
          const b = document.querySelector('.btn--danger');
          if (!b) throw new Error('没有找到清空回收站按钮');
          b.click();
          return true;
        })()
        """
    )


def wait_confirm_text(main: ui.Target, timeout: float = 8.0) -> str | None:
    """等按钮的异步流程走到 confirm（它前面有一次 await 计数查询）。"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        text = main.eval("window.__lumen_confirm_text")
        if text:
            return str(text)
        time.sleep(0.2)
    return None


def main() -> int:
    print(f"连接主窗口（调试端口 {PORT}）…", flush=True)
    main_win = ui.connect(PORT, want="main")
    print("已连接。\n", flush=True)

    base_trash = count(main_win, {"deletedOnly": True, "statuses": []})
    base_all = count(main_win, {"includeDeleted": True, "statuses": []})
    print(f"基线：回收站 {base_trash} 条，全部（含回收站）{base_all} 条\n", flush=True)

    hit_ids: list[str] = []
    miss_ids: list[str] = []
    armed = False
    try:
        # 第一件事就是接管 confirm：此后**任何**确认框都不会被自动接受
        arm_confirm(main_win)
        armed = True

        print(
            f"造数据：{GROUP} 条命中组（{HIT_PREFIX}）+ {GROUP} 条对照组（{MISS_PREFIX}）…",
            flush=True,
        )
        hit_ids = create_tasks(main_win, HIT_PREFIX, GROUP)
        miss_ids = create_tasks(main_win, MISS_PREFIX, GROUP)
        trash_now = count(main_win, {"deletedOnly": True, "statuses": []})
        check(
            "两组数据都已进入回收站",
            trash_now == base_trash + GROUP * 2,
            f"回收站 {base_trash} → {trash_now} 条",
        )

        # ---- 界面：搜索命中组，看按钮与弹窗 ----
        click_sidebar(main_win, "回收站")
        set_search(main_win, HIT_PREFIX)

        filtered = count(main_win, {"deletedOnly": True, "search": HIT_PREFIX, "statuses": []})
        all_trash = count(main_win, {"deletedOnly": True, "statuses": []})
        check(
            "筛选后的计数小于回收站总数",
            filtered == GROUP and all_trash == base_trash + GROUP * 2,
            f"筛选 {filtered} 条 / 回收站共 {all_trash} 条",
        )

        btn = (
            main_win.eval(
                "(() => { const b = document.querySelector('.btn--danger'); return b ? b.innerText : null; })()"
            )
            or ""
        )
        btn_ok = check(
            "按钮数量 = 筛选后的条数（不是回收站总数）",
            f"{GROUP}" in btn,
            f"按钮文案 {btn!r}，期望含 {GROUP}",
        )

        click_danger(main_win)
        confirm_text = wait_confirm_text(main_win) or ""
        confirm_ok = check(
            "弹窗说明只删筛选结果、并告知会保留多少",
            f"{GROUP}" in confirm_text and f"{all_trash}" in confirm_text,
            f"弹窗文本：{confirm_text!r}",
        )

        # ---- 前置断言都过了才真的删 ----
        if btn_ok and confirm_ok:
            before = count(main_win, {"deletedOnly": True, "statuses": []})
            set_confirm_mode(main_win, "accept")
            click_danger(main_win)
            # 等删除真的发生（最多 10 秒），期间 confirm 仍然是我们的实现
            deadline = time.time() + 10
            after = before
            while time.time() < deadline:
                after = count(main_win, {"deletedOnly": True, "statuses": []})
                if after < before:
                    break
                time.sleep(0.4)
            set_confirm_mode(main_win, "block")

            hit_left = count(
                main_win, {"deletedOnly": True, "search": HIT_PREFIX, "statuses": []}
            )
            miss_left = count(
                main_win, {"deletedOnly": True, "search": MISS_PREFIX, "statuses": []}
            )
            check("命中组已被永久删除", hit_left == 0, f"仍剩 {hit_left} 条")
            check(
                "对照组原封不动（修复前这里会被一起删掉）",
                miss_left == GROUP,
                f"对照组仍剩 {miss_left}/{GROUP} 条",
            )
            check(
                "回收站总数只减少了命中组那么多",
                after == before - GROUP,
                f"{before} → {after} 条",
            )
        else:
            check("跳过真实删除（前置断言未通过，避免误删用户数据）", False, "见上面的失败项")

    finally:
        print("\n清理脚本造的数据…", flush=True)
        try:
            if armed:
                # 清理期间也保持接管：任何残留的异步确认框都只会被拒绝
                set_confirm_mode(main_win, "block")
            set_search(main_win, "")
            click_sidebar(main_win, "回收站")
            purge_ids(main_win, hit_ids)
            purge_ids(main_win, miss_ids)
            set_search(main_win, "")
            click_sidebar(main_win, "今天")
            if armed:
                release_confirm(main_win)
        except Exception as e:  # noqa: BLE001
            print(f"⚠️ 清理出错（请手工检查 ZZPURGE/ZZKEEP 前缀）：{e}", flush=True)

    left_hit = count(main_win, {"search": HIT_PREFIX, "includeDeleted": True, "statuses": []})
    left_miss = count(main_win, {"search": MISS_PREFIX, "includeDeleted": True, "statuses": []})
    check("脚本数据已清空", left_hit == 0 and left_miss == 0, f"剩 {left_hit} / {left_miss} 条")

    now_trash = count(main_win, {"deletedOnly": True, "statuses": []})
    now_all = count(main_win, {"includeDeleted": True, "statuses": []})
    check(
        "用户原有数据未被改动",
        now_trash == base_trash and now_all == base_all,
        f"回收站 {base_trash}→{now_trash}，全部 {base_all}→{now_all}",
    )

    passed = sum(1 for _, ok, _ in results if ok)
    print(f"\n===== {passed}/{len(results)} 项通过 =====", flush=True)
    for name, ok, detail in results:
        if not ok:
            print(f"❌ {name} —— {detail}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
