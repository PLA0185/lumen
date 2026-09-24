"""Lumen 第三轮整改的实机验收（《Lumen 第三轮整改任务书》§12）。

## 只跑隔离 profile：碰到生产目录**一律拒绝**，没有任何绕过开关（§24/§25/§26/§29）

**这个脚本没有任何绕过开关。** 检测到当前实例的数据目录是生产目录时，
无条件打印拒绝说明并 `exit 1`，**不做任何写操作**。
（此前存在的 `--allow-production` 已在收口整改 §26 中**删除**：参数本身、
`PRODUCTION_WARNING` / `PRODUCTION_WARNING_TAIL` 两段警告文案、
以及所有分支里对它的使用，全部去掉，没有留下任何"显式传参就能放行"的出口。）

它会在真实数据库上造数、删数（2000 条分页数据 + 回收站删除范围实验），
所以脚本做的第一件事就是通过后端命令 `app_data_paths` 读出**当前实例**的数据目录，
**先解析掉 junction/symlink 别名再比较**（`os.path.normcase(os.path.realpath(...))`，
`realpath` 会把目录联接解析成它真正指向的目录、并抹平尾部分隔符），
与默认生产目录 `%APPDATA%\\com.pla0185.lumen` 比较；命中就停下，
**不写任何数据、不造任何数据**。

为什么必须解析别名（§29）：只要把 `C:\\Temp\\fake-test` 做成指向生产目录的
**junction**，旧写法 `abspath + normcase` 就会把它当成"另一个目录"而放行——
别名与真身必须被判成同一个目录。同理，读不到 `app_data_paths`（IPC 抛错）
或返回的 `dataDir` 为空时也**拒绝**：拿不到证据就按最坏情况处理，
保守到底，绝不放行。

判定逻辑集中在不依赖 CDP/网络的两个函数里（`is_production_data_dir` 与
`decide_profile`），并有隐藏自测入口 `--self-test-profile-check` 覆盖 §29 的九种情形
（含**真的建一个 junction 别名再删掉**），不启动应用也能跑。

`ZZR3-` 前缀、`finally` 里按 id 清理、`window.confirm` 全程接管（默认拒绝）
都只是**第二层**防护——它们保证的是"失败时能收拾干净"，
挡不住"在用户的库上真的删了东西"这件事本身。

为什么这么严：这套脚本的前身已经制造过一次**真实事故**——
替换 `window.confirm` 之后过早还原，按钮的 async 流程弹出了**原生**确认框，
无头环境自动接受，用户回收站里原有的 **18 条任务被永久删除**，无法恢复。
那次事故的教训不是"清理要更小心"，而是"destructive 验收根本不该碰生产目录"。

## 怎么跑（隔离 profile，推荐）

```powershell
# 1) 指向一个一次性的测试数据目录（应用侧在**启动时**读取这个环境变量）
$test = Join-Path $env:TEMP "lumen-acceptance"
New-Item -ItemType Directory -Force $test | Out-Null
$env:LUMEN_TEST_DATA_DIR = $test
# 2) 打开 WebView2 的调试端口（同样必须在启动 Lumen 之前设置）
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"
# 3) 在**同一个窗口**里启动 Lumen（环境变量是启动时读的），然后再跑：
python tools/verify_remediation3.py --expect-data-dir $test
```

不设 `LUMEN_TEST_DATA_DIR` 时应用会用默认的 `%APPDATA%\\com.pla0185.lumen`，
那时本脚本会直接拒绝运行——**没有"我知道有风险"的开关**，
要跑就只能把应用起在隔离目录上。

命令行参数：

- `--expect-data-dir <path>`（可选）：显式声明本次期望的数据目录，
  与实例实际用的是同一个目录才继续，否则拒绝运行（双保险：防止"以为在隔离目录、
  实际连到了生产实例"）。比较同样走 `realpath`，所以"期望目录写成 junction 别名"
  不会假失败。
- `--self-test-profile-check`（隐藏，不写"-h"里）：只跑上面那两个判定函数的自测，
  覆盖 §29 的九种情形（含真的建 junction 别名），**不连 CDP、不碰应用**，
  全过 exit 0、有任一不过 exit 1。

**没有任何允许在生产目录上跑的开关**：`--allow-production` 已随 §26 整改删除，
再把脚本改成"能显式放行生产目录"属于回退，不要这样做。

退出码：全部断言通过为 0；profile 校验不通过、或有断言失败为 1
（末尾打印 `X/Y 项通过`）。

## 这个脚本验收什么

四组**只有真机才能看到**的断言（单元测试覆盖不到的跨层链路）：

- **A. 回收站删除范围（§12.1）**：「永久删除 N 项」的 N 必须等于**当前筛选条件**下的
  后端计数（搜索命中的 2 条），而不是回收站总数；确认弹窗要同时写出
  「只删除筛选结果里的 2 项；回收站共 N 项」；真正执行后**只有命中的 2 条**被永久删除，
  未命中的 3 条必须**仍在**回收站里（修复前会被一起删掉）。
- **B. 分页链路（§12.2，2000 条）**：首屏只渲染一页（`PAGE_SIZE = 200`，见 `src/lib/store.ts`）、
  「已显示 X / 共 2000 条」文案正确、连续点「加载更多」能一直取到第 501 / 1201 / 2000 条，
  以及**快速切换搜索词**之后列表必须是 B 的结果、不含 A 的残留
  （第三轮新增的 `queryGeneration` 机制）。
- **C. 附件删除不跟随链接（§12.3）**：在**真实文件系统**的 `attachments/` 下造目录联接
  （junction），调用后端 `attachment_cleanup_orphans`，断言
  **联接指向的目录外文件仍然存在**（删除绝不能跟随重解析点删目标）。
- **D. Provider 的 Key 状态按 provider 独立（§12.4）**：逐个切换服务商下拉框，界面上
  「已配置」徽标的有无必须与后端 `ai_provider_key_status` 返回的**那一格**一致
  （不能出现"切到没配 Key 的服务商却显示已配置"）。

## 安全约定（有血的教训，改之前先读 `tools/verify_purge_scope.py` 开头）

> 第 1 条（隔离 profile）是**第一层**防护，由本脚本在造任何数据之前强制；
> 第 2~7 条是**第二层**：即使真的跑起来了，也要保证"删的范围 = 确认的范围"、失败能收拾干净。

1. **隔离 profile 强制**（§24/§25/§26，见上方"只跑隔离 profile"）：
   启动后第一件事就是读 `app_data_paths`，把实例实际目录**解析别名后**（`realpath`）
   与默认生产目录比较，命中就打印错误 + `exit 1`，**不写任何数据**。
   **没有任何出口**：`--allow-production` 已删除；读不到 / 读到空值也一律拒绝。
2. 造出来的任务标题一律带唯一前缀 `ZZR3-`，结束时在 `finally` 里**逐个按 id** 永久删除；
   **绝不调用 `task_purge_all_deleted` / 「清空回收站」**——那会删掉用户自己回收站里的任务。
   （前缀 + finally 只能保证"自己造的自己收拾"，不能保证"没在别人的库上造"——
   所以它们是第二层，第一层是第 1 条的 profile 校验。）
3. `window.confirm` **全程接管**：脚本一开始就把 `window.confirm` 换成自己的实现并默认返回
   `false`；只有明确要执行删除的那一步才切到 `accept`，执行完立刻切回 `block`；
   直到脚本结束才还原。
   （上一版脚本"替换后又同步恢复"，按钮的 async 流程随后弹出的是**原生** confirm，
   无头环境自动接受 → 误删了用户 18 条数据。）
4. 需要点击的按钮不在视口内时先 `scrollIntoView({block:'center'})` 再点，
   否则 CDP 的坐标点击（视口坐标）会落空。
5. `sys.stdout.reconfigure(encoding="utf-8", ...)`：Windows 控制台默认 GBK，
   直接 print ✅ 会抛 UnicodeEncodeError 并打断验收。
6. 本脚本**不写任何凭据**：§12.4 只读后端状态 + 切换界面下拉（`AiPanel.switchProvider`
   只改组件本地 state，不落库、不碰凭据管理器）。
   若四个 provider 的密钥状态相同（例如都为 false），脚本**如实说明"无法区分"**，
   不会伪造数据去改凭据管理器。
7. §12.3 会在**数据目录**的 `attachments/` 里临时建联接与对照文件，`finally` 里逐个清掉
   （联接用 `os.rmdir` 只删链接自身，绝不用 `shutil.rmtree`）。
   隔离 profile 下动的是测试目录；脚本已经**不可能**在生产目录上跑起来
   （第 1 条没有任何绕过开关），这也是第 1 条存在的理由。
"""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid as uuidlib
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).parent))
import ui_drive as ui  # noqa: E402

PORT = int(os.environ.get("LUMEN_CDP_PORT", "9222"))

