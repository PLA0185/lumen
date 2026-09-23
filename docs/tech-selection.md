# 技术选型记录

> 任务书 §2.1 要求「先确定桌面框架、前端技术、本地数据库和打包方式，形成简短的选型记录」。
> 本文记录**实际采用**的方案、依据与已知代价。依据来自官方文档调研（见 [`README.md`](./README.md)
> 索引）与**本机实测**，二者都会标明来源。

## 0. 开发环境实测基线

以下为实际探测结果，不是假设值：

| 项目 | 实测值 | 来源 |
| --- | --- | --- |
| 操作系统 | Windows 11 专业版 10.0.26200 | `Get-CimInstance Win32_OperatingSystem` |
| CPU / 内存 | Intel i5-14400（10 核 16 线程）/ 15.7 GB | WMI |
| Rust | 1.98.1，host `x86_64-pc-windows-msvc` | `rustc -vV` |
| MSVC 链接器 | Build Tools 14.51.36231（`link.exe` 已就位） | 目录探测 |
| Windows SDK | 10.0.26100.0 | 目录探测 |
| WebView2 Runtime | 153.0.4234.48 | 注册表 `EdgeUpdate\Clients` |
| Node / pnpm | v24.21.0 / 12.5.1 | `node -v`、`pnpm -v` |
| Git | 2.55.0.windows.5 | `git --version` |

**编译链连通性已实测**：用最小 Rust 程序 `rustc main.rs -o linktest.exe` 成功产出可执行文件
并打印输出，确认 MSVC 链接器与 SDK 可用（而非仅凭目录存在推断）。

## 1. 桌面框架：Tauri 2（而非 Electron）

**选择**：Tauri 2.11.6

**依据**（逐条对应任务书明确点名的 Windows 能力，均已从官方源码/文档核实签名）：

| 任务书要求 | Tauri 2 对应 API | 核实结果 |
| --- | --- | --- |
| 窗口始终置顶（§8.1） | `set_always_on_top(bool)` | 签名已核实 |
| 鼠标穿透（§8.2） | `set_ignore_cursor_events(bool)` | 签名已核实，Windows 无平台限制注释 |
| 不透明度（§8.3） | 窗口 `transparent` 配置 + 前端 CSS | 已核实；**Windows 无需额外 Cargo feature**（`macos-private-api` 仅 macOS） |
| 任务栏显隐（§8.5） | `set_skip_taskbar(bool)` | 已核实（macOS 不支持，本项目只针对 Windows） |
| 系统托盘（§8.6） | `TrayIconBuilder` + `tray-icon` feature | 已核实 feature 名 |
| 系统通知（§4.3） | `tauri-plugin-notification` | 已核实底层走 WinRT Toast；**点击回调桌面端不支持**（见下） |
| 多显示器断连找回（§8） | `available_monitors()` / `monitor_from_point()` | 已核实签名 |
| 安装包（§12） | NSIS / MSI bundle | 已核实输出路径与卸载数据行为 |

**相对 Electron 的决定性理由**：
1. **体积与资源**：Tauri 使用系统 WebView2（本机已装 153.0.4234.48），不打包 Chromium；
   Electron 每个应用都要内置一份浏览器运行时。
2. **Windows 原生能力**：窗口穿透、跳过任务栏、托盘这些能力在 Tauri 是 Rust 直接调 Win32；
   Electron 需要 `setIgnoreMouseEvents` 等且透明窗口在 Windows 上限制更多。
3. **权限最小化**（§10）：Tauri 的 capability 机制默认拒绝一切 IPC 权限，必须显式声明；
   Electron 的渲染进程默认拥有 Node 集成，隔离需额外配置。

**已知代价（如实记录）**：
- 必须依赖 WebView2 运行时。Windows 11 已内置，Windows 10 需 1803+ 且可能需分发安装器；
  Tauri 的 NSIS 包支持自动下载安装 WebView2（已核实 `installer.nsi` 中存在该逻辑）。
- Rust 编译首次构建较慢（本项目首次 `cargo check` 需编译约 400 个 crate）。
- **中文/非 ASCII 路径：官方文档完全未涉及，属未核实项**。本项目路径 `D:\Todo` 为纯 ASCII，
  规避了该风险；若用户在其他路径构建遇到问题，此处是首要排查方向。

## 2. 前端：React 19 + TypeScript + Vite

**选择**：React 19.3.0、TypeScript 5.9.x、Vite 8.3.0、`@vitejs/plugin-react` 6.1.1

**依据**：
- Tauri 官方 React + TS 模板即使用 Vite + `@vitejs/plugin-react`，是官方验证过的组合。
- `@vitejs/plugin-react` 6.x 的 peer 要求是 **Vite ^8.0.0**——这是实测发现的真实约束
  （初版误用 Vite 7 会导致 peer 冲突），因此 Vite 锁定在 8.x 线。
- **TypeScript 刻意停在 5.9.x 而非最新的 7.0.2**：官方 Tauri React 模板锁定的仍是
  `typescript ~6.0.3`，尚未跟进 TS 7。本项目优先保证构建可复现，不追逐最新大版本。
  后续可在独立分支验证 TS 7 后再升级。

