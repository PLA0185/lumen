# Tauri 2 事实核查报告（Windows 桌面 Todo 应用）

> **访问日期：2026-09-23**（本文件所有 URL 的访问日期均为 2026-09-23）
> **方法**：版本号取自包注册表官方发布数据（crates.io API / npm registry）与官方仓库源码；API 签名、配置项、打包行为取自官方仓库源码（tauri-apps/tauri `2.11` 分支、tauri-apps/plugins-workspace `v2` 分支）并与 v2.tauri.app 官方文档交叉核对。
> **规则**：凡官方文档与源码均未明确者，一律写 **未核实**，不作推测。任何版本号均非记忆产物，均附来源。

---

## 0. 结论速览（可直接落地的依赖）

```toml
# src-tauri/Cargo.toml
[build-dependencies]
tauri-build = { version = "2.6.3", features = [] }

[dependencies]
tauri = { version = "2.11.6", features = ["tray-icon"] }
tauri-plugin-sql = { version = "2.4.1", features = ["sqlite"] }
tauri-plugin-notification = "2.4.0"
tauri-plugin-global-shortcut = "2.3.2"
tauri-plugin-autostart = "2.5.1"
tauri-plugin-store = "2.4.5"
tauri-plugin-dialog = "2.7.3"
tauri-plugin-fs = "2.5.2"
tauri-plugin-opener = "2.5.5"
tauri-plugin-updater = "2.12.0"
tauri-plugin-single-instance = "2.4.5"   # 仅 Rust 侧，无 npm 包
tauri-plugin-window-state = "2.4.1"
tauri-plugin-log = "2.9.2"
```

```jsonc
// package.json（devDependencies / dependencies）
"@tauri-apps/cli": "2.11.5",
"@tauri-apps/api": "2.11.1",
"@tauri-apps/plugin-sql": "2.4.1",
"@tauri-apps/plugin-notification": "2.4.0",
"@tauri-apps/plugin-global-shortcut": "2.3.2",
"@tauri-apps/plugin-autostart": "2.5.1",
"@tauri-apps/plugin-store": "2.4.5",
"@tauri-apps/plugin-dialog": "2.7.3",
"@tauri-apps/plugin-fs": "2.5.2",
"@tauri-apps/plugin-opener": "2.5.5",
"@tauri-apps/plugin-updater": "2.12.0",
"@tauri-apps/plugin-window-state": "2.4.1",
"@tauri-apps/plugin-log": "2.9.2"
```