# 一页加载多少条，必须与前端 `PAGE_SIZE`（src/lib/store.ts）一致
PAGE_SIZE = 200
SEED = 2000  # §12.2 要求的规模：必须远大于一页，才证明分页真的在翻
SEED_BATCH = 400  # 单次 Runtime.evaluate 不要跑太久（CDP 默认 30s 超时）

# 所有脚本造的数据都带这一族前缀，与用户数据不会混淆
PREFIX = "ZZR3-PAGE"  # B 段：2000 条分页数据
HIT_PREFIX = "ZZR3-HIT"  # A 段：会被搜索命中的回收站任务
MISS_PREFIX = "ZZR3-MISS"  # A 段：不该被这次删除碰到的对照组
RACE_A_PREFIX = "ZZR3-QA"  # B 段：快速切换搜索词用的 A
RACE_B_PREFIX = "ZZR3-QB"  # B 段：快速切换搜索词用的 B
HIT_COUNT = 2
MISS_COUNT = 3
RACE_A_COUNT = 60  # 故意与 B 的数量不同：数量本身就是"串台"的判别信号
RACE_B_COUNT = 40
ANY_PREFIX = "ZZR3-"

LOAD_TARGETS = (501, 1201, 2000)  # 必须能翻到的三条"深页"位置

# 只看回收站的条件：空 statuses = 不限制状态（commands.rs::apply_task_filters）
TRASH_Q = {"deletedOnly": True, "statuses": []}
ALL_Q = {"includeDeleted": True, "statuses": []}
# 「全部任务」视图的条件，必须与 buildQuery() 一致，否则比较的是两套筛选会假失败
PAGE_Q = {"search": PREFIX, "statuses": ["todo", "doing", "waiting", "done"]}
HIT_Q = {"deletedOnly": True, "search": HIT_PREFIX, "statuses": []}
MISS_Q = {"deletedOnly": True, "search": MISS_PREFIX, "statuses": []}

PROVIDERS = ("deep_seek", "open_ai", "claude", "custom")

results: list[tuple[str, bool, str]] = []
# 只在报告里列出的"本机无法确认"项，不计入通过率
uncertain: list[str] = []


def check(name: str, ok: bool, detail: str = "") -> bool:
    results.append((name, ok, detail))
    print(f"{'✅' if ok else '❌'} {name}" + (f" —— {detail}" if detail else ""), flush=True)
    return ok


def note_uncertain(text: str) -> None:
    uncertain.append(text)
    print(f"⚠️ 无法确认：{text}", flush=True)


def run_section(name: str, fn, *args) -> None:
    """跑一段验收；抛异常时如实记一条 ❌，但**不**中断后面的段落。

    这不等于"失败就跳过"：异常本身会被记成一条失败断言，
    剩余段落仍然要跑（各自的断言也照常累积）。
    """
    try:
        fn(*args)
    except Exception as e:  # noqa: BLE001
        check(
            f"{name} 段执行时抛出异常（该段后续断言未能执行）",
            False,
            f"{type(e).__name__}: {e}",
        )


# ---------------------------------------------------------------------------
# 隔离 profile 强制（§24/§25/§26/§29）：造任何数据之前的**第一件事**，且**无绕过开关**
# ---------------------------------------------------------------------------

# Tauri 的 bundle id，同时也是默认数据目录名（src-tauri/tauri.conf.json 的 identifier）
DEFAULT_DATA_DIR_NAME = "com.pla0185.lumen"
# 应用侧用来覆盖数据目录的环境变量——必须在**启动 Lumen 之前**设置才有效
TEST_DATA_DIR_ENV = "LUMEN_TEST_DATA_DIR"


def default_production_data_dir() -> Path:
    """默认生产数据目录：Windows 下是 `%APPDATA%\\com.pla0185.lumen`（Tauri 的 app_data_dir()）。"""
    roaming = os.environ.get("APPDATA")
    base = Path(roaming) if roaming else Path.home() / "AppData" / "Roaming"
    return base / DEFAULT_DATA_DIR_NAME


def normalize_dir(path: object) -> str:
    """把目录路径规范化成可直接比较的字符串（**不解析链接**）。

    - `abspath` → `normpath`：相对路径按 cwd 展开，并去掉尾部分隔符与 `.` / `..`；
    - `normcase`：Windows 下抹平大小写与分隔符差异（`C:/Temp/A` 与 `c:\\temp\\a` 是同一个目录）。
    """
    if path is None:
        return ""
    text = str(path).strip().strip('"')
    if not text:
        return ""
    return os.path.normcase(os.path.abspath(text))


def canonical_dir(path: object) -> str:
    """规范化 **+ 解析 junction/symlink** 之后的比较键（§29）。

    比 `normalize_dir` 多一步 `os.path.realpath`：它会把目录联接（junction）与
    符号链接解析成**真正指向的目录**，于是"别名"和"真身"得到同一个键——
    这正是旧写法 `abspath + normcase` 漏掉的那一步（`C:\\Temp\\fake-test`
    做成指向生产目录的 junction 时，旧写法会把它当成另一个目录而放行）。

    `realpath` 对**不存在**的路径也会尽力解析（逐段解析已存在的部分），
    所以"生产目录还没建出来"时同样成立；实在解析不动时退化成规范化路径。
    """
    normalized = normalize_dir(path)
    if not normalized:
        return ""
    return os.path.normcase(os.path.realpath(normalized))


def is_production_data_dir(path: object, default_dir: object) -> bool:
    """`path` 是否就是默认生产目录——**解析别名之后**比较（§29）。

    纯函数（只用 `os.path`，不连 CDP、不碰网络、不写盘），可以单独喂样例验证。
    下列情形返回 True：生产目录本身、大小写变体、尾部带分隔符变体、
    以及**通过 junction/symlink 指向生产目录的临时路径**；
    普通临时目录（真实目录，不是链接）与相似名（`…lumen-backup`）返回 False。

    `path` 为空 / 拿不到时返回 False——"拿不到"必须由调用方按**拒绝**处理
    （见 `decide_profile` 的 `read-failed` / `empty-data-dir`），不能靠这里放行。
    """
    actual = canonical_dir(path)
    if not actual:
        return False
    return actual == canonical_dir(default_dir)


# decide_profile 的原因码：只有 PROFILE_OK 放行，其余一律拒绝
PROFILE_OK = "ok"  # 允许继续
PROFILE_READ_FAILED = "read-failed"  # app_data_paths 读取失败
PROFILE_EMPTY_DATA_DIR = "empty-data-dir"  # 后端没返回 dataDir
PROFILE_PRODUCTION = "production"  # 命中了生产目录（含 junction/symlink 别名）
PROFILE_EXPECT_MISMATCH = "expect-mismatch"  # 与 --expect-data-dir 不一致


def decide_profile(
    actual: object,
    default_dir: object,
    expect_dir: object = None,
    *,
    read_failed: bool = False,
) -> tuple[bool, str]:
    """隔离 profile 的**纯判定**：返回 `(是否放行, 原因码)`。

    抽成纯函数是为了能在不启动应用的情况下自测（§29）：CDP 那段只负责把
    `app_data_paths` 的结果喂进来、再按原因码打印人话，判定本身只看参数。

    顺序即优先级：读取失败 / 空值 → 生产目录 → 与期望目录不一致。
    前两条是**保守拒绝**（拿不到证据就不放行），第三条是双保险。
    """
    if read_failed:
        return False, PROFILE_READ_FAILED
    if not str(actual or "").strip().strip('"'):
        return False, PROFILE_EMPTY_DATA_DIR
    if is_production_data_dir(actual, default_dir):
        return False, PROFILE_PRODUCTION
    if expect_dir and str(expect_dir).strip().strip('"'):
        if canonical_dir(actual) != canonical_dir(expect_dir):
            return False, PROFILE_EXPECT_MISMATCH
    return True, PROFILE_OK