**Vite 配置的关键项**（均来自官方模板/指南，已核实）：
`clearScreen: false`（保留 Rust 编译错误不被清屏）、`port: 1420` + `strictPort: true`
（Tauri `devUrl` 依赖固定端口）、`hmr` 走 1421、`watch.ignored: ['**/src-tauri/**']`。
> 注：官方模板与指南页的端口值不一致（模板 1420、指南 5173），本项目以**模板**为准，
> 并在 `tauri.conf.json` 的 `devUrl` 中显式写死 `http://localhost:1420`。

## 3. 本地数据库：SQLite + sqlx（不使用 tauri-plugin-sql）

**选择**：`sqlx` 0.9.0（features: `runtime-tokio`, `sqlite`, `macros`, `migrate`,
`chrono`, `uuid`, `json`）

**为什么不使用官方 `tauri-plugin-sql`**（这是一个有依据的取舍）：
- 核实发现该插件内部依赖 **sqlx 0.8**，而 crates.io 上 sqlx 最新稳定为 **0.9.0**。
  同时使用两者会在依赖树中出现**两个大版本的 sqlx**。
- 官方文档**未给出**"Rust 侧应该用插件还是直接用 sqlx"的推荐做法（已核实，属文档空白）。
- 本项目的所有数据库访问都在 Rust 侧（前端只经 IPC 调用业务命令），
  插件的价值（让前端直接发 SQL）**对本项目无用**，却引入版本分裂风险。
- 因此：**Rust 侧直接依赖 sqlx**，前端不获得任何 SQL 执行能力——
  这同时更符合 §10「Web 内容与本地特权隔离」。

**由此得到的架构收益**：前端无法构造任意 SQL，所有写操作都必须经过 Rust 侧的
用例函数，校验、事务与字段语义（如三个时间字段的区分）无法被绕过。

**关键已核实事实**：
- MSRV = 1.94.0 → 本项目的 `rust-version` 已设为 1.94。
- `sqlite` feature **等同于 `sqlite-bundled`**，即默认从源码编译 SQLite（需要 C 工具链，
  本机 MSVC 已具备）；好处是版本可控、不依赖用户系统 SQLite。
- `migrate!` 宏需要 `macros` + `migrate` 两个 feature，默认读取相对 `Cargo.toml` 的 `./migrations`。

**存储策略**：
- 启用 **WAL** 日志模式（读并发更好、崩溃恢复更稳）。
- 显式开启 `foreign_keys`（SQLite 默认关闭，必须每连接设置；已在连接池选项中固定）。
- 写操作全部走事务（`Db::with_tx`），避免"留下半条规则"（§9）。
- 迁移前自动备份数据库文件（§9「迁移有备份」）。
- 备份使用 `VACUUM INTO` 生成单文件一致性快照，而不是直接复制 `.db`
  （后者可能漏掉尚在 WAL 中的内容）。

## 4. 打包：NSIS（不用 MSI）

**选择**：`bundle.targets = ["nsis"]`，`installMode = "currentUser"`

**依据（已核实）**：
- **NSIS 不需要额外系统组件**，Tauri 打包时自动下载 NSIS 3.11 与 `nsis_tauri_utils`。
- **MSI(WiX) 需要 Windows 的 VBSCRIPT 可选功能**，否则构建失败。本机未验证该功能是否启用，
  因此不选 MSI，避免引入不确定的构建前置条件。
- **卸载时数据保留/删除**（§10 明确要求）：NSIS 卸载页自带 **"Delete app data" 复选框**，
  **默认不勾选 = 保留用户数据**；勾选且非更新模式时才删除 `%APPDATA%\<identifier>`。
  已从 `installer.nsi` 源码逐行核实（`DeleteAppDataCheckbox` 变量与 `RmDir /r` 分支）。
- MSI 是**升级友好**的格式，若将来需要企业分发再补充；当前 `currentUser` 模式
  不需要管理员权限，更适合个人任务管理软件。

**输出路径**（已核实的命名规则）：
`src-tauri/target/release/bundle/nsis/<productName>_<version>_<arch>-setup.exe`

## 5. 数据目录位置

