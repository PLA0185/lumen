# 仓库工作约定（AGENTS.md）

本文件是本仓库的**长期约定**，无论是人还是自动化代理，在这里动手前都应先读它。
内容来自用户明确提出的要求与项目已经踩过的坑。

---

## 1. 每轮工作结束必须留一份工作记录（用户明确要求）

> "以后任务做完了之后同步整理一份文档到 GitHub，用于记录你做了什么没做到什么。"

- 记录写在 **`docs/work-log.md`**，按轮次**追加**（不要覆盖历史）。
- 每一条必须有四部分：**做了 / 没做到 / 怎么验证的 / 相关文档**。
- 约定细节写在 `docs/work-log.md` 开头，新增轮次前先看那里。
- **没做到的要如实写**。没验证到的写"未验证"并说明原因（例如"本机没有可用 API Key"），
  不许用"后续优化""暂不支持"这类含糊措辞糊过去。
- 同一轮里发现但没修的问题也要记进去，避免下一轮重新踩。
- 记录写完后，**随代码一起提交并推送到 `PLA0185/lumen`**。
- 如果这一轮有深度产出（整改报告、调研报告、设计说明），放到 `docs/` 下单独成文，
  并在 `docs/work-log.md` 与 README 的「文档索引」里链接过去。

---

## 2. 不许假实现、假测试、假结论

- 不写"按钮能点但功能没落地"的界面；未实现的功能在界面上**明确标注"尚未实现"**，
  而不是放一个看起来能用的占位。
- 不允许遇到错误后静默返回伪成功。核心数据路径（任务写入、提醒关联、重复规则、
  迁移、备份、恢复、永久删除）**必须向上返回错误**，只有 UI 同步、托盘刷新、
  日志清理这类非关键路径才允许 best-effort。
- 测试里不许硬编码结果来"通过"。测试要能真的失败——新增修复时，先确认它在修复前是红的。
- 报告里不许写"测试通过"却没真的跑过。没跑就写"未验证"。

## 3. 数据安全底线

- **不破坏用户数据**：结构变更一律走 `src-tauri/migrations/` 下的正式迁移；
  不允许要求用户删库、清空配置或重装。
- 迁移前必须做**一致性快照**（WAL 模式下不能只复制 `.db`，见 `db.rs` 的
  `pre_migration_snapshot`）；快照失败要**中止升级**而不是冒险继续。
- **密钥、数据库、日志、构建产物绝不入库**（`.gitignore` 已覆盖，提交前仍要自查）：

  ```powershell
  git status --porcelain
  git ls-files | Select-String '\.(key|keystore|db|log|exe|sig)$|^dist/|^src-tauri/target/'
  git grep -l "minisign encrypted secret key"
  ```

  > 最后一条会命中**本文件**和 `docs/remediation-report.md` —— 那两处只是把这句话当成
  > 自查模式写了下来，命中它们是正常的。要判定是不是真泄露，看命中处**有没有跟着
  > 一大段 base64 与 `untrusted comment:` 行**；只有说明文字就是安全的。

- 更新签名私钥存放在仓库之外（`%USERPROFILE%\.tauri\lumen-updater.key`）；
  **密码不再明文保存**（第二轮整改任务书 §9）：它用 Windows DPAPI 加密存放在同目录的
  `lumen-updater-password.dpapi`，只有本机当前 Windows 账户能解密。
  统一用脚本操作，不要手抄密码：

  ```powershell
  pwsh -File tools/updater-secret.ps1 -Status   # 看私钥/密码是否就绪（不打印密码）
  pwsh -File tools/updater-secret.ps1 -Set      # 交互式录入（不回显），DPAPI 加密落盘
  pwsh -File tools/updater-secret.ps1 -Build    # 载入签名信息并执行 pnpm tauri build
  ```

  **私钥丢了就再也发不了更新**；密码密文只能在生成它的那台机器/那个账户上解开，
  换机器需要重新 `-Set` 录一次。CI 上不用这套：走 GitHub Actions Secrets。

  > 如实说明这道防护的边界：DPAPI 密文面向"文件被拷走 / 被别的账户读到 /
  > 误提交进仓库"，**挡不住**本机同一账户下的恶意进程——
  > 那种威胁模型需要硬件密钥，本轮没做。