PRODUCTION_REFUSAL = """
============================================================
❌ 拒绝运行：当前 Lumen 实例的数据目录是【生产目录】

   实例实际数据目录：{actual}
   默认生产目录：    {default}

本脚本会**在真实数据库上造数、删数**（{seed} 条分页数据 + 回收站删除范围实验），
而它已经制造过一次真实事故：上一版脚本"替换 window.confirm 后又同步还原"，
按钮的 async 流程随后弹出的是**原生**确认框，无头环境自动接受，
用户回收站里原有的 **18 条任务被永久删除**，无法恢复。
那次事故说明：destructive 验收就不该碰生产目录。

**这里没有任何绕过开关**：判定为生产目录就无条件退出，
不写任何数据、不造任何数据（原来的 `--allow-production` 已删除）。

正确的启动方式（环境变量必须在**启动 Lumen 之前**设置，应用是启动时读它的）：

   $test = Join-Path $env:TEMP "lumen-acceptance"
   New-Item -ItemType Directory -Force $test | Out-Null
   $env:{env_name} = $test
   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
   # 在**同一个窗口**里启动 Lumen，然后：
   python tools/verify_remediation3.py --expect-data-dir $test

注意：`{env_name}` 指向的目录**本身**也不能是指向生产目录的 junction/symlink——
判定会解析别名，别名指向生产目录时同样在这里被拒绝。
============================================================
"""


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """命令行参数：只跑隔离 profile，**没有**允许生产目录的开关（§26）。"""
    parser = argparse.ArgumentParser(
        prog="verify_remediation3.py",
        description=(
            "Lumen 第三轮整改的实机验收（§12）。"
            "只跑隔离 profile：数据目录一旦是 %APPDATA%\\com.pla0185.lumen"
            "（或指向它的 junction/symlink 别名）就直接拒绝运行，没有任何绕过开关。"
        ),
        epilog=(
            "隔离 profile 的启动方式：先设 $env:LUMEN_TEST_DATA_DIR=<一次性目录> 与 "
            '$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"，'
            "再启动 Lumen，最后运行本脚本（可加 --expect-data-dir 做双保险）。"
        ),
    )
    parser.add_argument(
        "--expect-data-dir",
        metavar="PATH",
        default=None,
        help="显式声明本次期望的数据目录；与实例实际用的目录不一致就拒绝运行（双保险）。",
    )
    parser.add_argument(
        "--self-test-profile-check",
        action="store_true",
        help=argparse.SUPPRESS,  # 隐藏：仅供本脚本自测，正常验收不会用到
    )
    return parser.parse_args(argv)


def verify_isolated_profile(main_win: ui.Target, args: argparse.Namespace) -> tuple[bool, str]:
    """启动即校验 profile，返回 `(是否继续, 实例实际的数据目录)`。

    这条检查是脚本做的**第一件事**，在造任何数据之前执行；返回 False 时调用方必须
    直接结束，不得再做任何写操作。判定本身在纯函数 `decide_profile` 里
    （可被 `--self-test-profile-check` 覆盖），这里只负责取数、打印、返回。
    """
    print("===== 隔离 profile 校验（§24/§25/§26/§29：第一件事）=====", flush=True)
    env_hint = os.environ.get(TEST_DATA_DIR_ENV)
    print(
        f"   脚本环境里的 {TEST_DATA_DIR_ENV} = {env_hint!r}"
        f"（应用侧在**启动时**读它，这里只作提示，判定一律以实例实际目录为准）",
        flush=True,
    )
    default_dir = default_production_data_dir()
    print(f"   默认生产目录（%APPDATA%，判定时会解析 junction/symlink）：{default_dir}", flush=True)

    # 读**当前实例**的数据目录：这是唯一的判定依据。
    # 读不到 = 拿不到证据，按最坏情况处理（reject），不放行。
    read_failed = False
    read_error = ""
    actual = ""
    try:
        paths = main_win.eval(invoke_js("app_data_paths", {})) or {}
        actual = str(paths.get("dataDir") or "")
    except Exception as e:  # noqa: BLE001
        read_failed = True
        read_error = f"{type(e).__name__}: {e}"

    allowed, reason = decide_profile(
        actual, default_dir, args.expect_data_dir, read_failed=read_failed
    )

    if reason != PROFILE_PRODUCTION:
        print(f"   实例实际数据目录：{actual!r}", flush=True)

    if not allowed:
        # 三种拒绝各自的说明；**都不写任何数据**
        if reason == PROFILE_READ_FAILED:
            print(
                f"❌ 拒绝运行：读不到 app_data_paths（{read_error}）。"
                "无法证明当前实例跑在隔离 profile 上，按最坏情况处理。",
                flush=True,
            )
            return False, ""
        if reason == PROFILE_EMPTY_DATA_DIR:
            print(
                "❌ 拒绝运行：app_data_paths 没有返回 dataDir，"
                "无法证明当前实例跑在隔离 profile 上（空值一律按拒绝处理）。",
                flush=True,
            )
            return False, ""
        if reason == PROFILE_PRODUCTION:
            print(
                PRODUCTION_REFUSAL.format(
                    actual=actual,
                    default=default_dir,
                    seed=SEED,
                    env_name=TEST_DATA_DIR_ENV,
                ),
                flush=True,
            )
            return False, actual
        # 只剩 expect 不一致：这条是双保险，说明"连到的不是预期那个实例"
        print(
            "❌ 拒绝运行：实例实际的数据目录与 --expect-data-dir 不一致。\n"
            f"   实例实际：{actual}（规范化并解析别名后：{canonical_dir(actual)}）\n"
            f"   期望目录：{args.expect_data_dir}（同上：{canonical_dir(args.expect_data_dir)}）\n"
            "   （多半是连到了另一个实例，或启动 Lumen 时忘了设 "
            f"{TEST_DATA_DIR_ENV}；请确认连的是预期那个实例再跑。）",
            flush=True,
        )
        return False, actual

    print(f"✅ 已确认隔离 profile：{actual}（解析别名后不是生产目录 {default_dir}）", flush=True)
    if args.expect_data_dir:
        print(f"✅ 已确认数据目录与 --expect-data-dir 一致：{actual}", flush=True)

    print("", flush=True)
    return True, actual


# ---------------------------------------------------------------------------
# 判定函数的自测（隐藏入口 `--self-test-profile-check`）：覆盖 §29 的九种情形
# ---------------------------------------------------------------------------