**选择**：`app.path().app_data_dir()` → Windows 下 `%APPDATA%\com.pla0185.aitodo\`

**依据**：
- 任务书 §10 要求"卸载后的数据保留/删除选项"。若把数据库放在**安装目录**，
  NSIS 卸载会把安装目录整个删掉，用户数据随之丢失，且无法提供选择。
- 放在 `%APPDATA%\<identifier>` 正好匹配 NSIS "Delete app data" 复选框的删除目标
  （已核实其删除的正是 `$APPDATA\${BUNDLEID}`，其中 BUNDLEID 即 identifier）。
- 目录布局：
  ```
  %APPDATA%\com.pla0185.aitodo\
    ├── aitodo.db          SQLite 数据库
    ├── aitodo.db-wal      WAL 日志
    ├── backups\           自动与手动备份（含迁移前备份）
    ├── attachments\       受控存储的附件
    └── logs\              运行日志（不含密钥与任务正文）
  ```

## 6. 时间与时区策略

**选择**：**全部时间以 UTC ISO-8601 存储为 TEXT；本地时区仅在展示层使用。**

**依据**（对应 §4.3「区分仅日期和精确时间」、§5「日期、时间和时区的存储方式须写入设计说明」）：

1. **固定宽度 UTC 字符串可直接字典序比较**，SQLite 无需时间类型即可正确排序与区间查询。
   这一性质有单元测试锁定（`src/lib/datetime.test.ts`）。
2. **"仅日期"与"精确时间"用 `has_planned_time` / `has_due_time` 布尔位区分**，
   而不是用"时间部分是否为 00:00"这种脆弱约定。仅日期任务的逾期判断取**当日结束**，
   因此不会像 §4.3 明确禁止的那样"把全天任务默认当作凌晨到期"。
3. **"今天/本周"的边界由前端按用户本地时区计算**，再传 UTC 给后端
   （见 `today_overview` 命令签名）。后端不做任何本地时区假设——
   这样 `chrono-tz` 在 Rust 侧只用于重复规则的 `tzid` 展开，不用于"今天"的判定。
4. 表单解析的本地时间经 `combineDateTime()` 转 UTC 后送后端；后端**拒绝**不带时区的时间串
   （纯 `2026-09-23` 会被拒绝，因为其时区语义不明确）。有测试覆盖。

## 7. 尚未选定的项目（如实标注）

| 项目 | 状态 | 说明 |
| --- | --- | --- |
| 日历视图库 | **未定** | FullCalendar v7 是破坏性重组（daygrid/timegrid/list/interaction 已并入 core，Premium 更名且起价 $480）；本项目重复规则自有实现，不急于引入。候选：`@fullcalendar/*` 7.1.0（MIT）或 `react-big-calendar` 1.20.0 |
| 拖拽排序库 | **暂用 dnd-kit 经典版** | `@dnd-kit/core` 6.3.1 自 2024-12 起无新发布，开发重心转向 beta 的 `@dnd-kit/react`。经典版 peer 覆盖 React 19，可用；已在依赖中锁定 |
| PDF 导出 | **未定，倾向前端生成** | Rust 侧 `printpdf` 对非 ASCII 直接乱码（官方 issue 原文 "all non-ASCII text is mojibake'd"），中文需自嵌字体且 CJK 子集化有未解决 bug；`genpdf` 停更于 2021。**前置验证项**：Tauri/WebView2 上 `window.print()` 的确切行为尚未核实 |
| Markdown 渲染 | 已选 react-markdown + `rehype-sanitize` 6.0.0 | 默认白名单即 GitHub 渲染白名单；代码高亮与数学公式**默认会被丢弃**，需按官方示例扩展 schema 后才能支持 |
| 云同步服务端 | **本轮不做**（用户决定） | 数据模型已预留 `sync_rev` / `sync_state` 字段，避免将来加同步时做破坏性迁移 |
| 自动更新 | 未启用（`createUpdaterArtifacts: false`） | 需要签名密钥与分发端点；未配置前不启用，避免留下无法工作的更新按钮 |

## 8. 依赖版本锁定策略

任务书 §2.4 要求「依赖版本锁定」。实际做法：

- **Rust 侧**：提交 `Cargo.lock`（应用而非库，锁文件应入库）；版本要求写 `"2"` / `"0.9"`
  这类兼容范围，由锁文件保证可复现。
- **前端侧**：`.npmrc` 中设 `save-exact=true`；提交 `pnpm-lock.yaml`。
- **构建脚本白名单**：pnpm 12 默认拒绝执行依赖的 postinstall 脚本（防供应链攻击），
  因此 `pnpm-workspace.yaml` 中显式批准 `esbuild` 与 `@tauri-apps/cli` 两个**构建必需**项，
  其余一律保持禁止。这是安全与可构建性的平衡点，不是随意放行。

## 9. 许可证合规

| 依赖 | 许可证 |
| --- | --- |
| Tauri 2 / 官方插件 | MIT 或 Apache-2.0 |
| React / React DOM | MIT |
| Vite / Vitest | MIT |
| TypeScript | Apache-2.0 |
| sqlx | MIT 或 Apache-2.0 |
| tokio | **MIT（单许可）** |
| reqwest / keyring / thiserror / anyhow | MIT 或 Apache-2.0 |
| rrule.js | BSD-3-Clause |
| date-fns / zustand / zod | MIT |
| echarts(未采用) / recharts | MIT |
| csv (Rust) | Unlicense 或 MIT |
| FullCalendar Premium | **专有，起价 $480 —— 不使用** |

**结论**：当前依赖树全部为宽松许可，无非商业限制，可自由分发安装包。
`tools/make-icon.mjs` 以纯代码合成图标，不含任何第三方素材（§1「独立设计」要求）。
