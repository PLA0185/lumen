"""第二轮整改的实机验收（《Lumen 第二轮整改任务书》§4 / §5 / §6 / §11）。

## 为什么必须实机跑

前端分页、回收站确认数量、附件清理这三件事都有**单元测试覆盖不到的缝隙**：

- 单元测试测的是 store 的状态机，测不到"界面到底渲染了多少张卡片"；
- 回收站确认数量是"后端 count ↔ 界面文案"这条跨层链路，
  只有真机才能看到文案里的数字是不是后端那个数；
- 附件清理要动真实文件系统。

## 安全约定（重要）

脚本会在**真实数据库**里临时造数据，因此：

- 所有造出来的任务标题都带前缀 `ZZVERIFY2`，与用户数据不会混淆；
- 结束时在 `finally` 里**逐个按 id 永久删除**（不用"清空回收站"，
  那会删掉用户自己回收站里的东西）；
- 最后断言这批数据归零，并把界面恢复到进入前的状态。

用法：`python tools/verify_remediation2.py`
前提：Lumen 正在运行，且启动时设置了
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`。
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

# Windows 控制台默认是 GBK，直接 print ✅/❌ 会抛 UnicodeEncodeError，
# 而那个异常发生在 check() 里，会把整个验收流程打断（第一次跑就是这样）。
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))

# 一页加载多少条，必须与前端 `PAGE_SIZE`（src/lib/store.ts）一致
PAGE_SIZE = 200
# 本轮要造多少条：必须**远大于**修复前的 500 上限，才能证明"第 501 条可达"
SEED = 1200
PREFIX = "ZZVERIFY2"

results: list[tuple[str, bool, str]] = []


def check(name: str, ok: bool, detail: str = "") -> bool:
    results.append((name, ok, detail))
    print(f"{'✅' if ok else '❌'} {name}" + (f" —— {detail}" if detail else ""), flush=True)
    return ok


def invoke_js(cmd: str, args: dict) -> str:
    """一段"调用后端命令并返回结果"的 JS 片段（在页面上下文里执行）。"""
    return (
        "window.__TAURI_INTERNALS__.invoke(%s, %s)"
        % (json.dumps(cmd), json.dumps(args, ensure_ascii=False))
    )


def count_tasks(main: ui.Target, query: dict) -> int:
    return main.eval(invoke_js("task_count", {"query": query}))["total"]


def seed_tasks(main: ui.Target, n: int, batch: int = 400) -> list[str]:
    """批量创建带前缀的任务，返回 id 列表。

    分批发是为了避免单次 `Runtime.evaluate` 太久（CDP 默认 30s 超时），
    也让进度可见。
    """
    ids: list[str] = []
    done = 0
    while done < n:
        take = min(batch, n - done)
        js = """
        (async () => {
          const ids = [];
          for (let i = 0; i < %d; i++) {
            const t = await window.__TAURI_INTERNALS__.invoke('task_create', {
              input: { title: %s + '-' + (done_index++), status: 'todo' }
            });
            ids.push(t.id);
          }
          return ids;
        })()
        """ % (
            take,
            json.dumps(PREFIX),
        )
        # done_index 在页面里维护，避免拼 1200 个字符串字面量
        main.eval("window.__lumen_done_index = window.__lumen_done_index || 0; true")
        got = main.eval(js.replace("done_index++", "window.__lumen_done_index++"))
        ids.extend(got)
        done += take
        print(f"   已创建 {done}/{n}", flush=True)
    return ids


def purge_ids(main: ui.Target, ids: list[str], batch: int = 100) -> int:
    """逐个永久删除（分批并发）。返回成功删除的数量。

    刻意**不用** `task_purge_all_deleted`：那会把用户自己回收站里的任务
    一起删掉。这里只删脚本自己造的那些 id。
    """
    removed = 0
    for i in range(0, len(ids), batch):
        chunk = ids[i : i + batch]
        js = """
        (async () => {
          let ok = 0;
          await Promise.all(%s.map(async (id) => {
            try { await window.__TAURI_INTERNALS__.invoke('task_purge', { id }); ok++; }
            catch (e) { /* 已经被删掉的忽略 */ }
          }));
          return ok;
        })()
        """ % json.dumps(chunk)
        removed += int(main.eval(js) or 0)
    return removed


def card_count(main: ui.Target) -> int:
    return int(main.eval("document.querySelectorAll('.task').length") or 0)


def card_titles(main: ui.Target, n: int = 50) -> list[str]:
    """取前 n 张卡片的标题。

    刻意用标题而不是 innerText：卡片上还有"逾期 3 天""还有 2 小时"这类
    **会随时间变化**的文案，拿它做顺序比较会假失败。
    """
    return (
        main.eval(
            "Array.from(document.querySelectorAll('.task .task__title'))"
            ".slice(0, %d).map(e => e.innerText)" % n
        )
        or []
    )


def scroll_to_load_more(main: ui.Target) -> None:
    """把「加载更多」滚进视口。

    这一步是必须的：`Input.dispatchMouseEvent` 用的是**视口坐标**，
    而"加载更多"在 200 张卡片之后、远在视口外，直接点会落到空处
    （第一次跑实机验收就踩了这个坑，表现为"点了没反应"）。
    """
    main.eval(
        """
        (() => {
          const b = document.querySelector('.loadmore button');
          if (!b) throw new Error('找不到「加载更多」按钮');
          b.scrollIntoView({ block: 'center' });
          return true;
        })()
        """
    )
    time.sleep(0.6)


def wait_cards(main: ui.Target, want: int, timeout: float = 30.0) -> int:
    """等卡片数量达到 want（界面刷新是异步的）。"""
    deadline = time.time() + timeout
    got = 0
    while time.time() < deadline:
        got = card_count(main)
        if got >= want:
            return got
        time.sleep(0.4)
    return got


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


def main() -> int:
    print(f"连接主窗口（调试端口 {PORT}）…", flush=True)
    main_win = ui.connect(PORT, want="main")
    print("已连接。\n", flush=True)

    baseline_all = count_tasks(main_win, {"includeDeleted": True, "statuses": []})
    baseline_trash = count_tasks(main_win, {"deletedOnly": True, "statuses": []})
    print(f"基线：全部任务（含回收站）{baseline_all} 条，回收站 {baseline_trash} 条\n", flush=True)

    ids: list[str] = []
    soft_deleted = False
    try:
        # ---------------- 造数据 ----------------
        print(f"创建 {SEED} 条「{PREFIX}」任务…", flush=True)
        t0 = time.time()
        ids = seed_tasks(main_win, SEED)
        check("造数据完成", len(ids) == SEED, f"{len(ids)}/{SEED} 条，耗时 {time.time() - t0:.1f}s")
        if len(ids) != SEED:
            return finish(main_win, ids, baseline_all, baseline_trash)

        # ---------------- §4 分页 ----------------
        click_sidebar(main_win, "全部任务")
        set_search(main_win, PREFIX)

        # 用**与界面一致**的条件查总数（否则比较的是两套筛选，会假失败）
        ui_query = {"search": PREFIX, "statuses": ["todo", "doing", "waiting", "done"]}
        total = count_tasks(main_win, ui_query)
        check("搜索定位到这批数据", total >= SEED, f"后端 count = {total}")

        rendered = wait_cards(main_win, PAGE_SIZE)
        check(
            "首屏没有一次渲染全部（分页生效）",
            rendered == PAGE_SIZE,
            f"DOM 里 {rendered} 张卡片，远小于 {total}",
        )

        note = ui.text_of(main_win, ".loadmore__note") or ""
        check(
            "界面明确写出「已显示 X / 共 Y」",
            f"{PAGE_SIZE}" in note and f"{total}" in note,
            f"实际文案：{note!r}（后端 count = {total}）",
        )

        more = ui.text_of(main_win, ".loadmore button") or ""
        check("存在「加载更多」入口", "加载更多" in more, f"按钮文案：{more!r}")

        # 滚动到底部：应当自动加载下一页（IntersectionObserver）
        before_scroll = card_count(main_win)
        scroll_to_load_more(main_win)
        time.sleep(2.0)
        after_scroll = card_count(main_win)
        check(
            "滚动到底部会自动加载下一页",
            after_scroll > before_scroll,
            f"{before_scroll} → {after_scroll} 张卡片",
        )

        # 再用真实鼠标事件点按钮（不是 element.click()）
        if after_scroll <= before_scroll:
            # 自动加载没生效时，至少按钮必须是能用的
            ui.real_click(main_win, ".loadmore button")
        else:
            scroll_to_load_more(main_win)
            ui.real_click(main_win, ".loadmore button")
        after_click = wait_cards(main_win, after_scroll + 1, timeout=20)
        check(
            "点「加载更多」能继续往下取",
            after_click > after_scroll,
            f"{after_scroll} → {after_click} 张卡片",
        )

        # 排序稳定性：同一条件重新加载后，首屏顺序必须一致
        first_titles = card_titles(main_win)
        click_sidebar(main_win, "今天")
        click_sidebar(main_win, "全部任务")
        wait_cards(main_win, PAGE_SIZE)
        again = card_titles(main_win)
        check(
            "重新加载后首屏顺序稳定",
            first_titles == again and len(first_titles) == 50,
            f"前 {len(first_titles)} 张卡片逐项一致",
        )

        # ---------------- §5 回收站确认数量 ----------------
        js = """
        (async () => {
          return await window.__TAURI_INTERNALS__.invoke('task_bulk', {
            input: { ids: %s, action: 'delete' }
          });
        })()
        """ % json.dumps(ids)
        affected = main_win.eval(js)
        soft_deleted = True
        check("把这批任务移入回收站", int(affected or 0) == SEED, f"受影响 {affected} 条")

        set_search(main_win, "")
        click_sidebar(main_win, "回收站")
        time.sleep(1.0)

        trash_total = count_tasks(main_win, {"deletedOnly": True, "statuses": []})
        loaded = wait_cards(main_win, min(PAGE_SIZE, trash_total), timeout=20)
        btn = (
            main_win.eval(
                "(() => { const b = document.querySelector('.btn--danger'); return b ? b.innerText : null; })()"
            )
            or ""
        )
        check(
            "回收站按钮显示的数量等于后端真实总数",
            f"{trash_total}" in btn,
            f"按钮文案 {btn!r}，后端 count = {trash_total}",
        )
        check(
            "确认数量不是「已加载条数」",
            loaded < trash_total and f"{loaded}" not in btn,
            f"界面已加载 {loaded} 条，按钮写的是 {trash_total} 条",
        )

        # 弹窗文案也要用真实总数（拦下 confirm，读它的文本）
        confirm_text = main_win.eval(
            """
            (() => {
              const orig = window.confirm;
              let seen = null;
              window.confirm = (msg) => { seen = msg; return false; };
              const b = document.querySelector('.btn--danger');
              if (b) b.click();
              window.confirm = orig;
              return seen;
            })()
            """
        )
        check(
            "二次确认弹窗里也是真实总数",
            confirm_text is not None and f"{trash_total}" in str(confirm_text),
            f"弹窗文本：{confirm_text!r}",
        )

        # ---------------- §6 附件清理 ----------------
        orphan = main_win.eval(invoke_js("attachment_cleanup_orphans", {}))
        check(
            "附件孤儿清理命令可用且返回完整统计",
            isinstance(orphan, dict) and {"scanned", "kept", "removed", "skipped"} <= set(orphan),
            f"返回：{orphan}",
        )

    finally:
        # ---------------- 清理（无论成败都要执行） ----------------
        print("\n清理脚本造的数据…", flush=True)
        try:
            if soft_deleted:
                removed = purge_ids(main_win, ids)
                print(f"   已永久删除 {removed}/{len(ids)} 条", flush=True)
            else:
                # 还没来得及软删：先删掉，避免留下垃圾
                js = """
                (async () => await window.__TAURI_INTERNALS__.invoke('task_bulk', {
                  input: { ids: %s, action: 'delete' } }))()
                """ % json.dumps(ids)
                main_win.eval(js)
                purge_ids(main_win, ids)
                print(f"   已删除 {len(ids)} 条", flush=True)
            set_search(main_win, "")
            click_sidebar(main_win, "今天")
        except Exception as e:  # noqa: BLE001
            print(f"⚠️ 清理过程出错（请手工检查 ZZVERIFY2 前缀的任务）：{e}", flush=True)

    return finish(main_win, ids, baseline_all, baseline_trash)


def finish(main_win: ui.Target, ids: list[str], baseline_all: int, baseline_trash: int) -> int:
    """收尾断言 + 汇总。"""
    left = count_tasks(main_win, {"search": PREFIX, "includeDeleted": True, "statuses": []})
    check("脚本造的数据已清空", left == 0, f"仍然存在 {left} 条")

    now_all = count_tasks(main_win, {"includeDeleted": True, "statuses": []})
    now_trash = count_tasks(main_win, {"deletedOnly": True, "statuses": []})
    check(
        "用户原有数据未被改动",
        now_all == baseline_all and now_trash == baseline_trash,
        f"全部 {baseline_all}→{now_all}，回收站 {baseline_trash}→{now_trash}",
    )

    passed = sum(1 for _, ok, _ in results if ok)
    print(f"\n===== {passed}/{len(results)} 项通过 =====", flush=True)
    for name, ok, detail in results:
        if not ok:
            print(f"❌ {name} —— {detail}", flush=True)
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