def run_profile_check_selftest() -> int:
    """只跑生产目录判定的自测，返回退出码（全过 0，有失败 1）。

    只用 `os.path` / `tempfile` / `cmd /C mklink`：**不连 CDP、不读应用、
    不写生产数据**，应用没在跑时也能执行。junction 别名会在系统临时目录里
    **真的建出来**，用完逐个 `os.rmdir` 删掉（只删链接自身，绝不跟随目标），
    确认删干净之后才删沙箱。
    """
    print("===== 生产目录判定自测（--self-test-profile-check，§29）=====", flush=True)
    real_default = default_production_data_dir()
    print(f"   本机默认生产目录：{real_default}（存在={real_default.is_dir()}）", flush=True)

    root = Path(tempfile.mkdtemp(prefix="lumen-profile-selftest-"))
    print(f"   沙箱（系统临时目录，自测结束即删除）：{root}", flush=True)

    # 沙箱里伪造一套"生产目录"体系：默认目录、相似名目录、普通临时目录
    prod_root = root / "prod-root"
    fake_default = prod_root / DEFAULT_DATA_DIR_NAME
    fake_default.mkdir(parents=True)
    plain = root / "plain-temp"
    plain.mkdir()
    other = root / "other-temp"
    other.mkdir()
    similar_backup = prod_root / (DEFAULT_DATA_DIR_NAME + "-backup")
    similar_backup.mkdir()

    links: list[Path] = []
    case_failures: list[str] = []
    cleanup_problems: list[str] = []
    stats = {"total": 0}

    def case(title: str, got: object, want: object, detail: str = "") -> None:
        ok = got == want
        stats["total"] += 1
        if not ok:
            case_failures.append(title)
        print(
            f"{'✅' if ok else '❌'} {title}\n"
            f"      实际 {got!r} / 期望 {want!r}" + (f"\n      {detail}" if detail else ""),
            flush=True,
        )

    def make_alias(name: str, target: Path) -> Path:
        """真的建一个目录联接（普通用户权限即可），并打印真实命令与返回码。"""
        link = root / name
        rc, out = make_junction(link, target)
        print(
            f'   $ cmd /C mklink /J "{link}" "{target}"\n'
            f"     rc={rc}" + (f"，输出：{out}" if out else ""),
            flush=True,
        )
        ok = rc == 0 and os.path.lexists(link)
        if not ok:
            case_failures.append(f"junction 创建失败：{name}")
            print(f"❌ junction 创建失败（{name}）—— 别名用例不成立", flush=True)
        else:
            links.append(link)
        return link

    try:
        alias = make_alias("fake-test", fake_default)  # 指向沙箱里的"生产目录"
        alias_real = make_alias("fake-test-real", real_default)  # 指向本机真实生产目录
        alias_plain = make_alias("fake-plain", plain)  # 指向普通临时目录（非生产）
        alias_parent = make_alias("fake-parent", prod_root)  # 指向生产目录的**父目录**

        # ---- ① 真实生产目录 ----
        case(
            "① 真实生产目录本身 → 判为生产（拒绝）",
            is_production_data_dir(fake_default, fake_default),
            True,
        )
        case(
            "①b 本机真实 %APPDATA% 生产目录 → 判为生产（拒绝）",
            is_production_data_dir(real_default, real_default),
            True,
            detail=f"{real_default}",
        )

        # ---- ② 大小写变体 ----
        case(
            "② 大小写变体（整条路径转大写）→ 判为生产（拒绝）",
            is_production_data_dir(str(fake_default).upper(), fake_default),
            True,
            detail=str(fake_default).upper(),
        )

        # ---- ③ 尾分隔符变体 ----
        for suffix, label in (("\\", "反斜杠"), ("/", "正斜杠")):
            case(
                f"③ 尾分隔符变体（结尾多一个{label}）→ 判为生产（拒绝）",
                is_production_data_dir(str(fake_default) + suffix, fake_default),
                True,
                detail=repr(str(fake_default) + suffix),
            )

        # ---- ④ junction 别名（真的建出来了）----
        case(
            "④ junction 别名 → 生产目录 → 判为生产（拒绝；旧 abspath 写法会放行）",
            is_production_data_dir(alias, fake_default),
            True,
            detail=f"{alias} → {fake_default}",
        )
        case(
            "④b junction 别名 → 本机真实生产目录 → 判为生产（拒绝）",
            is_production_data_dir(alias_real, real_default),
            True,
            detail=f"{alias_real} → {real_default}",
        )
        case(
            "④c 别名走完整判定（decide_profile）同样拒绝，且传 --expect-data-dir 也改不了结论",
            decide_profile(str(alias), fake_default, expect_dir=str(alias)),
            (False, PROFILE_PRODUCTION),
        )
        case(
            "④d junction 出现在路径**前缀**里（假父目录 + 生产目录名）→ 判为生产（拒绝）",
            is_production_data_dir(alias_parent / DEFAULT_DATA_DIR_NAME, fake_default),
            True,
            detail=f"{alias_parent}\\{DEFAULT_DATA_DIR_NAME}",
        )

        # ---- ⑤ 普通临时目录（真实目录，不是链接）----
        case(
            "⑤ 普通临时目录（真实目录，不是链接）→ 放行",
            decide_profile(str(plain), fake_default),
            (True, PROFILE_OK),
        )
        case(
            "⑤b 普通临时目录不被判为生产目录",
            is_production_data_dir(plain, fake_default),
            False,
        )

        # ---- ⑥ app_data_paths 读取失败 → 拒绝（保守）----
        case(
            "⑥ app_data_paths 读取失败（拿不到证据）→ 拒绝（read-failed）",
            decide_profile(None, fake_default, read_failed=True),
            (False, PROFILE_READ_FAILED),
        )

        # ---- ⑦ dataDir 为空 → 拒绝（保守）----
        for empty in ("", "   ", '""'):
            case(
                f"⑦ dataDir 为空（{empty!r}）→ 拒绝（empty-data-dir）",
                decide_profile(empty, fake_default),
                (False, PROFILE_EMPTY_DATA_DIR),
            )

        # ---- ⑧ 与 --expect-data-dir 不一致 → 拒绝 ----
        case(
            "⑧ --expect-data-dir 与实例实际目录不一致 → 拒绝（expect-mismatch）",
            decide_profile(str(plain), fake_default, expect_dir=str(other)),
            (False, PROFILE_EXPECT_MISMATCH),
            detail=f"实际 {plain} / 期望 {other}",
        )
        case(
            "⑧b --expect-data-dir 一致 → 放行",
            decide_profile(str(plain), fake_default, expect_dir=str(plain)),
            (True, PROFILE_OK),
        )
        case(
            "⑧c 期望目录写成别名、实例报的是真身 → 视为一致（realpath 也用在期望比较上）",
            decide_profile(str(plain), fake_default, expect_dir=str(alias_plain)),
            (True, PROFILE_OK),
            detail=f"{alias_plain} → {plain}",
        )

        # ---- ⑨ 相似名（…lumen-backup）→ 放行 ----
        case(
            "⑨ 相似名目录（com.pla0185.lumen-backup）→ 放行（不是生产目录）",
            decide_profile(str(similar_backup), fake_default),
            (True, PROFILE_OK),
            detail=f"{similar_backup}",
        )
        case(
            "⑨b 本机真实生产目录的相似名（真路径 + -backup）→ 放行",
            is_production_data_dir(str(real_default) + "-backup", real_default),
            False,
        )

        # ---- ⑩ 附：确认绕过开关真的没了 ----
        stderr_buf = io.StringIO()
        exit_code: object = None
        try:
            with contextlib.redirect_stderr(stderr_buf):
                parse_args(["--allow-production"])
        except SystemExit as e:
            exit_code = e.code
        tail = (stderr_buf.getvalue().strip().splitlines() or [""])[-1]
        case(
            "⑩ 绕过开关已删除：`--allow-production` 被 argparse 拒绝（SystemExit 2）",
            exit_code,
            2,
            detail=f"argparse 输出：{tail}",
        )
        leftovers = [
            name
            for name in ("PRODUCTION_WARNING", "PRODUCTION_WARNING_TAIL")
            if name in globals()
        ]
        if hasattr(parse_args([]), "allow_production"):
            leftovers.append("args.allow_production")
        case(
            "⑩b 旧符号确实不存在（PRODUCTION_WARNING / PRODUCTION_WARNING_TAIL / allow_production）",
            leftovers,
            [],
        )
    finally:
        # 先删 junction：**只能** os.rmdir（只删链接自身），
        # 并确认删干净之后才允许递归删沙箱——Windows 上 rmtree 会跟着目录联接
        # 进目标目录删文件（§12.3 那条断言防的就是这件事）。
        left_links: list[Path] = []
        for link in links:
            try:
                if os.path.lexists(link):
                    os.rmdir(link)
                    print(f"   已删除 junction（os.rmdir，只删链接自身）：{link}", flush=True)
            except OSError as e:  # noqa: BLE001
                left_links.append(link)
                print(f"⚠️ junction 未删除，请手工清理：{link} —— {e}", flush=True)
        if left_links:
            cleanup_problems.append(
                f"树里仍有 {len(left_links)} 个目录联接，已放弃递归删除沙箱：{root}"
            )
            print(
                f"⚠️ 沙箱保留不删（树里还有链接，递归删除可能跟随链接）：{root}",
                flush=True,
            )
        else:
            shutil.rmtree(root, ignore_errors=True)
            print(f"   沙箱已删除：{root}（仍然存在={root.exists()}）", flush=True)
            if root.exists():
                cleanup_problems.append(f"沙箱未能删净：{root}")

    total = stats["total"]
    passed = total - len(case_failures)
    print(f"\n===== 自测 {passed}/{total} 项通过 =====", flush=True)
    for name in case_failures:
        print(f"❌ {name}", flush=True)
    for text in cleanup_problems:
        print(f"⚠️ 清理未完成：{text}", flush=True)
    if passed == total and not cleanup_problems:
        print("结论：判定符合 §29 的九种情形（含真实 junction 别名）。", flush=True)
        return 0
    print("结论：自测**未通过**，不要拿这套判定去跑破坏性验收。", flush=True)
    return 1


# ---------------------------------------------------------------------------
# 通用：JS 调用与界面操作
# ---------------------------------------------------------------------------

def invoke_js(cmd: str, args: dict | None = None) -> str:
    """一段"调用后端命令并返回结果"的 JS 片段（在页面上下文里执行）。"""
    return "window.__TAURI_INTERNALS__.invoke(%s, %s)" % (
        json.dumps(cmd),
        json.dumps(args or {}, ensure_ascii=False),
    )


def count_tasks(main_win: ui.Target, query: dict) -> int:
    r = main_win.eval(invoke_js("task_count", {"query": query}))
    return int(r["total"])


def seed_tasks(
    main_win: ui.Target,
    prefix: str,
    n: int,
    batch: int = SEED_BATCH,
    sink: list[str] | None = None,
) -> list[str]:
    """批量创建带前缀的任务，返回 id 列表。

    分批发是为了避免单次 `Runtime.evaluate` 太久（CDP 默认 30s 超时），
    也让进度可见。`sink` 会在每批之后立刻落进调用方的登记表——
    万一中途抛错，`finally` 仍然知道要清理哪些 id。
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
              input: { title: %s + '-' + (window.__zzr3_index++), status: 'todo' }
            });
            ids.push(t.id);
          }
          return ids;
        })()
        """ % (take, json.dumps(prefix))
        main_win.eval("window.__zzr3_index = window.__zzr3_index || 0; true")
        got = main_win.eval(js) or []
        ids.extend(got)
        if sink is not None:
            sink.extend(got)
        done += take
        print(f"   已创建 {done}/{n}（{prefix}）", flush=True)
    return ids


def bulk_delete(main_win: ui.Target, ids: list[str]) -> str:
    """软删到回收站（分批调用，失败不抛异常——清理路径要尽量走完）。"""
    if not ids:
        return ""
    js = """
    (async () => {
      try {
        return String(await window.__TAURI_INTERNALS__.invoke('task_bulk', {
          input: { ids: %s, action: 'delete' }
        }));
      } catch (e) {
        return 'ERR:' + String(e);
      }
    })()
    """ % json.dumps(ids)
    return str(main_win.eval(js))