## 4. 提交习惯

- 每类整改**独立 commit**，不要一个超大 commit 混在一起。
- Commit message 用中文，格式形如 `fix(inbox): ...` / `test(reminders): ...` / `docs: ...`。
- 推送前跑一遍下面的门禁。

## 5. 提交前的门禁（全绿才算完成）

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test          # 前端 101 项
pnpm build         # 必须先跑，见下方说明
pnpm lint
cd src-tauri
cargo fmt --check
cargo test --lib   # 323 项（会随迭代增长）
cargo clippy --all-targets --all-features -- -D warnings
```

> **`pnpm build` 不是可省的一步**：上面最后一条带 `--all-features`，
> 它会启用 tauri 的 `custom-protocol`；在这个 feature 下
> `tauri::generate_context!()` 要把前端产物 `../dist` **嵌进二进制**，
> 目录不存在就直接 proc macro panic：
> `The frontendDist configuration is set to "../dist" but this path doesn't exist`。
> 表现是"本地莫名其妙过了、CI 却红"（本机往往早就构建过前端）。
> CI 的 rust job 因此也先跑 `pnpm build`。

CI（`.github/workflows/ci.yml`）会在 push 到 main 时真实执行同一套检查，
**以 CI 的结果为准**：本地过了但 CI 红了，等于没完成。

> **clippy 没有任何豁免**（第二轮整改任务书 §8）：此前 CI 这里带着
> `-A clippy::too_many_arguments -A clippy::type_complexity` 两条豁免，
> 而文档写的是"所有警告全部禁止"——两者不是同一套规则。
> 现已改为与文档一致的完整门禁（本地命令与 CI **完全相同**，含 `--all-features`）。

---

## 6. 这个项目特有的一些坑（都已踩过，别再踩）

| 坑 | 说明 |
| --- | --- |
| `Separated::push` 自带分隔符 | `sep.push("col = ").push_bind(v)` 会生成 `col = , ?1`；列名片段之后必须用 `push_bind_unseparated` |
| 空表上的 `COALESCE(MAX(x),0)+1` | SQLite 会把它判成 INTEGER，sqlx 按 f64 解码直接报错；查询要 `CAST(... AS REAL)` |
| `projectId: null` ≠ "项目为空" | 过 IPC 会变成 `None`，与"不限制"同义；用显式的 `withoutProject` |
| 分页排序必须全序 | `sort_order` 相同会导致翻页重复/漏项，`ORDER BY` 末尾要追加 `id` |
| 悬浮窗顶栏既是拖动区又有按钮 | `mousedown` 会先触发 `startDragging()`，按钮的 `click` 永不发生；起拖时要排除交互元素 |
| 打印报告不能放进 `.app` | 导出 PDF 时用 `body[data-print] .app { display:none }` 隐藏主界面，报告在里面会被一起隐藏 → 空白 PDF |
| Tauri 托盘/窗口回调是同步上下文 | 在里面 `block_on` 查库会因 tokio 嵌套 panic；菜单只读内存缓存 |
| `installMode` 用 `quiet` | `passive`（`/P`）在应用自触发升级时出现过"文件没被替换"，`/S` 正常 |
| 实机验收用 CDP | Windows UI Automation 看不到 WebView2 内部元素；用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` + `tools/ui_drive.py`，断言按钮可点要派发**真实鼠标事件** |
| `cargo clippy --all-features` 需要 `dist/` | `--all-features` 启用 tauri 的 `custom-protocol`，`generate_context!()` 会把 `frontendDist`（`../dist`）嵌进二进制；目录不存在就 proc macro panic。**先 `pnpm build`** —— 这正是"本地绿、CI 红"的那次根因 |
| 列表分页用 OFFSET，深分页是 O(n²) | 报告导出曾因此在 10 万条时耗时 56 秒（改一次取全量后 1.6 秒）。列表仍是 OFFSET，条数很大时才需要换 keyset 分页 |
| 前端 `setSearch` 会自动刷新 | 搜索输入带 250ms 防抖并重新查询（条件变化必须把 offset 归零）；再往手动加"回车才刷新"会重复请求 |