**最低 Rust 版本（MSRV）：1.77.2** —— 来源：[tauri 仓库 2.11 分支根 Cargo.toml `[workspace.package] rust-version = "1.77.2"`](https://github.com/tauri-apps/tauri/blob/2.11/Cargo.toml)、[项目模板 Cargo.crate-manifest](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-cli/templates/app/src-tauri/Cargo.crate-manifest)。

---

## 1. Tauri 2 当前稳定版本

| 包 | 通道 | 确切版本 | 来源（访问日期 2026-09-23） |
| --- | --- | --- | --- |
| `tauri` | crate | **2.11.6** | [crates.io API `max_stable_version`](https://crates.io/api/v1/crates/tauri)；[2.11 分支 Cargo.toml `version = "2.11.6"`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/Cargo.toml) |
| `tauri-build` | crate | **2.6.3** | [crates.io API](https://crates.io/api/v1/crates/tauri-build) |
| `tauri-cli` | crate | **2.11.5** | [crates.io API](https://crates.io/api/v1/crates/tauri-cli) |
| `@tauri-apps/cli` | npm | **2.11.5** | [npm registry `latest`](https://registry.npmjs.org/@tauri-apps/cli/latest) |
| `@tauri-apps/api` | npm | **2.11.1** | [npm registry `latest`](https://registry.npmjs.org/@tauri-apps/api/latest)；[2.11 分支 packages/api/package.json](https://github.com/tauri-apps/tauri/blob/2.11/packages/api/package.json) |

**注意不稳定线**：`tauri` 的 `max_version` 为 `3.0.0-alpha.2`，`tauri-cli` 为 `3.0.0-alpha.2`，插件同步发布 `3.0.0-alpha.1`（发布时间 2026-09-21）。**α 版本不是稳定版**，本报告全部结论以 2.x 稳定线为准。

来源：[crates.io `tauri` 版本列表](https://crates.io/api/v1/crates/tauri)、[GitHub Release tauri-v3.0.0-alpha.2](https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.2)、[plugins-workspace release sql-v3.0.0-alpha.1](https://github.com/tauri-apps/plugins-workspace/releases)。

官方 CLI 安装方式（原文）：

```
npm="npm install --save-dev @tauri-apps/cli@latest"
cargo='cargo install tauri-cli --version "^2.0.0" --locked'
```
来源：[官方 CLI 参考 `_cli.mdx`](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/reference/_cli.mdx)

---

## 2. Tauri 2 官方插件：版本与包名

官方插件总览页：[https://v2.tauri.app/plugin/](https://v2.tauri.app/plugin/)（访问 2026-09-23）。

### 2.1 逐一核实结果

| 插件 | crate 名 | crate 版本 | npm 包名 | npm 版本 |
| --- | --- | --- | --- | --- |
| SQL (SQLite) | `tauri-plugin-sql` | **2.4.1** | `@tauri-apps/plugin-sql` | **2.4.1** |
| Notification | `tauri-plugin-notification` | **2.4.0** | `@tauri-apps/plugin-notification` | **2.4.0** |
| Global Shortcut | `tauri-plugin-global-shortcut` | **2.3.2** | `@tauri-apps/plugin-global-shortcut` | **2.3.2** |
| Autostart | `tauri-plugin-autostart` | **2.5.1** | `@tauri-apps/plugin-autostart` | **2.5.1** |
| Store | `tauri-plugin-store` | **2.4.5** | `@tauri-apps/plugin-store` | **2.4.5** |
| Dialog | `tauri-plugin-dialog` | **2.7.3** | `@tauri-apps/plugin-dialog` | **2.7.3** |
| FS | `tauri-plugin-fs` | **2.5.2** | `@tauri-apps/plugin-fs` | **2.5.2** |
| Opener | `tauri-plugin-opener` | **2.5.5** | `@tauri-apps/plugin-opener` | **2.5.5** |
| Shell | `tauri-plugin-shell` | **2.3.6** | `@tauri-apps/plugin-shell` | **2.3.6** |
| Updater | `tauri-plugin-updater` | **2.12.0** | `@tauri-apps/plugin-updater` | **2.12.0** |
| Single Instance | `tauri-plugin-single-instance` | **2.4.5** | **无 npm 包（未发布）** | — |
| Window State | `tauri-plugin-window-state` | **2.4.1** | `@tauri-apps/plugin-window-state` | **2.4.1** |
| Log | `tauri-plugin-log` | **2.9.2** | `@tauri-apps/plugin-log` | **2.9.2** |
| （参考）Process | `tauri-plugin-process` | 2.3.1 | `@tauri-apps/plugin-process` | 2.3.1 |
| （参考）OS Info | `tauri-plugin-os` | 2.3.2 | `@tauri-apps/plugin-os` | 2.3.2 |
| （参考）Stronghold | `tauri-plugin-stronghold` | 2.3.2 | `@tauri-apps/plugin-stronghold` | （未逐一核实） |

来源：[crates.io API 各 crate](https://crates.io/api/v1/crates/tauri-plugin-sql)、[npm registry 各包](https://registry.npmjs.org/@tauri-apps/plugin-sql/latest)、[plugins-workspace v2 分支各 `Cargo.toml`](https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins)。

- `@tauri-apps/plugin-single-instance` 与 `@tauri-apps/plugin-secure-storage` 在 npm registry 上**返回未发布**（single-instance 是纯 Rust 插件，官方文档 [Single Instance](https://v2.tauri.app/plugin/single-instance/) 也只给 Rust 用法）。
- 插件工作区统一要求 `rust-version = "1.77.2"`，依赖 `tauri = "2.10"`、`tauri-build = "2.5"`、`tauri-plugin = "2.5"`：来源 [plugins-workspace v2 根 Cargo.toml](https://github.com/tauri-apps/plugins-workspace/blob/v2/Cargo.toml)。
- 插件安装可用 CLI：在 `src-tauri` 目录执行 `tauri add <plugin>`（官方 SQL 文档第 44 行原文“Run the following command in the `src-tauri` folder to add the plugin to the project's dependencies”），来源 [plugin/sql.mdx](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/sql.mdx)。

### 2.2 `tauri-plugin-sql`：能否在 Rust 侧直接用 sqlx？

**事实（源码级）**

1. 插件本身就是 sqlx 的封装：README 原文 “Interface with SQL databases through [sqlx](https://github.com/launchbadge/sqlx)” —— [plugins/sql/README.md](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/sql/README.md)。
2. 插件依赖 **sqlx 0.8**（`sqlx = { version = "0.8", features = ["json","time","uuid","rust_decimal"] }`），Cargo feature `sqlite = ["sqlx/sqlite", "sqlx/runtime-tokio"]` —— [plugins/sql/Cargo.toml](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/sql/Cargo.toml)。
3. 插件暴露的 `DbPool` 就是 sqlx 连接池的枚举包装：
   ```rust
   pub enum DbPool {
       #[cfg(feature = "sqlite")]  Sqlite(Pool<Sqlite>),
       #[cfg(feature = "mysql")]   MySql(Pool<MySql>),
       #[cfg(feature = "postgres")] Postgres(Pool<Postgres>),
       #[cfg(not(any(...)))]       None,
   }
   ```
   来源：[plugins/sql/src/wrapper.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/sql/src/wrapper.rs)。
4. 插件把连接池放在公开的 managed state 中：`pub struct DbInstances(pub RwLock<HashMap<String, DbPool>>);` —— [plugins/sql/src/lib.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/sql/src/lib.rs)。即 Rust 侧可通过 `app.state::<tauri_plugin_sql::DbInstances>()` 取得同一批池。
5. 数据库文件路径相对于 `tauri::api::path::BaseDirectory::AppConfig`：官方原文 “The path is relative to [`tauri::api::path::BaseDirectory::AppConfig`](https://docs.rs/tauri/2.0.0/tauri/path/enum.BaseDirectory.html#variant.AppConfig)” —— [plugin/sql.mdx](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/sql.mdx)（第 112 行）。Windows 上该目录为 `%APPDATA%\<bundle identifier>`（由 [tauri 2.11 `path/desktop.rs`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/path/desktop.rs) 的 `app_config_dir` 解析规则决定；该目录解析属于 tauri 内部实现，官方文档未用中文路径方式表述）。
6. 插件也支持 Rust 侧定义并应用迁移：`tauri_plugin_sql::{Migration, MigrationKind, Builder}` + `Builder::default().add_migrations("sqlite:mydatabase.db", migrations)`，并可在 `tauri.conf.json` 用 `plugins.sql.preload` 在启动时连接。

**推荐做法 —— 官方文档未明确给出“Rust 侧是否应绕过插件”的结论，此处区分“事实”与“工程建议”**

- 事实：**可以在 Rust 侧直接使用 sqlx 而不注册该插件**。插件不是 sqlx 的前置条件，sqlx 是普通 crate。
- 事实：插件带来的是 JS 侧 `Database.load()/execute()/select()` API、`preload`、迁移管理与连接池生命周期管理（官方文档 [SQL 插件页](https://v2.tauri.app/plugin/sql/)）。
- 工程建议（非官方结论）：
  - 若数据库**只**由 Rust 侧访问（Todo 应用的典型形态）：直接依赖 `sqlx`，用 `tauri::State` 持有 `SqlitePool`，迁移用 sqlx 自身机制；**不要**同时启用 `tauri-plugin-sql`，以避免同一 workspace 出现两个 sqlx 版本。
  - 版本风险点：`tauri-plugin-sql` 2.4.1 锁定 **sqlx 0.8**，而 crates.io 上 sqlx 最新稳定为 **0.9.0**（[crates.io/sqlx](https://crates.io/api/v1/crates/sqlx)，2026-05-21 更新）。若项目直接依赖 0.9 又引入插件，会同时链接两个 sqlx 大版本。**混用前请先确认能否统一为 0.8。**
  - 若前端也要直接读写数据库：使用插件，并只经插件持有连接池。

### 2.3 是否存在官方 keyring / 凭据存储插件？

**结论：截至 2026-09-23，2.x 稳定线上没有官方 keyring/凭据存储插件。**

| 事实 | 结论 | 来源 |
| --- | --- | --- |
| 官方插件目录（Autostart…Window State 共 29 项） | 无 keyring / credential / secure-storage 插件 | [v2.tauri.app/plugin/](https://v2.tauri.app/plugin/)、[plugins-workspace v2 分支 `plugins/*` 目录列表](https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins) |
| 官方加密存储插件 `tauri-plugin-stronghold` | 存在，2.3.2（口令加密保险库，不是 OS 凭据管理器） | [crates.io API](https://crates.io/api/v1/crates/tauri-plugin-stronghold)、[官方 Stronghold 文档](https://v2.tauri.app/plugin/stronghold/) |
| 官方仓库存在**未发布**分支 `plugin/secure-storage` | 该分支上有 `plugins/secure-storage`，`Cargo.toml version = "2.0.0"`，依赖 `keyring-core = "0.7"`，Windows 用 `windows-native-keyring-store = "0.2"` | [分支 Cargo.toml](https://github.com/tauri-apps/plugins-workspace/blob/plugin/secure-storage/plugins/secure-storage/Cargo.toml) |
| 该官方插件是否已发布 | **未发布**：npm 无 `@tauri-apps/plugin-secure-storage`；`v2` 分支不含该插件；crates.io 上同名 `tauri-plugin-secure-storage` 最新为 **1.5.0**，但所有者是第三方 `ThatzOkay`（非 tauri-apps） | npm registry 查询、[crates.io API](https://crates.io/api/v1/crates/tauri-plugin-secure-storage)、[crates.io owners](https://crates.io/api/v1/crates/tauri-plugin-secure-storage/owners) |

**社区首选（核实到版本）**

| crate | 最新稳定版 | Windows 后端 | 来源 |
| --- | --- | --- | --- |
| `keyring` | **4.2.0** | 通过可选依赖 `windows-native-keyring-store`（Windows 凭据管理器） | [crates.io API](https://crates.io/api/v1/crates/keyring)、[docs.rs keyring 4.2.0](https://docs.rs/keyring/4.2.0/keyring/) |
| `keyring-core` | **1.0.0** | 与具体 store 组合使用 | [crates.io API](https://crates.io/api/v1/crates/keyring-core) |
| `windows-native-keyring-store` | **1.1.0** | Windows Credential Manager 实现 | [crates.io API](https://crates.io/api/v1/crates/windows-native-keyring-store) |

`keyring` 4.2.0 官方文档给出的选型说明（原文要点）：该 crate 提供 `v1`（跨平台简单读写）与 `cli` 两种模式；**需要自行控制“在哪个平台用哪个凭据存储”或需要更多功能的应用，不应链接本 crate，而应直接链接 `keyring-core` 及所需的 store crate**。来源：[docs.rs keyring 4.2.0 crate 文档](https://docs.rs/keyring/4.2.0/keyring/)。

---

## 3. 窗口能力 API（Window / WebviewWindow）

`Window` 与 `WebviewWindow` **具有同名同签名的方法**（`WebviewWindow` 只是转发到内部 `Window`）。以下签名逐条取自 2.11 分支源码：
[`crates/tauri/src/window/mod.rs`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/window/mod.rs) 与 [`crates/tauri/src/webview/webview_window.rs`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/webview/webview_window.rs)。

### 3.1 Rust 侧确切签名

```rust
// 置顶 / 置底
pub fn set_always_on_top(&self, always_on_top: bool) -> crate::Result<()>
pub fn set_always_on_bottom(&self, always_on_bottom: bool) -> crate::Result<()>
pub fn is_always_on_top(&self) -> crate::Result<bool>

// 鼠标穿透
pub fn set_ignore_cursor_events(&self, ignore: bool) -> crate::Result<()>

// 边框 / 阴影 / 效果
pub fn set_decorations(&self, decorations: bool) -> crate::Result<()>
pub fn is_decorated(&self) -> crate::Result<bool>
pub fn set_shadow(&self, enable: bool) -> crate::Result<()>
pub fn set_effects<E: Into<Option<WindowEffectsConfig>>>(&self, effects: E) -> crate::Result<()>

// 任务栏显隐
pub fn set_skip_taskbar(&self, skip: bool) -> crate::Result<()>

// 位置与多显示器
pub fn set_position<Pos: Into<Position>>(&self, position: Pos) -> crate::Result<()>
pub fn available_monitors(&self) -> crate::Result<Vec<Monitor>>
pub fn primary_monitor(&self) -> crate::Result<Option<Monitor>>
pub fn current_monitor(&self) -> crate::Result<Option<Monitor>>
pub fn monitor_from_point(&self, x: f64, y: f64) -> crate::Result<Option<Monitor>>
pub fn set_visible_on_all_workspaces(&self, visible_on_all_workspaces: bool) -> crate::Result<()>

// 其他常用
pub fn center(&self) -> crate::Result<()>
pub fn set_size<S: Into<Size>>(&self, size: S) -> crate::Result<()>
pub fn set_min_size<S: Into<Size>>(&self, size: Option<S>) -> crate::Result<()>
pub fn set_max_size<S: Into<Size>>(&self, size: Option<S>) -> crate::Result<()>
pub fn set_focus(&self) -> crate::Result<()>
pub fn show(&self) -> crate::Result<()>
pub fn hide(&self) -> crate::Result<()>
```

`Monitor` 结构与访问器（同源码）：

```rust
pub struct Monitor {
    pub(crate) name: Option<String>,
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) position: PhysicalPosition<i32>,
    pub(crate) work_area: PhysicalRect<i32, u32>,
    pub(crate) scale_factor: f64,
}
// 访问器：name() / size() / position() / work_area() / scale_factor()
```

docs.rs 对照页：[`tauri::WebviewWindow`](https://docs.rs/tauri/2.11.6/tauri/webview/struct.WebviewWindow.html)、[`tauri::Window`](https://docs.rs/tauri/2.11.6/tauri/window/struct.Window.html)。

**平台差异（源码文档注释原文）**
- `set_skip_taskbar`：`- **macOS:** Unsupported.`（Windows/Linux 支持）
- `set_ignore_cursor_events`：源码中无平台特定注释（仅 “Ignores the window cursor events.”）
- `set_effects`：`**Windows**: If using decorations or shadows, you may want to try this workaround <https://github.com/tauri-apps/tao/issues/72#issuecomment-975607891>`；`**Linux**: Unsupported`
- `set_cursor_visible`：Windows 上光标仅在窗口范围内隐藏

### 3.2 JS 侧确切签名

来源：[`packages/api/src/window.ts`（2.11 分支）](https://github.com/tauri-apps/tauri/blob/2.11/packages/api/src/window.ts)、[JS API 参考](https://v2.tauri.app/reference/javascript/api/namespacewindow/)。

```ts
// Window 类实例方法
async setAlwaysOnTop(alwaysOnTop: boolean): Promise<void>
async setAlwaysOnBottom(alwaysOnBottom: boolean): Promise<void>
async setIgnoreCursorEvents(ignore: boolean): Promise<void>
async setDecorations(decorations: boolean): Promise<void>
async setShadow(enable: boolean): Promise<void>
async setSkipTaskbar(skip: boolean): Promise<void>
async setPosition(position: LogicalPosition | PhysicalPosition | Position): Promise<void>
async setSize(size: LogicalSize | PhysicalSize | Size): Promise<void>
async setVisibleOnAllWorkspaces(visible: boolean): Promise<void>
async setEffects(effects: Effects): Promise<void>

// 模块级函数（不挂在实例上）
async function primaryMonitor(): Promise<Monitor | null>
async function availableMonitors(): Promise<Monitor[]>
// currentMonitor() / monitorFromPoint() 同类导出

export interface Monitor {
  name: string | null
  size: PhysicalSize
  position: PhysicalPosition
  scaleFactor: number
}
```

用 JS 调用前必须在 capability 中放行相应权限，例如 `core:window:allow-set-always-on-top`、`core:window:allow-set-ignore-cursor-events` 等（权限清单见 [Window Customization 文档](https://v2.tauri.app/learn/window-customization/) 中 `core:window:*` 表；具体每个命令对应的权限名以 `src-tauri/gen/schemas/` 生成的 ACL schema 为准 —— 本报告未逐一核实全部命令的权限标识符，**未核实**）。

### 3.3 透明窗口（`transparent: true`）

**配置项**
```jsonc
// tauri.conf.json
{ "app": { "windows": [ { "transparent": true, "decorations": false } ] } }
```
字段定义与注释原文（`WindowConfig`）：

> `/// Whether the window is transparent or not.`
> `/// Note that on \`macOS\` this requires the \`macos-private-api\` feature flag, enabled under \`tauri > macOSPrivateApi\`.`
> `/// WARNING: Using private APIs on \`macOS\` prevents your application from being accepted to the \`App Store\`.`

来源：[`crates/tauri-utils/src/config.rs`（2.11 分支，第 2020-2025 行）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-utils/src/config.rs)、[配置参考](https://v2.tauri.app/reference/config/)。

**构建器方法**
```rust
// WebviewWindowBuilder
#[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]
pub fn transparent(mut self, transparent: bool) -> Self
```
来源：[`webview_window.rs`（2.11 分支，第 1073-1088 行）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/webview/webview_window.rs)。

**结论**
- **Windows 上透明窗口不需要额外 Cargo feature**：`transparent()` 仅在 macOS 上受 `macos-private-api` 条件编译限制（`cfg(any(not(target_os = "macos"), feature = "macos-private-api"))`）。`macos-private-api` 是 macOS 专属，与 Windows 无关。
- 需要 `transparent: true` 才能让 `windowEffects` 生效：源码注释 “Window effects. **Requires the window to be transparent.**”
- 若用 `windowEffects`，Windows 上还需注意 decorations/shadows 的 workaround（见 §3.1）。

**Windows 上的已知限制 —— 官方文档层面：未核实**
全量检索 tauri-docs `v2` 分支 162 个英文文档后，`transparent` 仅出现在 window-customization 教程（macOS 场景）、barcode-scanner 插件与 changelog 中，**没有任何 Windows 专属限制章节**。检索方式：下载 tauri-docs `v2` 分支 tarball 后对 `src/content/docs/**.mdx` 全文匹配 `transparent`。
唯一相关 changelog 记录为历史 bug 修复：“On Windows, fix decorated window not transparent initially until resized.”，来源 [tauri-2.0 博客](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/blog/tauri-2.0.mdx)。

**仓库 issue 中的已知 Windows 透明窗口问题（注意：是 issue，不是官方文档承诺，仅供风险评估）**
- [#15947](https://github.com/tauri-apps/tauri/issues/15947)（open）Windows 10：透明置顶窗口在切换鼠标穿透/最小化时偶发渲染异常
- [#15490](https://github.com/tauri-apps/tauri/issues/15490)（closed）`decorations=false` + `transparent=true` 时每次 `show()` 有白闪
- [#14764](https://github.com/tauri-apps/tauri/issues/14764)（closed）透明窗口拖拽/失焦时出现幽灵标题栏背景
- [#10318](https://github.com/tauri-apps/tauri/issues/10318)（closed）Windows 11 `transparent: true` 初始尺寸区域白底
- [#14636](https://github.com/tauri-apps/tauri/issues/14636)（closed）Windows 11 透明窗口阴影是否可去除

**`windows_subsystem` 与透明窗口无关（核实）**
`windows_subsystem = "windows"` 只用于 release 构建时隐藏控制台窗口，与透明无关。官方项目模板：

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```
来源：[模板 `src-tauri/src/main.rs`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-cli/templates/app/src-tauri/src/main.rs)（同片段亦出现在官方文档 [Window Menu](https://v2.tauri.app/learn/window-menu/) 示例中）。

### 3.4 任务栏显隐与多显示器的落地要点

- 隐藏任务栏图标：`window.set_skip_taskbar(true)`（Rust）/ `appWindow.setSkipTaskbar(true)`（JS）；macOS 不支持。
- 多显示器：`available_monitors()` 返回全部显示器（含 `position`/`size`/`scale_factor`/`work_area`），`set_position()` 接受实现了 `Into<Position>` 的值，因此 `PhysicalPosition::new(x, y)` 与 `LogicalPosition::new(x, y)` 均可直接传入；JS 侧对应 `availableMonitors()` 与 `setPosition()`。
- **注意 DPI**：`PhysicalPosition` 与 `LogicalPosition` 混用会在高 DPI 显示器上产生偏移；`Monitor.scale_factor` 用于换算（JS 侧 `Monitor` 注释原文：“Use `Monitor.scaleFactor` to convert to logical pixels”）。

---

## 4. 系统托盘（TrayIcon / TrayIconBuilder）

官方文档：[System Tray](https://v2.tauri.app/learn/system-tray/)（访问 2026-09-23）。

### 4.1 必需 Cargo feature

```toml
tauri = { version = "2.0.0", features = [ "tray-icon" ] }
```
feature 名：**`tray-icon`**（官方文档原文；`crates/tauri/Cargo.toml` 中 `tray-icon = ["dep:tray-icon"]`）。来源：[System Tray 文档](https://v2.tauri.app/learn/system-tray/)、[`crates/tauri/Cargo.toml`（2.11 分支，第 205 行）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/Cargo.toml)。

### 4.2 构建与运行时方法（Rust，2.11 分支源码）

```rust
// TrayIconBuilder —— crates/tauri/src/tray/mod.rs
pub fn new() -> Self
pub fn with_id<I: Into<TrayIconId>>(id: I) -> Self
pub fn menu<M: ContextMenu>(mut self, menu: &M) -> Self
pub fn icon(mut self, icon: Image<'_>) -> Self
pub fn tooltip<S: AsRef<str>>(mut self, s: S) -> Self
pub fn title<S: AsRef<str>>(mut self, title: S) -> Self
pub fn show_menu_on_left_click(mut self, enable: bool) -> Self
pub fn on_menu_event<F: Fn(&AppHandle<R>, MenuEvent) + Sync + Send + 'static>(mut self, f: F) -> Self
pub fn on_tray_icon_event<F: Fn(&TrayIcon<R>, TrayIconEvent) + Sync + Send + 'static>(mut self, f: F) -> Self
pub fn build<M: Manager<R>>(self, manager: &M) -> crate::Result<TrayIcon<R>>

// TrayIcon（运行时更新）—— 同一文件
pub fn id(&self) -> &TrayIconId
pub fn set_icon(&self, icon: Option<Image<'_>>) -> crate::Result<()>
pub fn set_menu<M: ContextMenu + 'static>(&self, menu: Option<M>) -> crate::Result<()>
pub fn set_tooltip<S: AsRef<str>>(&self, tooltip: Option<S>) -> crate::Result<()>
pub fn set_title<S: AsRef<str>>(&self, title: Option<S>) -> crate::Result<()>
pub fn set_visible(&self, visible: bool) -> crate::Result<()>
pub fn set_show_menu_on_left_click(&self, enable: bool) -> crate::Result<()>
```
来源：[`crates/tauri/src/tray/mod.rs`（2.11 分支）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/tray/mod.rs)、docs.rs [TrayIconBuilder](https://docs.rs/tauri/2.11.6/tauri/tray/struct.TrayIconBuilder.html) / [TrayIcon](https://docs.rs/tauri/2.11.6/tauri/tray/struct.TrayIcon.html)。

### 4.3 菜单项、动态更新文本、左键点击

```rust
use tauri::{
  menu::{Menu, MenuItem},
  tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
let menu = Menu::with_items(app, &[&quit_i])?;

let tray = TrayIconBuilder::new()
  .menu(&menu)
  .show_menu_on_left_click(true)
  .on_menu_event(|app, event| match event.id.as_ref() {
    "quit" => { app.exit(0); }
    _ => {}
  })
  .on_tray_icon_event(|tray, event| match event {
    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => {
      let app = tray.app_handle();
      if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
      }
    }
    _ => {}
  })
  .build(app)?;
```
来源：[System Tray 文档](https://v2.tauri.app/learn/system-tray/)（原文示例）。

**动态更新菜单文本**：`MenuItem` 提供
```rust
pub fn set_text<S: AsRef<str>>(&self, text: S) -> crate::Result<()>
pub fn text(&self) -> crate::Result<String>
pub fn set_enabled(&self, enabled: bool) -> crate::Result<()>
pub fn set_accelerator<S: AsRef<str>>(&self, accelerator: Option<S>) -> crate::Result<()>
```
来源：[`crates/tauri/src/menu/normal.rs`（2.11 分支）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/menu/normal.rs)、docs.rs [MenuItem](https://docs.rs/tauri/2.11.6/tauri/menu/struct.MenuItem.html)。
另有整菜单替换：`TrayIcon::set_menu()`；菜单结构变更可用 `Menu::append/prepend/insert/remove`（[menu.rs](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri/src/menu/menu.rs)）。

**左键点击语义（官方文档原文）**：
> By default the menu is displayed on both left and right clicks.
> To prevent the menu from popping up on left click, call the `show_menu_on_left_click(false)` Rust function or set the `menuOnLeftClick` JavaScript option to `false`.

托盘事件类型（官方文档原文）：Click / DoubleClick / Enter / Move / Leave；**Linux 不发出这些事件**。

**JS 侧**（[`packages/api/src/tray.ts`](https://github.com/tauri-apps/tauri/blob/2.11/packages/api/src/tray.ts)）：
```ts
static async new(options?: TrayIconOptions): Promise<TrayIcon>
static async getById(id: string): Promise<TrayIcon | null>
static async removeById(id: string): Promise<void>
async setIcon(...): Promise<void>
async setMenu(menu: Menu | Submenu | null): Promise<void>
async setTooltip(tooltip: string | null): Promise<void>
async setTitle(title: string | null): Promise<void>
async setVisible(visible: boolean): Promise<void>

interface TrayIconOptions {
  menu?: Menu | Submenu
  icon?: string | Uint8Array | ArrayBuffer | number[] | Image
  showMenuOnLeftClick?: boolean   // 旧的 menuOnLeftClick 已标记 @deprecated
  action?: (event: TrayIconEvent) => void
}
```

---

## 5. Tauri 2 打包（Windows：NSIS 与 MSI）

官方文档：[Windows Installer](https://v2.tauri.app/distribute/windows-installer/)（访问 2026-09-23）。

### 5.1 `bundle.targets` 取值

```jsonc
// tauri.conf.json
{
  "bundle": {
    "active": true,
    "targets": ["nsis", "msi"]   // 或 "all"，或单个字符串 "nsis"
  }
}
```
`BundleType` 全部取值（源码枚举）：`deb`、`rpm`、`appimage`、`msi`、`nsis`、`app`、`dmg`；`BundleTarget` 接受 `"all"`、单个值或值数组。
来源：[`crates/tauri-utils/src/config.rs`（2.11 分支，第 128-177 行 BundleType、第 210-313 行 BundleTarget）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-utils/src/config.rs)、[配置参考 bundle.targets](https://v2.tauri.app/reference/config/)。

### 5.2 CLI 命令

```
npm run tauri build          # 官方文档给出的命令
cargo tauri build            # cargo 安装方式
tauri build -b nsis          # -b/--bundles：空格或逗号分隔的目标列表
tauri build --bundles nsis,msi
tauri build --no-bundle      # 跳过打包
```
CLI 参数定义（2.11 分支源码）：
```rust
/// Space or comma separated list of bundles to package.
#[clap(short, long, action = ArgAction::Append, num_args(0..), value_delimiter = ',')]
pub bundles: Option<Vec<BundleFormat>>,
/// Skip the bundling step even if `bundle > active` is `true` in tauri config.
#[clap(long)]
pub no_bundle: bool,
```
来源：[`crates/tauri-cli/src/build.rs`（2.11 分支）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-cli/src/build.rs)、[Windows Installer 文档](https://v2.tauri.app/distribute/windows-installer/)。

### 5.3 是否需要额外安装 WiX / NSIS

**结论：不需要手动安装。Tauri 会在首次打包时自动下载工具链到本地缓存。**

| 工具 | 版本与来源 | 证据 |
| --- | --- | --- |
| NSIS | `https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip` | [`nsis/mod.rs` 常量 `NSIS_URL`、`NSIS_SHA1`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs) |
| NSIS 附加插件 | `nsis_tauri_utils-v0.5.3`（`nsis_tauri_utils.dll`） | 同上 |
| WiX Toolset | `wix3141rtm`（WiX v3.14.1，`wix314-binaries.zip`），解包到缓存目录 `WixTools314` | [`msi/mod.rs` 常量 `WIX_URL` / `WIX_SHA256`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/msi/mod.rs) |

Windows 上 NSIS 调用的是**下载目录里的 `makensis.exe`**（`nsis_toolset_path.join("makensis.exe")`），非 Windows 主机才用 PATH 中的 `makensis`；即 Windows 本机构建无需自行安装 NSIS。
**例外前提**：`.msi` **只能在 Windows 上构建**（官方原文：“`.msi` installers can **only be created on Windows** as WiX can only run on Windows systems”），且构建 MSI 需要 **VBSCRIPT 可选功能**（见 §7）。

### 5.4 生成安装包的默认输出路径

源码中的输出路径构造：

```rust
// NSIS
let package_base_name = format!("{}_{}_{}-setup", settings.product_name(), settings.version_string(), arch);
let nsis_installer_path = settings.project_out_directory()
    .join(format!("bundle/{}/{}.exe", /* "nsis" 或 "nsis-updater" */, package_base_name));

// MSI
"bundle/{}/{}.msi"
```
来源：[`nsis/mod.rs` 第 652-668 行](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs)、[`msi/mod.rs` 第 238 行](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/msi/mod.rs)。

因此默认（`project_out_directory` = `<src-tauri>/target`）：

| 目标 | 默认路径 | 示例 |
| --- | --- | --- |
| NSIS | `src-tauri/target/release/bundle/nsis/<productName>_<version>_<arch>-setup.exe` | `Todo_0.1.0_x64-setup.exe` |
| MSI | `src-tauri/target/release/bundle/msi/<...>.msi` | `Todo_0.1.0_x64_en-US.msi` |
| 可执行文件 | `src-tauri/target/release/<binary-name>.exe` | — |

（交叉编译时官方文档给出的路径形如 `target/x86_64-pc-windows-msvc/release/bundle/nsis/`，见 [Windows Installer 文档第 145 行](https://v2.tauri.app/distribute/windows-installer/)。）

### 5.5 NSIS 安装包：卸载时保留 / 删除用户数据

**默认行为：保留。** `installer.nsi` 在卸载确认页动态添加一个 “Delete app data” 复选框，**初始未勾选**（只 `CreateWindowEx`，无 `BM_SETCHECK`）：

```nsis
Function un.ConfirmShow ; Add add a `Delete app data` check box
  System::Call 'user32::CreateWindowEx(... w "$(deleteAppData)" ...) i .s'
  Pop $DeleteAppDataCheckbox
FunctionEnd
Function un.ConfirmLeave
  SendMessage $DeleteAppDataCheckbox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
FunctionEnd
```

勾选后才删除数据（且**更新模式下永不删除**）：

```nsis
; Delete app data if the checkbox is selected and if not updating
${If} $DeleteAppDataCheckboxState = 1
${AndIf} $UpdateMode <> 1
  SetShellVarContext current
  RmDir /r "$APPDATA\${BUNDLEID}"
  RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
${EndIf}
```
来源：[`nsis/installer.nsi`（2.11 分支，第 423-467 行与第 868-884 行）](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)。

要点：
1. 数据目录以 **bundle identifier**（`${BUNDLEID}`，即 `tauri.conf.json` 的 `identifier`）为名，位于 `%APPDATA%\<identifier>` 与 `%LOCALAPPDATA%\<identifier>`；与 `productName` 无关。
2. 卸载时也会删除 HKCU `...\Run` 中同名的自启动项（“Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists”）。
3. **自定义保留/删除策略的唯一官方钩子**：`bundle.windows.nsis.installerHooks` 指向的 `.nsh` 文件，支持四个宏：

```nsh
!macro NSIS_HOOK_PREINSTALL
!macroend
!macro NSIS_HOOK_POSTINSTALL
!macroend
!macro NSIS_HOOK_PREUNINSTALL
!macroend
!macro NSIS_HOOK_POSTUNINSTALL
!macroend
```
来源：[`NsisConfig::installer_hooks` 文档注释](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-utils/src/config.rs)、[Windows Installer 文档「Customizing the NSIS Installer」](https://v2.tauri.app/distribute/windows-installer/)。（`NSIS_HOOK_POSTUNINSTALL` 正是“无论是否勾选复选框都要保留数据 / 额外清理”的挂载点。）

**NSIS 可配置项全量（`bundle.windows.nsis`，源码字段）**：`template`、`headerImage`、`sidebarImage`、`installerIcon`、`uninstallerIcon`、`uninstallerHeaderImage`、`installMode`（`currentUser` / `perMachine` / `both`）、`languages`、`customLanguageFiles`、`displayLanguageSelector`、`compression`、`startMenuFolder`、`installerHooks`。来源：[`NsisConfig`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-utils/src/config.rs)。
（`minimumWebview2Version` 已自 2.10.0 起废弃，改用 `bundle.windows.minimumWebview2Version` —— 源码 `#[deprecated(since = "2.10.0")]`。）

---

## 6. Windows 通知

官方文档：[Notifications](https://v2.tauri.app/plugin/notification/)（访问 2026-09-23）。

### 6.1 实现机制（核实到依赖链）

```
tauri-plugin-notification 2.4.0
  └─ notify-rust = "4.11"                      （桌面平台）
       └─ [target.'cfg(target_os="windows")'] winrt-notification = { package = "tauri-winrt-notification", version = "0.8" }
```
- 结论：**Windows 上确实走 WinRT Toast API**，最终实现是 `tauri-winrt-notification` 0.8.x（由 notify-rust 引入）。
- 来源：[`plugins/notification/Cargo.toml`（v2 分支）](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/Cargo.toml)、[notify-rust `Cargo.toml`](https://github.com/hoodie/notify-rust/blob/main/Cargo.toml)。
- Windows 7 不走 Toast，需开启 `windows7-compat` feature（依赖 `win7-notifications 0.4.5` + `windows-version 0.1`）。
- 桌面端可选字段（源码注释原文）：“Only the title, body, icon and sound of the notification are used on desktop; the scheduling, grouping and action related options are ignored.” 来源：[`plugins/notification/src/desktop.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/src/desktop.rs)。

### 6.2 通知点击回调是否支持

**结论：官方文档明确标注通知 Actions API 仅移动端可用；桌面端（含 Windows）不支持点击回调。**

- 官方原文（Notifications 文档 “Actions” 一节）：
  > **:::caution[Mobile Only]**
  > The Actions API is only available on mobile platforms.
- `onAction()` / `registerActionTypes()` 属该 Actions API，桌面端无对应桌面实现（`desktop.rs` 中无动作/回调相关代码路径）。
- 来源：[plugin/notification.mdx Actions](https://v2.tauri.app/plugin/notification/)、[`guest-js/index.ts`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/guest-js/index.ts)、[`src/desktop.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/notification/src/desktop.rs)。
- 「点击通知把应用带到前台」在 Windows 桌面端是否有官方支持：**未核实**（官方文档未记载）。

### 6.3 AUMID / 快捷方式要求（关键结论）

插件的平台说明（`Cargo.toml` 元数据，原文）：
> `windows = { level = "full", notes = "Only works for installed apps. Shows powershell name & icon in development." }`

代码层面：
```rust
#[cfg(windows)]
{
    let exe = tauri::utils::platform::current_exe()?;
    let curr_dir = exe.parent()...display().to_string();
    // set the notification's System.AppUserModel.ID only when running the installed app
    if !(curr_dir.ends_with(format!("{SEP}target{SEP}debug").as_str())
      || curr_dir.ends_with(format!("{SEP}target{SEP}release").as_str())) {
        notification.app_id(&self.identifier);
    }
}
```
即：**AUMID 取 `tauri.conf.json` 的 `identifier`**；开发模式（`target/debug`、`target/release`）下不设置 AUMID，所以通知显示为 PowerShell 的名称与图标。

安装器侧会把这个 AUMID 写进快捷方式：
- NSIS：`!insertmacro SetLnkAppUserModelId "$SMPROGRAMS\...\${PRODUCTNAME}.lnk"`，该宏通过 `IPropertyStore` 写入 `PKEY_AppUserModel_ID = ${BUNDLEID}`（[`utils.nsh`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh)）。
- MSI：`<ShortcutProperty Key="System.AppUserModel.ID" Value="{{bundle_id}}"/>`（[`main.wxs`](https://github.com/tauri-apps/tauri/blob/2.11/crates/tauri-bundler/src/bundle/windows/msi/main.wxs)）。

**实践含义**：Windows 上要显示正确的应用名称与图标，应用必须通过安装包安装（存在带 AUMID 的快捷方式），开发模式下显示 PowerShell 属预期行为；`identifier` 一旦变更，已安装版本的 AUMID 也随之变化。

### 6.4 官方插件能力不足时的替代方案（当前版本）

| crate | 最新版本 | 说明 | 来源 |
| --- | --- | --- | --- |
| `tauri-winrt-notification` | **0.8.1**（2026-07-17 更新） | 官方 tauri-apps 仓库维护；自述 “An incomplete wrapper over the WinRT toast api”；用法 `Toast::new(<AUMID>).title(..).text1(..).show()`；Windows 7 不支持；README 声明 “Add support for Adaptive Content” 仍在 TODO | [crates.io API](https://crates.io/api/v1/crates/tauri-winrt-notification)、[仓库 README](https://github.com/tauri-apps/winrt-notification) |
| `winrt-notification`（旧同名 crate） | 0.5.1（**2022-01-11 后未更新**） | 原作者归档式项目，不建议新项目使用 | [crates.io API](https://crates.io/api/v1/crates/winrt-notification) |
| `notify-rust` | 4.18.0（2026-06-16） | 插件实际使用的库；Windows 后端即 `tauri-winrt-notification` 0.8 | [crates.io API](https://crates.io/api/v1/crates/notify-rust) |

即：**若需自定义 Toast（按钮、图片、点击后回调），可直接依赖 `tauri-winrt-notification` 0.8.1**（与插件在同一条依赖链上，不会引入第二套 Toast 实现），并自行传入 AUMID（`Toast::new(<identifier>)`）。

---

## 7. 中文 / 中文路径与 Rust 工具链：Windows 11 + MSVC 构建前置要求

### 7.1 官方前置要求清单（原文归纳）

来源：[Prerequisites](https://v2.tauri.app/start/prerequisites/)（访问 2026-09-23），Windows 一节原文：

1. **Microsoft C++ Build Tools** —— 安装时勾选 “Desktop development with C++”（官方原文：“Tauri uses the Microsoft C++ Build Tools for development as well as Microsoft Edge WebView2. These are both required for development on Windows.”）。
2. **Microsoft Edge WebView2 Runtime** —— Windows 10 1803 及更高（含 Windows 11）已预装，可跳过；否则安装 “Evergreen Bootstrapper”。
3. **VBSCRIPT 可选功能** —— **仅构建 MSI 时需要**（`"targets": "msi"` 或 `"all"`）。开启路径：**Settings → Apps → Optional features → More Windows features**，勾选 **VBSCRIPT**。若报 `failed to run light.exe` 即为此问题。官方同时标注 VBSCRIPT 正在被弃用（附微软弃用公告链接）。
4. **Rust（rustup）** —— Windows 可用 `winget install --id Rustlang.Rustup`；**必须确保 MSVC 工具链为默认 host triple**：

   > `:::caution[MSVC toolchain as default]`
   > For full support for Tauri and tools like `trunk` make sure the MSVC Rust toolchain is the selected `default host triple` in the installer dialog. Depending on your system it should be either `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, or `aarch64-pc-windows-msvc`.
   > If you already have Rust installed, you can make sure the correct toolchain is installed by running this command:
   > ```powershell
   > rustup default stable-msvc
   > ```
5. **Node.js LTS**（仅在使用 JS 前端时需要；官方示例 `node -v → v20.10.0`、`npm -v → 10.2.3`；可选 `corepack enable` 以启用 pnpm/yarn）。
6. 安装后**重启终端**（必要时重启系统）。
7. 官方仅在 Windows 7 及以上范围内声明支持；**官方只支持 MSVC 目标**（原文：“Since Tauri officially only supports the MSVC Windows target, the setup is a bit more involved.” —— [Windows Installer](https://v2.tauri.app/distribute/windows-installer/)）。

**MSRV**：Tauri 2.x 与全部官方插件要求 **Rust ≥ 1.77.2**（见 §0 来源）。

### 7.2 中文 / 非 ASCII 路径

**结论：官方文档未涉及中文安装路径或中文项目路径，属「未核实」。**

- 检索结果：全量检索 tauri-docs `v2` 分支 162 个英文文档，`prerequisites.mdx` 中**没有**任何关于非 ASCII 路径、中文用户名（`C:\Users\中文名\...`）或路径长度的说明。
- 检索结果：在 tauri-apps/tauri 仓库用 GitHub Issue 搜索 `non-ascii path`、`chinese path`、`unicode path`、`"non-UTF-8" path`、`path spaces build windows`（均限标题）**均返回 0 条结果**。
- 唯一相关的官方记录是历史修复条目：“Fix building apps with unicode characters in their `productName`. [#5872](https://github.com/tauri-apps/tauri/pull/5872)”，出自 [Tauri 1.3 changelog](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/blog/tauri-1-3.mdx)（v1 时代，针对 `productName` 而非构建路径）。
- **工程建议（非官方）**：把项目放在纯 ASCII 路径（例如 `D:\dev\todo-app`）可规避一类未记录的潜在问题；本报告无法给出官方保证，亦不宣称中文路径必然失败。**未核实**：中文 `productName`、中文 `identifier`、中文用户名下的 `C:\Users\<中文>` 是否影响 NSIS/WiX 构建与通知 AUMID。

---

## 附录 A：官方 URL 索引（全部访问于 2026-09-23）

| 主题 | URL |
| --- | --- |
| 前置要求 | https://v2.tauri.app/start/prerequisites/ |
| 窗口定制 | https://v2.tauri.app/learn/window-customization/ |
| 系统托盘 | https://v2.tauri.app/learn/system-tray/ |
| 窗口菜单 | https://v2.tauri.app/learn/window-menu/ |
| 配置参考 | https://v2.tauri.app/reference/config/ |
| JS Window API | https://v2.tauri.app/reference/javascript/api/namespacewindow/ |
| JS Tray API | https://v2.tauri.app/reference/javascript/api/namespacetray/ |
| Windows 安装包 | https://v2.tauri.app/distribute/windows-installer/ |
| 插件总览 | https://v2.tauri.app/plugin/ |
| SQL 插件 | https://v2.tauri.app/plugin/sql/ |
| 通知插件 | https://v2.tauri.app/plugin/notification/ |
| Rust API 文档 | https://docs.rs/tauri/2.11.6/tauri/ |
| 源码（tauri） | https://github.com/tauri-apps/tauri/tree/2.11 |
| 源码（插件） | https://github.com/tauri-apps/plugins-workspace/tree/v2 |
| 文档源码 | https://github.com/tauri-apps/tauri-docs/tree/v2 |

## 附录 B：明确标注「未核实」的条目

1. Tauri 2 Windows 透明窗口的官方专属限制清单 —— **官方文档无记载**（仅有仓库 issue 记录，见 §3.3）。
2. `set_ignore_cursor_events` 在 Windows 上的已知渲染问题 —— 官方文档无记载（issue #15947 涉及该组合，但为 issue 而非文档）。
3. Rust 侧使用 sqlx 而非 `tauri-plugin-sql` 是否被官方「推荐」 —— **官方文档未给出推荐结论**；本报告仅陈述源码事实（§2.2）。
4. Windows 桌面端通知「点击后激活应用」的官方支持情况 —— **未核实**（官方仅说明 Actions API 为 Mobile Only）。
5. 全部 `core:window:*` 命令的权限标识符对照表 —— **未逐一核实**（以项目生成的 ACL schema 为准）。
6. 中文路径 / 中文用户名 / 中文 `productName` 对构建的影响 —— **未核实**（官方文档无记载，仓库标题检索 0 结果）。
7. `@tauri-apps/plugin-stronghold` 的 npm 版本号 —— **未逐一核实**（crate 版本 2.3.2 已核实）。
8. WiX 生成 `.msi` 的完整默认文件名模板（语言段 `en-US` 等）—— 未逐一核实；仅核实到输出目录与 `bundle/{}/{}.msi` 构造方式。