def purge_ids(main_win: ui.Target, ids: list[str], batch: int = 100) -> int:
    """逐个永久删除（分批并发）。返回成功删除的数量。

    刻意**不用** `task_purge_all_deleted`：那会把用户自己回收站里的任务一起删掉。
    这里只删脚本自己造的那些 id；已经不在回收站里的会被后端拒绝，忽略即可。
    """
    removed = 0
    for i in range(0, len(ids), batch):
        chunk = ids[i : i + batch]
        js = """
        (async () => {
          let ok = 0;
          await Promise.all(%s.map(async (id) => {
            try { await window.__TAURI_INTERNALS__.invoke('task_purge', { id }); ok++; }
            catch (e) { /* 已经不在回收站里的忽略 */ }
          }));
          return ok;
        })()
        """ % json.dumps(chunk)
        removed += int(main_win.eval(js) or 0)
    return removed


def card_count(main_win: ui.Target) -> int:
    return int(main_win.eval("document.querySelectorAll('.task').length") or 0)


def card_titles(main_win: ui.Target, n: int = 200) -> list[str]:
    """取前 n 张卡片的标题。

    刻意用标题而不是 innerText：卡片上还有"逾期 3 天""还有 2 小时"这类
    **会随时间变化**的文案，拿它做比较会假失败。
    """
    return (
        main_win.eval(
            "Array.from(document.querySelectorAll('.task .task__title'))"
            ".slice(0, %d).map(e => e.innerText)" % n
        )
        or []
    )


def loadmore_note(main_win: ui.Target) -> str:
    return ui.text_of(main_win, ".loadmore__note") or ""


def parse_shown(note: str) -> tuple[int | None, int | None]:
    """解析「已显示 X / Y 条」。

    注意实际文案**没有「共」字**（App.tsx 的 LoadMore 组件），
    所以这里按真实渲染结果解析，而不是按任务书里的示意写法。
    """
    m = re.search(r"已显示\s*(\d+)\s*/\s*(\d+)\s*条", note or "")
    if not m:
        return (None, None)
    return (int(m.group(1)), int(m.group(2)))


def extract_int(text: str | None, pattern: str) -> int | None:
    if not text:
        return None
    m = re.search(pattern, text)
    return int(m.group(1)) if m else None


def click_sidebar(main_win: ui.Target, label: str) -> None:
    main_win.eval(
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


def set_search(main_win: ui.Target, text: str, wait: float = 1.5) -> None:
    """给顶栏搜索框赋值。

    组件带 250ms 防抖，并且 `setSearch` 会立刻把 queryGeneration +1、offset 归零，
    所以这里必须等一会儿再读结果。
    """
    ui.set_react_input(main_win, 'input[type="search"]', text)
    time.sleep(wait)


def click_tab(main_win: ui.Target, label: str) -> None:
    main_win.eval(
        """
        (() => {
          const t = Array.from(document.querySelectorAll('.settings .tabs [role="tab"]'))
            .find(b => b.innerText.trim() === %s);
          if (!t) throw new Error('设置页没有标签：' + %s);
          t.click();
          return true;
        })()
        """
        % (json.dumps(label), json.dumps(label))
    )
    time.sleep(1.0)


def wait_cards(main_win: ui.Target, want: int, timeout: float = 30.0) -> int:
    """等卡片数量达到 want（界面刷新是异步的）。"""
    deadline = time.time() + timeout
    got = 0
    while time.time() < deadline:
        got = card_count(main_win)
        if got >= want:
            return got
        time.sleep(0.4)
    return got


def click_load_more(main_win: ui.Target, timeout: float = 10.0) -> bool:
    """把「加载更多」滚进视口后用**真实鼠标事件**点它。

    为什么必须先 `scrollIntoView`：`Input.dispatchMouseEvent` 用的是**视口坐标**，
    而"加载更多"在几百张卡片之后、远在视口外，直接点会落到空处
    （第一次跑实机验收就踩过这个坑，表现为"点了没反应"）。
    另外按钮在加载中会 `disabled`，那时点了也没用，所以先等它可用。
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        state = main_win.eval(
            """
            (() => {
              const b = document.querySelector('.loadmore button');
              if (!b) return null;
              b.scrollIntoView({ block: 'center' });
              return { disabled: b.disabled, text: b.innerText };
            })()
            """
        )
        if state is None:
            return False
        if not state.get("disabled"):
            time.sleep(0.35)  # 等 scrollIntoView 真的滚动完，再按坐标点
            try:
                ui.real_click(main_win, ".loadmore button")
            except Exception:
                # 这一刻按钮可能刚好因为"已经全部加载完"而消失——
                # 那是**正常行为**（总数为 0 或已加载完时不再渲染按钮），
                # 不是缺陷。交给调用方用卡片数/文案判断即可。
                return False
            return True
        time.sleep(0.3)
    return False


def load_until(main_win: ui.Target, want: int, timeout: float = 120.0) -> int:
    """反复点「加载更多」，直到 DOM 卡片数达到 want（或超时）。"""
    deadline = time.time() + timeout
    got = card_count(main_win)
    while got < want and time.time() < deadline:
        if not click_load_more(main_win):
            # 可能是自动加载（IntersectionObserver）正在跑，等一会儿再看
            time.sleep(0.8)
        else:
            time.sleep(0.6)
        got = card_count(main_win)
    return got


# ---------------------------------------------------------------------------
# confirm 接管：全程不撒手，直到脚本结束（见模块 docstring 第 2 条）
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


def arm_confirm(main_win: ui.Target) -> None:
    """接管 window.confirm，默认拒绝（阻止一切真实删除）。"""
    main_win.eval(ARM_CONFIRM)


def set_confirm_mode(main_win: ui.Target, mode: str) -> None:
    main_win.eval("window.__lumen_confirm_mode = %s; true" % json.dumps(mode))


def release_confirm(main_win: ui.Target) -> None:
    main_win.eval(
        """
        (() => {
          if (window.__lumen_orig_confirm) window.confirm = window.__lumen_orig_confirm;
          return true;
        })()
        """
    )


def wait_confirm_text(main_win: ui.Target, timeout: float = 8.0) -> str | None:
    """等按钮的异步流程走到 confirm。

    回收站那个按钮的处理函数是 async 的（先 await 一次计数查询，之后才弹 confirm），
    所以点完之后必须**轮询等待**，不能同步读一次就算。
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        text = main_win.eval("window.__lumen_confirm_text")
        if text:
            return str(text)
        time.sleep(0.2)
    return None


# 「永久删除 N 项」那个按钮：按文案匹配，避免误点到卡片上的单个删除按钮
DANGER_INFO_JS = r"""
(() => {
  const btn = Array.from(document.querySelectorAll('.btn--danger'))
    .find((b) => /永久删除\s*\d+\s*项/.test(b.innerText));
  return btn ? btn.innerText : null;
})()
"""

DANGER_CLICK_JS = r"""
(() => {
  const btn = Array.from(document.querySelectorAll('.btn--danger'))
    .find((b) => /永久删除\s*\d+\s*项/.test(b.innerText));
  if (!btn) throw new Error('找不到「永久删除 N 项」按钮');
  btn.click();
  return true;
})()
"""


# ===========================================================================
# A. 回收站删除范围（§12.1）
# ===========================================================================

def create_trash_tasks(
    main_win: ui.Target, prefix: str, n: int, sink: list[str]
) -> list[str]:
    """造 n 条任务并直接移入回收站。"""
    ids = seed_tasks(main_win, prefix, n, batch=n, sink=sink)
    bulk_delete(main_win, ids)
    return ids


def section_trash_scope(main_win: ui.Target, base_trash: int, sink: list[str]) -> None:
    print("\n===== A. 回收站删除范围（§12.1）=====", flush=True)
    hit_ids: list[str] = []
    miss_ids: list[str] = []

    hit_ids = create_trash_tasks(main_win, HIT_PREFIX, HIT_COUNT, sink)
    miss_ids = create_trash_tasks(main_win, MISS_PREFIX, MISS_COUNT, sink)
    trash_seeded = count_tasks(main_win, TRASH_Q)
    check(
        "两组回收站数据都已就位",
        len(hit_ids) == HIT_COUNT
        and len(miss_ids) == MISS_COUNT
        and trash_seeded == base_trash + HIT_COUNT + MISS_COUNT,
        f"命中组 {len(hit_ids)} + 对照组 {len(miss_ids)}，回收站 {base_trash} → {trash_seeded} 条",
    )

    # ---- 界面：进回收站、搜索命中组 ----
    click_sidebar(main_win, "回收站")
    set_search(main_win, HIT_PREFIX)

    filtered = count_tasks(main_win, HIT_Q)
    check(
        "搜索命中数 = 造的命中条数（前置条件）",
        filtered == HIT_COUNT,
        f"后端 count = {filtered}（期望 {HIT_COUNT}）",
    )

    btn_text = main_win.eval(DANGER_INFO_JS)
    btn_num = extract_int(btn_text, r"永久删除\s*(\d+)\s*项")
    btn_ok = check(
        "按钮上的数字 = 搜索命中的条数（不是回收站总数）",
        btn_num == HIT_COUNT and btn_num != trash_seeded,
        f"按钮文案 {btn_text!r}（命中 {filtered} 条 / 回收站共 {trash_seeded} 条）",
    )

    # ---- 拦截 confirm 读文本（此时 confirm 处于 block，绝不会真删）----
    main_win.eval(DANGER_CLICK_JS)
    confirm_text = wait_confirm_text(main_win) or ""
    conf_num = extract_int(confirm_text, r"只删除筛选结果里的\s*(\d+)\s*项")
    conf_all = extract_int(confirm_text, r"回收站共\s*(\d+)\s*项")
    conf_ok = check(
        "确认弹窗同时写出「筛选结果 N 项 / 回收站共 M 项」",
        conf_num == HIT_COUNT and conf_all == trash_seeded,
        f"弹窗文本：{confirm_text!r}（期望 {HIT_COUNT} / {trash_seeded}）",
    )

    still_there = count_tasks(main_win, HIT_Q)
    check(
        "拦截期间没有发生真实删除（confirm 返回 false）",
        still_there == HIT_COUNT,
        f"命中组仍为 {still_there} 条",
    )

    # ---- 前置断言都过了才真的删 ----
    pre_ok = (
        filtered == HIT_COUNT
        and btn_ok
        and conf_ok
        and conf_num == HIT_COUNT
        and conf_all == trash_seeded
        and len(hit_ids) == HIT_COUNT
        and still_there == HIT_COUNT
    )
    if pre_ok:
        set_confirm_mode(main_win, "accept")
        try:
            main_win.eval(DANGER_CLICK_JS)
            # 等删除真的发生（最多 15 秒），期间 confirm 仍然是我们的实现
            deadline = time.time() + 15
            after_filtered = HIT_COUNT
            while time.time() < deadline:
                after_filtered = count_tasks(main_win, HIT_Q)
                if after_filtered < HIT_COUNT:
                    break
                time.sleep(0.4)
        finally:
            # 立刻切回 block：后面任何确认框都只会被拒绝
            set_confirm_mode(main_win, "block")

        hit_left = count_tasks(main_win, HIT_Q)
        miss_left = count_tasks(main_win, MISS_Q)
        trash_now = count_tasks(main_win, TRASH_Q)
        check("命中的 2 条已被永久删除", hit_left == 0, f"仍剩 {hit_left} 条")
        check(
            "未命中的 3 条原封不动（修复前会被一起删掉）",
            miss_left == MISS_COUNT,
            f"对照组仍剩 {miss_left}/{MISS_COUNT} 条",
        )
        check(
            "回收站总数只减少了命中的条数",
            trash_now == trash_seeded - HIT_COUNT,
            f"{trash_seeded} → {trash_now} 条（期望减少 {HIT_COUNT}）",
        )
    else:
        check(
            "跳过真实删除（前置断言未通过，避免误删用户数据）",
            False,
            "见上面对应的失败项",
        )

    # ---- A 段自己的收尾：剩下的对照组按 id 清掉（不碰用户回收站里的东西）----
    left = purge_ids(main_win, hit_ids + miss_ids)
    rest = count_tasks(main_win, {"search": ANY_PREFIX, "includeDeleted": True, "statuses": []})
    check(
        "A 段造的数据已清理",
        rest == 0,
        f"已永久删除 {left} 条，仍剩 {rest} 条 ZZR3- 数据",
    )


# ===========================================================================
# B. 分页链路（§12.2）
# ===========================================================================

def assert_query_switched(main_win: ui.Target, label: str) -> None:
    """断言列表已经换成 B 的结果、且不含 A 的残留。"""
    deadline = time.time() + 15
    titles: list[str] = []
    note = ""
    while time.time() < deadline:
        titles = card_titles(main_win, 200)
        note = loadmore_note(main_win)
        if (
            len(titles) == RACE_B_COUNT
            and not any(RACE_A_PREFIX in t for t in titles)
            and not any(PREFIX in t for t in titles)
        ):
            break
        time.sleep(0.4)
    stale = [t for t in titles if RACE_A_PREFIX in t or PREFIX in t]
    check(
        f"{label}：列表是 B 的结果（{RACE_B_COUNT} 张卡片）",
        len(titles) == RACE_B_COUNT,
        f"实际 {len(titles)} 张卡片，文案 {note!r}",
    )
    check(
        f"{label}：列表里没有 A 的残留",
        not stale,
        f"残留卡片：{stale[:3]}" if stale else "未发现 A/分页数据的残留",
    )


def section_pagination(main_win: ui.Target, sink: list[str]) -> None:
    print(f"\n===== B. 分页链路（§12.2，{SEED} 条）=====", flush=True)
    print(f"创建 {SEED} 条「{PREFIX}」任务…", flush=True)
    t0 = time.time()
    page_ids = seed_tasks(main_win, PREFIX, SEED, sink=sink)
    check(
        "造数据完成",
        len(page_ids) == SEED,
        f"{len(page_ids)}/{SEED} 条，耗时 {time.time() - t0:.1f}s",
    )
    if len(page_ids) != SEED:
        check("分页断言的前提（2000 条数据）不成立", False, "造数据没成功，跳过后续分页断言")
        return

    click_sidebar(main_win, "全部任务")
    set_search(main_win, PREFIX)

    total = count_tasks(main_win, PAGE_Q)
    check("搜索把范围限定到这批 2000 条上", total == SEED, f"后端 count = {total}")

    # ---- 首屏只渲染一页 ----
    deadline = time.time() + 25
    shown: int | None = None
    shown_total: int | None = None
    while time.time() < deadline:
        shown, shown_total = parse_shown(loadmore_note(main_win))
        if shown == PAGE_SIZE and shown_total == SEED:
            break
        time.sleep(0.4)
    rendered = card_count(main_win)
    check(
        "首屏 DOM 卡片数等于一页（PAGE_SIZE = 200）",
        rendered == PAGE_SIZE,
        f"DOM 里 {rendered} 张卡片，后端总数 {total}",
    )
    check(
        "「已显示 X / 共 Y 条」文案正确",
        (shown, shown_total) == (PAGE_SIZE, SEED),
        f"实际文案 {loadmore_note(main_win)!r}（期望已显示 {PAGE_SIZE} / {SEED} 条；"
        f"注：界面文案里没有「共」字）",
    )

    more = ui.text_of(main_win, ".loadmore button") or ""
    check("存在「加载更多」入口", "加载更多" in more, f"按钮文案：{more!r}")

    # ---- 连续加载，必须能翻到第 501 / 1201 / 2000 条 ----
    for target in LOAD_TARGETS:
        got = load_until(main_win, target)
        check(
            f"连续点「加载更多」能取到第 {target} 条（DOM 卡片数 ≥ {target}）",
            got >= target,
            f"当前 DOM 卡片数 {got}",
        )

    final = card_count(main_win)
    check(
        f"一直取到第 {SEED} 条，不多不少",
        final == SEED,
        f"DOM 卡片数 {final}",
    )
    done_text = ui.text_of(main_win, ".loadmore__done") or ""
    check(
        f"加载到底后界面写明「已加载全部 {SEED} 条」",
        "已加载全部" in done_text and f"{SEED}" in done_text,
        f"实际文案 {done_text!r}",
    )

    # ---- 快速切换搜索词：列表必须是 B 的结果（queryGeneration）----
    print("造两组小数据用于「快速切换搜索词」…", flush=True)
    seed_tasks(main_win, RACE_A_PREFIX, RACE_A_COUNT, batch=RACE_A_COUNT, sink=sink)
    seed_tasks(main_win, RACE_B_PREFIX, RACE_B_COUNT, batch=RACE_B_COUNT, sink=sink)

    # 场景 1：分页请求还在飞的时候切搜索词（真正会走到 queryGeneration 的时序）
    set_search(main_win, PREFIX)
    wait_cards(main_win, PAGE_SIZE, timeout=25)
    click_load_more(main_win)  # 点完不等，立刻切词
    ui.set_react_input(main_win, 'input[type="search"]', RACE_B_PREFIX)
    time.sleep(3.0)
    assert_query_switched(main_win, "分页请求在飞时切到 B")

    # 场景 2：输入 A → 立刻（约 50ms）输入 B
    # （A 的结果这时还没渲染，测的是"防抖 + 代际作废"这条更短的路径）
    ui.set_react_input(main_win, 'input[type="search"]', RACE_A_PREFIX)
    time.sleep(0.05)
    ui.set_react_input(main_win, 'input[type="search"]', RACE_B_PREFIX)
    time.sleep(3.0)
    assert_query_switched(main_win, "输入 A 后立刻切到 B")

    # 场景 3：A 的结果已经渲染出来之后再切 B
    set_search(main_win, RACE_A_PREFIX)
    a_cards = wait_cards(main_win, RACE_A_COUNT, timeout=25)
    check(
        "切换前 A 的结果已经渲染出来（场景 3 的前提）",
        a_cards == RACE_A_COUNT,
        f"A 的卡片数 {a_cards}（期望 {RACE_A_COUNT}）",
    )
    ui.set_react_input(main_win, 'input[type="search"]', RACE_B_PREFIX)
    time.sleep(3.0)
    assert_query_switched(main_win, "A 已落地后切到 B")

    note_uncertain(
        "§12.2 的「快速切换」三组场景断言的是**最终状态**："
        "A 的请求是否恰好在切换的那一刻仍在飞，取决于 IPC 时序，脚本无法强制制造该窗口；"
        "断言只能证明「用户看到的结果属于 B」，不能证明代际作废这条分支真的被执行过。"
    )


# ===========================================================================
# C. 附件删除不跟随链接（§12.3）
# ===========================================================================

def _decode(raw: bytes | None) -> str:
    if not raw:
        return ""
    for enc in ("utf-8", "mbcs", "gbk"):
        try:
            return raw.decode(enc)
        except (UnicodeDecodeError, LookupError):
            continue
    return raw.decode("utf-8", errors="replace")


def make_junction(link: Path, target: Path) -> tuple[int, str]:
    """用 `cmd /C mklink /J <link> <target>` 建目录联接。

    为什么用 junction 而不是 symlink：普通用户即可创建目录联接，
    而 symlink 需要管理员权限或开发者模式，本机不可用。
    """
    proc = subprocess.run(
        ["cmd", "/C", "mklink", "/J", str(link), str(target)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return proc.returncode, _decode(proc.stdout).strip()


def snapshot_names(d: Path) -> set[str]:
    try:
        return {p.name for p in d.iterdir()}
    except OSError:
        return set()


def is_managed_name(name: str) -> bool:
    """文件名是否形如 Lumen 生成的副本 `<uuid><扩展名>`（attachments.rs 的判定）。"""
    stem = name.rsplit(".", 1)[0] if "." in name else name
    try:
        uuidlib.UUID(stem)
        return True
    except ValueError:
        return False


def section_attachment_links(main_win: ui.Target) -> None:
    print("\n===== C. 附件删除不跟随链接（§12.3）=====", flush=True)
    paths = main_win.eval(invoke_js("app_data_paths", {})) or {}
    data_dir = Path(str(paths.get("dataDir") or ""))
    # 这里**不**再要求目录名等于 bundle id：隔离 profile 的目录名是用户随便起的
    # （`LUMEN_TEST_DATA_DIR` 指哪就是哪），按名字判会假失败。
    # "是不是生产目录"由启动时的 verify_isolated_profile() 负责，不在这一段。
    check(
        "拿到数据目录（app_data_paths）",
        bool(str(data_dir)) and data_dir.is_dir(),
        f"dataDir = {data_dir}（生产目录={is_production_data_dir(data_dir, default_production_data_dir())}）",
    )
    attach_dir = data_dir / "attachments"

    # 从建目录、建联接开始就用 try 包住：任何一步抛错都必须走到 finally，
    # 不能把联接或临时文件留在用户的 attachments/ 里。
    made_dir = False
    outside_root: Path | None = None
    control: Path | None = None
    links: list[tuple[Path, Path, Path]] = []
    try:
        if not attach_dir.is_dir():
            attach_dir.mkdir(parents=True, exist_ok=True)
            made_dir = True
            print(f"   （attachments/ 原本不存在，已临时创建：{attach_dir}）", flush=True)
        check("受控附件目录可访问", attach_dir.is_dir(), f"{attach_dir}")

        # 目录外的真实目标：全部造在系统临时目录里，绝不动用户文件
        outside_root = Path(tempfile.mkdtemp(prefix="lumen-zzr3-outside-"))
        dir_a = outside_root / "outside-a"
        dir_b = outside_root / "outside-b"
        dir_a.mkdir()
        dir_b.mkdir()
        victim_a = dir_a / "victim-a.bin"
        victim_b = dir_b / "victim-b.bin"
        victim_c = outside_root / "victim-c.bin"
        for p in (victim_a, victim_b, victim_c):
            p.write_bytes(b"zzr3 junction victim")

        # 三种形态：
        # 1) UUID 命名（无扩展名）的联接 → 目录外的目录
        # 2) UUID + 扩展名（= Lumen 生成副本的命名）的联接 → 目录外的目录
        # 3) UUID + 扩展名的联接 → 目录外的**文件**
        links = [
            (attach_dir / str(uuidlib.uuid4()), dir_a, victim_a),
            (attach_dir / f"{uuidlib.uuid4()}.bin", dir_b, victim_b),
            (attach_dir / f"{uuidlib.uuid4()}.bin", victim_c, victim_c),
        ]
        created_links: list[tuple[Path, Path, Path]] = []
        for link, target, victim in links:
            rc, out = make_junction(link, target)
            ok = rc == 0 and os.path.lexists(link)
            print(
                f"   mklink /J {link.name} → {target}   rc={rc}"
                + (f"，输出：{out}" if out else "")
                + ("" if ok else "（创建失败）"),
                flush=True,
            )
            if ok:
                created_links.append((link, target, victim))
            elif victim is victim_c:
                note_uncertain(
                    "§12.3 的「联接目标直接是一个文件」形态在本机无法创建"
                    f"（mklink /J 返回 {rc}：{out}），这一形态未能验证。"
                )
        check(
            "目录联接创建成功（mklink /J，普通用户权限即可）",
            len(created_links) >= 2,
            f"成功 {len(created_links)}/{len(links)} 个",
        )

        # 对照文件：普通 UUID 命名的孤儿副本，证明扫描**确实执行了**
        control_name = f"{uuidlib.uuid4()}.zzr3tmp"
        control = attach_dir / control_name
        control.write_bytes(b"zzr3 orphan control")

        before_names = snapshot_names(attach_dir)

        scan = main_win.eval(invoke_js("attachment_cleanup_orphans", {}))
        shape_ok = isinstance(scan, dict) and {
            "scanned",
            "kept",
            "removed",
            "skipped",
            "removedFiles",
        } <= set(scan)
        check(
            "attachment_cleanup_orphans 返回完整统计",
            shape_ok,
            f"返回：{scan}",
        )
        removed_files = list(scan.get("removedFiles") or []) if isinstance(scan, dict) else []

        check(
            "对照：普通 UUID 命名的孤儿副本确实被清理（证明扫描真的跑了）",
            (not control.exists()) and control_name in removed_files,
            f"对照文件仍存在={control.exists()}，removedFiles 含它={control_name in removed_files}",
        )
        check(
            "【§12.3】联接指向的目录外文件仍然存在（删除绝不能跟随链接删目标）",
            victim_a.exists() and victim_b.exists() and victim_c.exists(),
            f"victim-a={victim_a.exists()} victim-b={victim_b.exists()} victim-c={victim_c.exists()}",
        )
        check(
            "目录外的目标目录本身仍然存在",
            dir_a.is_dir() and dir_b.is_dir(),
            f"{dir_a} is_dir={dir_a.is_dir()}，{dir_b} is_dir={dir_b.is_dir()}",
        )

        # 联接是否被当成孤儿候选：取决于 Path::is_file() 是否跟随重解析点
        for link, _target, _victim in created_links:
            was_candidate = link.name in removed_files
            still = os.path.lexists(link)
            print(
                f"   联接 {link.name}："
                + (
                    "被当作孤儿候选并已处理（链接自身没了=%s）" % (not still)
                    if was_candidate
                    else "被扫描跳过（未跟随目标；链接仍在=%s）" % still
                ),
                flush=True,
            )
        if not any(link.name in removed_files for link, _t, _v in created_links):
            note_uncertain(
                "§12.3：本机实测这些联接**没有**进入孤儿删除分支"
                "（`cleanup_orphans_impl` 先做 `path.is_file()`，目录联接会返回 false ⇒ 记 skipped）。"
                "因此本次只验证了「扫描没碰目录外的目标文件」，"
                "没有真正走到 `safe_remove_managed_copy()` 的符号链接分支。"
            )

        after_names = snapshot_names(attach_dir)
        gone = before_names - after_names
        stray = sorted(n for n in gone if not is_managed_name(n))
        check(
            "清理只删掉 UUID 命名的托管副本，没有碰用户放进来的文件",
            not stray,
            f"消失的文件：{sorted(gone)}" + (f"；其中非托管命名：{stray}" if stray else ""),
        )
    finally:
        # 逐个清掉联接：只用 os.rmdir（只删链接自身）
        # —— 绝不能用 shutil.rmtree，旧实现会跟随联接把目标目录里的东西删掉
        for link, _target, _victim in links:
            try:
                if os.path.lexists(link):
                    os.rmdir(link)
            except OSError as e:  # noqa: BLE001
                print(f"⚠️ 目录联接未删除（请手工检查）：{link} —— {e}", flush=True)
        if control is not None:
            try:
                if control.exists():
                    control.unlink()
            except OSError as e:  # noqa: BLE001
                print(f"⚠️ 对照文件未删除：{control} —— {e}", flush=True)
        if outside_root is not None:
            shutil.rmtree(outside_root, ignore_errors=True)
        if made_dir:
            try:
                attach_dir.rmdir()
            except OSError:
                pass  # 目录里还有别的东西（正常），保持原样


# ===========================================================================
# D. Provider 的 Key 状态按 provider 独立（§12.4）
# ===========================================================================

SET_AI_PROVIDER_JS = r"""
(() => {
  const rows = Array.from(document.querySelectorAll('.settings label.formrow'));
  const provRow = rows.find((r) => {
    const l = r.querySelector('.formlabel');
    return l && l.innerText.includes('服务商');
  });
  if (!provRow) throw new Error('找不到「服务商」这一行');
  const sel = provRow.querySelector('select');
  if (!sel) throw new Error('「服务商」行里没有 select');
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;
  setter.call(sel, %s);
  sel.dispatchEvent(new Event('change', { bubbles: true }));
  sel.dispatchEvent(new Event('input', { bubbles: true }));
  return sel.value;
})()
"""

READ_AI_PANEL_JS = r"""
(() => {
  const rows = Array.from(document.querySelectorAll('.settings label.formrow'));
  const findRow = (kw) => rows.find((r) => {
    const l = r.querySelector('.formlabel');
    return l && l.innerText.includes(kw);
  });
  const provRow = findRow('服务商');
  const keyRow = findRow('API Key');
  if (!provRow || !keyRow) return null;
  const sel = provRow.querySelector('select');
  const chip = keyRow.querySelector('.formlabel .chip--ok');
  return {
    value: sel ? sel.value : null,
    selectedText: sel && sel.selectedIndex >= 0 ? sel.options[sel.selectedIndex].text : null,
    options: sel ? Array.from(sel.options).map((o) => o.value) : [],
    badge: !!chip,
    badgeText: chip ? chip.innerText : null,
    keyLabel: keyRow.querySelector('.formlabel').innerText,
  };
})()
"""


def section_ai_key_status(main_win: ui.Target) -> None:
    print("\n===== D. Provider 的 Key 状态按 provider 独立（§12.4）=====", flush=True)
    status = main_win.eval(invoke_js("ai_provider_key_status", {}))
    shape_ok = (
        isinstance(status, dict)
        and set(status) == set(PROVIDERS)
        and all(isinstance(v, bool) for v in status.values())
    )
    check(
        "ai_provider_key_status 返回四个 provider 各自的布尔状态",
        shape_ok,
        f"返回：{status}",
    )
    if not shape_ok:
        note_uncertain("§12.4：拿不到密钥状态表（返回结构不符合预期），界面比对未能进行。")
        check("§12.4 的界面比对无法进行", False, f"后端返回 {status!r}")
        return

    distinct = sorted(set(status.values()))
    if len(distinct) == 1:
        only = "已配置" if distinct[0] else "未配置"
        note_uncertain(
            f"§12.4：四个 provider 的密钥状态完全相同（都为「{only}」），"
            "「按 provider 独立」这一行为在本机**无法区分**——"
            "下面的一致性断言只证明「没有缺失、没有溢出」，不构成充分证据；"
            "脚本不会去改凭据管理器来伪造差异。"
        )

    click_sidebar(main_win, "设置")
    click_tab(main_win, "AI")
    ready = main_win.wait_for(
        "!!document.querySelector('.settings label.formrow select')", timeout=15
    )
    check("「设置 → AI」页渲染出服务商下拉框", ready, "AI 面板已就绪")
    if not ready:
        check("§12.4 的界面比对无法进行（AI 面板没渲染出来）", False, "见上一项")
        return

    original = main_win.eval(READ_AI_PANEL_JS) or {}
    print(f"   当前服务商：{original.get('value')!r}（{original.get('selectedText')!r}）", flush=True)

    for provider in PROVIDERS:
        main_win.eval(SET_AI_PROVIDER_JS % json.dumps(provider))
        time.sleep(0.35)  # React 的 setState → 重渲染是异步的，必须等一拍再读
        state = main_win.eval(READ_AI_PANEL_JS) or {}
        expect = bool(status[provider])
        switched = state.get("value") == provider
        badge = bool(state.get("badge"))
        detail = (
            f"下拉框实际值 {state.get('value')!r}（{state.get('selectedText')!r}），"
            f"「已配置」徽标={'有' if badge else '无'}，"
            f"后端 {provider}={'已配置' if expect else '未配置'}"
        )
        if not switched:
            detail += "；下拉框没有切换过去（可能是该 provider 缺少默认配置）"
        if len(distinct) == 1:
            detail += "（本机四个 provider 状态相同，本条无法区分）"
        check(
            f"切到 {provider} 后「已配置」徽标 = 后端该 provider 的状态",
            switched and badge == expect,
            detail,
        )

    # 还原成进入前的服务商（只改组件本地 state，不落库、不碰凭据）
    if original.get("value"):
        main_win.eval(SET_AI_PROVIDER_JS % json.dumps(str(original["value"])))
        time.sleep(0.2)


# ===========================================================================
# 主流程
# ===========================================================================

def main() -> int:
    args = parse_args()

    # 隐藏自测入口：只跑判定函数的用例，**不连 CDP、不碰应用**（§29）
    if args.self_test_profile_check:
        return run_profile_check_selftest()

    print(f"连接主窗口（调试端口 {PORT}）…", flush=True)
    main_win = ui.connect(PORT, want="main")
    print("已连接。\n", flush=True)

    # ---- 第一件事：确认不是生产 profile。不通过就立刻退出，绝不造任何数据 ----
    ok, data_dir = verify_isolated_profile(main_win, args)
    if not ok:
        print(
            "\n已退出（退出码 1）：未执行任何写操作、未创建任何任务、未改动任何数据。"
            "（本脚本没有任何允许在生产目录上跑的开关。）",
            flush=True,
        )
        return 1

    base_all = count_tasks(main_win, ALL_Q)
    base_trash = count_tasks(main_win, TRASH_Q)
    print(f"基线：全部任务（含回收站）{base_all} 条，回收站 {base_trash} 条\n", flush=True)

    created: list[str] = []  # 所有脚本造的任务 id（每批创建后立刻登记）
    armed = False
    try:
        # 第二层防护的起点：接管 confirm，此后**任何**确认框都不会被自动接受
        # （第一层是上面的隔离 profile 校验，已经过了才会走到这里）
        arm_confirm(main_win)
        armed = True

        run_section("A", section_trash_scope, main_win, base_trash, created)
        run_section("B", section_pagination, main_win, created)
        run_section("C", section_attachment_links, main_win)
        run_section("D", section_ai_key_status, main_win)

    finally:
        # ---------------- 清理（无论成败都要执行）----------------
        print("\n清理脚本造的数据…", flush=True)
        try:
            if armed:
                # 清理期间也保持接管：任何残留的异步确认框都只会被拒绝
                set_confirm_mode(main_win, "block")
            set_search(main_win, "", wait=0.8)
            click_sidebar(main_win, "全部任务")

            ids = list(dict.fromkeys(created))  # 去重但保持顺序
            for i in range(0, len(ids), 500):
                chunk = ids[i : i + 500]
                print(f"   软删 {len(chunk)} 条…", flush=True)
                bulk_delete(main_win, chunk)
            removed = purge_ids(main_win, ids)
            print(f"   已永久删除 {removed}/{len(ids)} 条", flush=True)

            set_search(main_win, "")
            click_sidebar(main_win, "今天")
        except Exception as e:  # noqa: BLE001
            print(f"⚠️ 清理过程出错（请手工检查 {ANY_PREFIX} 前缀的任务）：{e}", flush=True)
        finally:
            if armed:
                try:
                    release_confirm(main_win)
                except Exception as e:  # noqa: BLE001
                    print(f"⚠️ 未能还原 window.confirm：{e}", flush=True)

    # ---------------- 收尾断言 ----------------
    left = count_tasks(main_win, {"search": ANY_PREFIX, "includeDeleted": True, "statuses": []})
    check("脚本造的数据已清空", left == 0, f"仍然存在 {left} 条")

    now_all = count_tasks(main_win, ALL_Q)
    now_trash = count_tasks(main_win, TRASH_Q)
    check(
        "用户原有数据未被改动",
        now_all == base_all and now_trash == base_trash,
        f"全部 {base_all}→{now_all}，回收站 {base_trash}→{now_trash}",
    )

    passed = sum(1 for _, ok, _ in results if ok)
    print(f"\n===== {passed}/{len(results)} 项通过 =====", flush=True)
    for name, ok, detail in results:
        if not ok:
            print(f"❌ {name} —— {detail}", flush=True)
    if uncertain:
        print("\n----- 无法确认（不计入通过率，供人工判断）-----", flush=True)
        for text in uncertain:
            print(f"• {text}", flush=True)

    print(f"\n本次运行的 profile：{data_dir or '（未知）'}", flush=True)

    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
