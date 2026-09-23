# Rust 依赖调研（Tauri 2 + SQLite / Windows 桌面 Todo）

> **访问日期**：2026-09-21（全表统一标注，按任务约定）
> **方法**：版本号、发布日期、许可证一律取自 `crates.io` API（机器可读一手来源，`/api/v1/crates/<name>` 与 `/api/v1/crates/<name>/<version>`，字段 `max_stable_version` / `created_at` / `license`）；行为与 API 结论一律取自 `docs.rs` 渲染后的官方 rustdoc 页面或官方 GitHub 仓库原始文件（README / CHANGELOG）。仓库活跃度取自 GitHub REST API（`pushed_at` / `archived`）。凡官方来源未明确记载者，标注 **未核实**，不作推断。
> **文件命名说明**：本文件**未**写入 `docs/api-research.md`，因为该路径已被《AI 适配层 API 事实核查（DeepSeek / OpenAI / Anthropic）》占用；`docs/api-research-deps.md` 亦已被前端 npm 依赖调研占用。本文件为同级第三份，如需合并请由人工决定。
> **工具说明**：本次调研未使用内置 `web_search`，全部走 `Invoke-RestMethod` / `Invoke-WebRequest` 直连官方端点。

---

## 0. 版本总表

| crate | 确切版本 | 发布日期 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `sqlx` | **0.9.0** | 2026-05-21 | MIT OR Apache-2.0 | <https://crates.io/crates/sqlx> · <https://docs.rs/sqlx/0.9.0/> | 2026-09-21 |
| `serde` | **1.0.229** | 2026-07-18 | MIT OR Apache-2.0 | <https://crates.io/crates/serde> | 2026-09-21 |
| `serde_json` | **1.0.151** | 2026-07-20 | MIT OR Apache-2.0 | <https://crates.io/crates/serde_json> | 2026-09-21 |
| `chrono` | **0.4.45** | 2026-06-04 | MIT OR Apache-2.0 | <https://crates.io/crates/chrono> | 2026-09-21 |
| `chrono-tz` | **0.10.4** | 2025-07-11 | MIT OR Apache-2.0 | <https://crates.io/crates/chrono-tz> | 2026-09-21 |
| `rrule` | **0.14.0** | 2025-04-20 | MIT OR Apache-2.0 | <https://crates.io/crates/rrule> · <https://github.com/fmeringdal/rust-rrule> | 2026-09-21 |
| `uuid` | **1.26.1** | 2026-09-10 | Apache-2.0 OR MIT | <https://crates.io/crates/uuid> | 2026-09-21 |
| `tokio` | **1.53.1** | 2026-07-20 | **MIT**（单许可，非双许可） | <https://crates.io/crates/tokio> | 2026-09-21 |
| `keyring` | **4.2.0** | 2026-08-29 | MIT OR Apache-2.0 | <https://crates.io/crates/keyring> · <https://docs.rs/keyring/4.2.0/> | 2026-09-21 |
| `keyring-core` | **1.0.0** | 2026-04-21 | MIT OR Apache-2.0 | <https://crates.io/crates/keyring-core> | 2026-09-21 |
| `windows-native-keyring-store` | **1.1.0** | 2026-05-24 | MIT OR Apache-2.0 | <https://crates.io/crates/windows-native-keyring-store> | 2026-09-21 |
| `tauri-plugin-keyring` | **0.1.0** | 2024-12-23 | MIT | <https://crates.io/crates/tauri-plugin-keyring> | 2026-09-21 |
| `reqwest` | **0.13.5** | 2026-09-08 | MIT OR Apache-2.0 | <https://crates.io/crates/reqwest> · <https://docs.rs/reqwest/0.13.5/> | 2026-09-21 |
| `thiserror` | **2.0.20** | 2026-08-08 | MIT OR Apache-2.0 | <https://crates.io/crates/thiserror> | 2026-09-21 |
| `anyhow` | **1.0.104** | 2026-07-18 | MIT OR Apache-2.0 | <https://crates.io/crates/anyhow> | 2026-09-21 |
| `csv` | **1.4.0** | 2025-10-17 | **Unlicense/MIT**（crates.io 原样写法） | <https://crates.io/crates/csv> · <https://github.com/BurntSushi/rust-csv> | 2026-09-21 |
| `printpdf` | **0.12.8** | 2026-09-05 | MIT | <https://crates.io/crates/printpdf> · <https://github.com/fschutt/printpdf> | 2026-09-21 |
| `genpdf` | **0.2.0** | **2021-06-17** | Apache-2.0 OR MIT | <https://crates.io/crates/genpdf> · <https://git.sr.ht/~ireas/genpdf-rs> | 2026-09-21 |
| `typst` | **0.15.1** | 2026-07-17 | Apache-2.0 | <https://crates.io/crates/typst> · <https://github.com/typst/typst> | 2026-09-21 |
| `tauri`（参考） | **2.11.6** | 2026-09-21 | MIT OR Apache-2.0 | <https://crates.io/crates/tauri> | 2026-09-21 |
| `tauri-plugin-sql`（参考） | **2.4.1** | 2026-09-21 | MIT OR Apache-2.0 | <https://crates.io/crates/tauri-plugin-sql> | 2026-09-21 |
| `libsqlite3-sys`（间接） | 0.38.2（最新）；sqlx 要求范围 `>=0.30.1, <0.38.0` | 2026-08-08 | MIT | <https://crates.io/crates/libsqlite3-sys> | 2026-09-21 |

**MSRV 汇总**（来自 crates.io 各版本 `rust_version` 字段，访问日期 2026-09-21）：

| crate | MSRV | 备注 |
| --- | --- | --- |
| `sqlx` 0.9.0 | **1.94.0** | 全场最高，是本项目工具链的实际下限 |
| `keyring` 4.2.0 | 1.88.0 | |
| `windows-native-keyring-store` 1.1.0 | 1.88 | |
| `keyring-core` 1.0.0 | 1.85 | |
| `printpdf` 0.12.8 | 1.88 | |
| `reqwest` 0.13.5 | 1.85.0 | |
| `uuid` 1.26.1 | 1.85.0 | |
| `csv` 1.4.0 | 1.73 | |
| `tokio` 1.53.1 | 1.71 | |
| `thiserror` 2.0.20 | 1.71 | |
| `serde_json` 1.0.151 | 1.71 | |
| `anyhow` 1.0.104 | 1.68 | |
| `chrono` 0.4.45 | 1.62.0 | |
| `serde` 1.0.229 | 1.56 | |
| `genpdf` 0.2.0 | 未声明 | |

> **⚠️ 本项直接影响本项目 `src-tauri/Cargo.toml`**：该文件当前写 `rust-version = "1.77"`，但 `sqlx 0.9.0` 要求 **1.94.0**，`keyring 4.2.0` 与 `printpdf 0.12.8` 要求 **1.88**，`reqwest 0.13.5` / `uuid 1.26.1` 要求 **1.85**。`rust-version = "1.77"` 与实际依赖树不自洽，应上调至 **1.94**。本次仅核对 crate 声明的 MSRV，未实际在 1.94 工具链上编译验证。

---

## D1. sqlx

### D1.1 版本与许可证

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 当前稳定版 | **0.9.0**（发布于 2026-05-21） | <https://crates.io/crates/sqlx> | 2026-09-21 |
| 许可证 | **MIT OR Apache-2.0** | <https://docs.rs/sqlx/0.9.0/> | 2026-09-21 |
| MSRV | **1.94.0**（`sqlx` 与 `sqlx-sqlite` 均为 1.94.0） | <https://crates.io/api/v1/crates/sqlx/0.9.0> | 2026-09-21 |
| 仓库 | <https://github.com/launchbadge/sqlx>，最后 push 2026-09-14，17488 stars，未归档 | GitHub REST API | 2026-09-21 |

### D1.2 `migrate!` 宏（逐字核实）

来源：<https://docs.rs/sqlx/latest/sqlx/macro.migrate.html>，访问日期 2026-09-21。

**签名（文档原文）**：

```rust
macro_rules! migrate {
    ($dir:literal) => { ... };
    () => { ... };
}
```

**可用性**：文档明确标注 `Available on crate features macros and migrate only.`——即必须同时启用 `macros` 与 `migrate` 两个 feature。

**行为**：`Embeds migrations into the binary by expanding to a static instance of Migrator.`（把迁移嵌入二进制，展开为 `Migrator` 的静态实例）。

**默认目录**：`// defaults to "./migrations"`

```rust
use sqlx::migrate::Migrator;
static MIGRATOR: Migrator = sqlx::migrate!();   // defaults to "./migrations"
```

**目录解析基准（易错点）**：文档原文——*"The directory must be relative to the project root (the directory containing `Cargo.toml`), unlike `include_str!()` which uses compiler internals to get the path of the file where it was invoked."*。即**相对于含 `Cargo.toml` 的项目根目录**，而不是调用处的源文件目录。

**推荐调用形式**：

```rust
sqlx::migrate!("db/migrations")
    .run(&pool)
    .await?;
```

**`sqlx.toml` 配置说明**：文档设有独立章节 "Configuration with `sqlx.toml`"，列举的 crate 级配置能力包括：

- creating schemas on database setup
- renaming the `_sqlx_migrations` table or placing it into a new schema
- relocating the migrations directory
- ignoring characters for hashing (such as whitespace and newlines)

并注明 `sqlx-cli` 也会读取这些选项。文档指向 "the configuration guide" 与 "reference `sqlx.toml`"。（**未核实**：`sqlx.toml` 的完整字段清单与 schema 定义本次未逐字抓取。）

**平台行尾问题（Windows 必读）**：文档原文——Linux/macOS 用 LF (`\n`)，Windows 用 CRLF (`\r\n`)，*"This may result in un-reproducible hashes across platforms"*。官方给出的方案是在 `.gitattributes` 中强制 SQL 文件用 LF：

```gitattributes
*.sql text eol=lf
```

另一个方案是配置迁移忽略空白字符（见 `sqlx.toml` 小节）。**对本项目意义**：Windows 开发 + 可能的多机协作下，若不处理会让迁移哈希在平台间不一致。

**重编译触发**：因 proc-macro 无法可靠监听外部文件，稳定版 Rust 的唯一办法是在项目里加 `build.rs`：

```rust
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
```

**迁移文件命名规则**（来源：<https://docs.rs/sqlx/latest/sqlx/migrate/trait.MigrationSource.html>，访问日期 2026-09-21，逐字）：

> *"All these scripts must be stored in files with names using the format `<VERSION>_<DESCRIPTION>.sql`, where `<VERSION>` is a string that can be parsed into `i64` and its value is greater than zero, and `<DESCRIPTION>` is a string. Files that don't match this format are silently ignored."*

要点：
- 形如 `20260921000001_create_todos.sql`
- `<VERSION>` 必须能解析为 `i64` 且 **> 0**
- **不匹配的文件被静默忽略**（不会报错）
- 可用 `sqlx migrate add <DESCRIPTION>` 生成空迁移脚本
- 迁移记录在数据库内的 `_sqlx_migrations` 表；**若某迁移的 hash 变化且已执行过，会报错**

### D1.3 `Migrator` 与 `run(&pool)`

来源：<https://docs.rs/sqlx/latest/sqlx/migrate/struct.Migrator.html>，访问日期 2026-09-21。

| 项目 | 结论 | 访问日期 |
| --- | --- | --- |
| 类型 | `pub struct Migrator { /* private fields */ }`，`Available on crate feature migrate only` | 2026-09-21 |
| 说明 | "A resolved set of migrations, ready to be run."；可经 `migrate!()` 静态构造，或经 `Migrator::new()` 运行时构造 | 2026-09-21 |
| `run` 签名 | `pub async fn run<'a, A>(&self, migrator: A) -> Result<(), MigrateError> where A: Acquire<'a>, <<A as Acquire<'a>>::Connection as Deref>::Target: Migrate` | 2026-09-21 |
| `run` 语义 | "Run any pending migrations against the database; and, validate previously applied migrations against the current migration source to detect accidental changes in previously-applied migrations." | 2026-09-21 |
| 官方示例 | `let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?; m.run(&pool).await` | 2026-09-21 |

**结论：`Migrator::run(&pool)` 正是官方示例用法**——`Acquire` 对 `&Pool` 有实现，所以直接传引用即可。

其他已核实方法：`Migrator::new(source)`、`Migrator::with_migrations(Vec<Migration>)`、`run_to(target, migrator)`、`skip(...)`、`dangerous_set_table_name(...)`（默认表名 `_sqlx_migrations`，官方带 "Potential Data Loss or Corruption!" 警告）、`create_schema(schema_name)`。

### D1.4 Cargo features（关键结论）

来源：crates.io `/api/v1/crates/sqlx/0.9.0` 的 `features` 字段，访问日期 2026-09-21。以下为逐字展开关系。

```toml
default = ["any", "macros", "migrate", "json"]

sqlite            = ["sqlite-bundled", "sqlite-deserialize", "sqlite-load-extension", "sqlite-unlock-notify"]
sqlite-bundled    = ["_sqlite", "sqlx-sqlite/bundled",   "sqlx-macros?/sqlite"]
sqlite-unbundled  = ["_sqlite", "sqlx-sqlite/unbundled", "sqlx-macros?/sqlite-unbundled"]

runtime-tokio = ["_rt-tokio", "sqlx-core/_rt-tokio", "sqlx-macros?/_rt-tokio"]
macros        = ["derive", "sqlx-macros/macros", "sqlx-core/offline", "sqlx-mysql?/offline", "sqlx-postgres?/offline", "sqlx-sqlite?/offline"]
migrate       = ["sqlx-core/migrate", "sqlx-macros?/migrate", "sqlx-mysql?/migrate", "sqlx-postgres?/migrate", "sqlx-sqlite?/migrate"]
```

**本项目所需的最小 feature 集**（`default-features = false` 时）：`sqlite`, `runtime-tokio`, `macros`, `migrate`。若需要 `DateTime`/`Uuid`/JSON 类型映射还需 `chrono`, `uuid`, `json`。本项目当前配置已包含全部这些，**这部分是正确的**。

### D1.5 是否默认捆绑 SQLite？——是

**来源：<https://docs.rs/sqlx/latest/sqlx/sqlite/index.html>，访问日期 2026-09-21，逐字：**

> **Static Linking (Default)**
> *"The `sqlite` feature enables the `bundled` feature of `libsqlite3-sys`, which builds SQLite 3 from included source code and statically links it into the final binary. This requires some C build tools to be installed on the system"*

> **Dynamic linking**
> *"To dynamically link to an existing SQLite library, the `sqlite-unbundled` feature can be used instead."*

**结论**：
- 启用 `sqlite` ⇒ 实际走 `sqlite-bundled` ⇒ `sqlx-sqlite/bundled` ⇒ `libsqlite3-sys/bundled`，**从源码编译 SQLite 3 并静态链接**，需要 C 构建工具链。
- 想动态链接则改用 `sqlite-unbundled`（此时 `sqlx-sqlite/unbundled` ⇒ `libsqlite3-sys/buildtime_bindgen`）。
- 官方对动态链接的提醒：SQLite 版本过旧或编译期关掉了某些特性会导致链接错误，建议 **SQLite ≥ 3.20.0（2018-08 发布）**。

**`libsqlite3-sys` 版本范围（0.9.0 新变化）**：`sqlx-sqlite 0.9.0` 依赖声明为 `libsqlite3-sys >=0.30.1, <0.38.0`（范围而非固定版本，故当前最新 0.38.2 **不在**范围内）。文档说明这样可让 Cargo 与 `rusqlite` 等共用一个版本；**若 `cargo update` 把 `libsqlite3-sys` 提到范围内不兼容的版本可能破坏构建**，官方建议必要时在自己的依赖里钉住版本（例如 `libsqlite3-sys = "0.34"`）。

### D1.6 `SqlitePoolOptions` 与事务 API

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `SqlitePoolOptions` 的真实身份 | **是类型别名，不是 struct**：`pub type SqlitePoolOptions = PoolOptions<Sqlite>;`（`Available on crate feature _sqlite only`） | <https://docs.rs/sqlx/latest/sqlx/sqlite/type.SqlitePoolOptions.html> | 2026-09-21 |
| 构造与连接 | `SqlitePoolOptions::new().connect("sqlite::memory:").await?`（`Migrator` 文档内官方示例） | <https://docs.rs/sqlx/latest/sqlx/migrate/struct.Migrator.html> | 2026-09-21 |
| 池配置方法 | `max_connections(u32)`、`get_max_connections()`、`min_connections(u32)`、`get_min_connections()`、`acquire_time_level(LevelFilter)` 等，均标注 **`Available on crate feature any only`** | <https://docs.rs/sqlx/latest/sqlx/pool/struct.PoolOptions.html> | 2026-09-21 |
| 开启事务 | `pub async fn begin(&self) -> Result<Transaction<'static, DB>, Error>`（"Retrieves a connection and immediately begins a new transaction."），以及 `try_begin()`；均标注 `Available on crate feature any only` | <https://docs.rs/sqlx/latest/sqlx/struct.Pool.html> | 2026-09-21 |
| 提交 / 回滚 | `pub async fn commit(self) -> Result<(), Error>`（"Commits this transaction or savepoint."）；`pub async fn rollback(self) -> Result<(), Error>` | <https://docs.rs/sqlx/latest/sqlx/struct.Transaction.html> | 2026-09-21 |
| 惰性池 | `Pool::connect_lazy(url: &str)`（`Available on crate feature any only`） | <https://docs.rs/sqlx/latest/sqlx/struct.Pool.html> | 2026-09-21 |

**两个坑**：
1. **`SqlitePoolOptions` 是别名**，所以 `docs.rs/sqlx/latest/sqlx/sqlite/struct.SqlitePoolOptions.html` 是 **404**；文档要看 `sqlx/pool/struct.PoolOptions.html` 与 `sqlx/sqlite/type.SqlitePoolOptions.html`。
2. 池与事务相关方法多带 **`Available on crate feature any only`**。由于 `sqlx` 的 `default` 包含 `any`，默认可用；但本项目用了 `default-features = false`，**`any` 必须显式列出**，否则 `max_connections` / `begin` 等不可用。**⚠️ 本项目当前 `Cargo.toml` 的 sqlx feature 列表中没有 `any`**（见 §D7）。

---

## D2. 序列化 / 时间 / 重复规则 / ID / 异步运行时

### D2.1 serde / serde_json

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `serde` 版本 / 许可证 | **1.0.229** / MIT OR Apache-2.0，MSRV 1.56 | <https://crates.io/crates/serde> | 2026-09-21 |
| `serde` features | `alloc`, `default`, `derive`, `rc`, `std`, `unstable`；`default = ["std"]` | 同上 | 2026-09-21 |
| **derive feature 名** | **`derive`**（逐字），用法 `serde = { version = "1", features = ["derive"] }` | 同上 | 2026-09-21 |
| `serde_json` 版本 / 许可证 | **1.0.151** / MIT OR Apache-2.0，MSRV 1.71 | <https://crates.io/crates/serde_json> | 2026-09-21 |
| `serde_json` features | `alloc`, `arbitrary_precision`, `default`, `float_roundtrip`, `preserve_order`, `raw_value`, `std`, `unbounded_depth`；`default = ["std"]` | 同上 | 2026-09-21 |

### D2.2 chrono / chrono-tz

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `chrono` 版本 / 许可证 | **0.4.45** / MIT OR Apache-2.0，MSRV 1.62.0 | <https://crates.io/crates/chrono> | 2026-09-21 |
| `chrono` 默认 features | `["clock", "std", "oldtime", "wasmbind"]` | 同上 | 2026-09-21 |
| `chrono-tz` 版本 / 许可证 | **0.10.4** / MIT OR Apache-2.0 | <https://crates.io/crates/chrono-tz> | 2026-09-21 |
| `chrono-tz` 发布日期 | **2025-07-11**（该 crate 自此后无新版本） | 同上 | 2026-09-21 |
| `chrono-tz` 仓库活动 | <https://github.com/chronotope/chrono-tz>，最后 push 2025-10-10，281 stars，未归档 | GitHub REST API | 2026-09-21 |

#### chrono-tz 提供什么

来源：<https://docs.rs/chrono-tz/latest/chrono_tz/> 与官方 README <https://github.com/chronotope/chrono-tz>，访问日期 2026-09-21。

- 为 `chrono` 提供 `TimeZone` trait 的实现者。
- **文档原文**：*"The impls are generated by a build script using the IANA database and `zoneinfo_parse`."*（实现由 build script 用 **IANA database** 与 `zoneinfo_parse` 生成）。README 同义表述为 *"generated by a build script using the IANA database and `parse-zoneinfo`"*。
- 提供 `chrono_tz::Tz` 枚举（可由字符串 `FromStr` 解析，例如 `"Antarctica/South_Pole".parse::<Tz>()`），以及按区域的模块路径如 `chrono_tz::US::Pacific`、`chrono_tz::Europe::London`、`chrono_tz::Asia::Kolkata`、`chrono_tz::UTC`。
- 提供 `OffsetComponents` trait（`base_utc_offset()` / `dst_offset()`），可分别取基准 UTC 偏移与 DST 偏移。

#### 「静态内置 IANA、不读 OS 时区数据库」——核实结果

**结论：成立，但需精确表述证据来源。**

| 论据 | 原文 | 来源 | 访问日期 |
| --- | --- | --- | --- |
| 时区表由 build script 生成、编译进产物 | *"The impls are generated by a build script using the IANA database and zoneinfo_parse."* | <https://docs.rs/chrono-tz/latest/chrono_tz/> | 2026-09-21 |
| 构建期可裁剪静态表 | `filter-by-regex` feature + 环境变量 `CHRONO_TZ_TIMEZONE_FILTER`（正则），*"This can significantly reduce the size of the generated database"* | 官方 README | 2026-09-21 |
| **动态加载尚未实现** | README 的 **"Future Improvements"** 明确列出 *"Dynamic tzdata loading"* | 官方 README | 2026-09-21 |
| README 其他 Future Improvements | *"Handle leap seconds"*、*"Handle Julian to Gregorian calendar transitions"*、*"Load tzdata always from latest version"* | 官方 README | 2026-09-21 |

**推理链**：时区实现由构建脚本从 IANA 数据生成并嵌入二进制 ⇒ 运行时数据是**构建时快照**；官方把 *"Dynamic tzdata loading"* 与 *"Load tzdata always from latest version"* 列为**未来**改进 ⇒ 当前版本**没有**运行时加载路径。因此 chrono-tz **不会读取操作系统时区数据库**（Windows 的时区注册表、`TZDIR` 下的 zoneinfo 文件均不参与）。

**注意**：官方文档**没有**出现 "does not read the OS timezone database" 这类逐字表述——上述结论是从"构建期生成" + "动态加载属未来项"两条官方陈述推出的。若报告需要严格逐字引用，请引用上表两条原文。

**Windows 相关 caveat**：
- **官方文档未提供针对 Windows 的特殊说明或已知问题** → **未核实**（不以推测充数）。
- 可陈述的客观事实：Windows 本身不提供 Unix 风格的 `zoneinfo` 文件树，依赖 OS 时区数据的方案在 Windows 上需要额外处理；chrono-tz 的静态内置模型反而与平台无关、行为可复现。**这是分析性判断，不是官方文档结论。**
- 实际影响：chrono-tz 内置的 tzdata 只能随 crate 升级更新。**若产品需要跟随系统时区变更（例如微软调整某地区 DST 规则），必须升级 `chrono-tz` 并重新发版**，无法靠系统更新自动生效。

### D2.3 rrule（RFC 5545 RRULE）

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 | **0.14.0** / MIT OR Apache-2.0（crates.io 与 docs.rs 一致） | <https://crates.io/crates/rrule> | 2026-09-21 |
| 仓库 | <https://github.com/fmeringdal/rust-rrule> | 同上 | 2026-09-21 |
| 发布日期 | **2025-04-20** | crates.io API | 2026-09-21 |
| 仓库最后 push | **2025-04-20**（与 0.14.0 发布同日，此后无推送） | GitHub REST API | 2026-09-21 |
| stars / open issues / 归档 | 95 / 34 / **未归档** | GitHub REST API | 2026-09-21 |
| 累计下载 | **1,225,467** | crates.io API | 2026-09-21 |
| 依赖 | `chrono ^0.4.39`、`chrono-tz ^0.10.1`、`regex ^1.11.1`、`thiserror ^2.0.11`，可选 `clap ^4.5.26`、`serde_with ^3.12.0` | <https://docs.rs/rrule/0.14.0/> | 2026-09-21 |

#### 成熟度评估（诚实版）

**支持面（官方 README / docs.rs 逐字核实）**：

| 能力 | 结论 | 证据 | 访问日期 |
| --- | --- | --- | --- |
| RFC 5545 遵循声明 | 官方 README：*"This crate follows the iCalendar (RFC-5545) specification for the 'Recurrence Rule'."* 注意这是**自我声明**，本次未找到第三方合规测试报告 | 官方 README | 2026-09-21 |
| **`BYSETPOS`** | **支持**。`RRule` 上有 `by_set_pos` 构造方法与 `get_by_set_pos` 读取方法 | <https://docs.rs/rrule/latest/rrule/struct.RRule.html> | 2026-09-21 |
| **`BYDAY` 数字前缀** | **支持**。`pub enum NWeekday { Every(Weekday), Nth(i16, Weekday) }`；文档原文：*"`NWeekday::Nth(1, MO)` represents the first Monday within the month or year, whereas `NWeekday::Nth(-1, MO)` represents the last Monday of the month or year. And `NWeekday::Every(MO)` means all Mondays."* | <https://docs.rs/rrule/latest/rrule/enum.NWeekday.html> | 2026-09-21 |
| 题目示例 `FREQ=MONTHLY;BYDAY=MO;BYSETPOS=2` | **可表达**（BYDAY=`NWeekday::Every(MO)` 或 `Nth`，BYSETPOS 走 `by_set_pos`） | 同上两条 | 2026-09-21 |
| `RRuleSet` 组合 | 支持多 `RRule` 并集（`A ∪ B`）、`RDATE` 并集、`EXDATE` 排除；另支持 `EXRULE`（RFC 2445 遗留） | 官方 README | 2026-09-21 |
| **feature 门控项** | `EXRULE` 需启用 feature **`exrule`**（默认关闭）；`BYEASTER` 需启用 feature **`by-easter`** | 官方 README | 2026-09-21 |
| 硬上限 / 无限重复 | `RRuleSet::all(limit)` 需传入上限（README 示例 `let limit = 100;`）；无限规则靠此截断 | 官方 README | 2026-09-21 |
| 绕过校验 | `RRuleSet::all_unchecked`，或直接用 `Iterator` API | 官方 README | 2026-09-21 |
| 年份范围 | 因 Chrono 限制，所有日期被限制在 **±262,000 年** | 官方 README | 2026-09-21 |
| 时区支持 | *"Supported timezones are limited to by the timezones that Chrono-Tz supports. This is equivalent to the IANA database."* 即 **`TZID` 能力受 chrono-tz 约束** | 官方 README | 2026-09-21 |
| 解析入口 | `RRuleSet` 实现 `FromStr`，可 `"DTSTART:20120201T093000Z\nRRULE:FREQ=DAILY;COUNT=3".parse()` | 官方 README | 2026-09-21 |
| 安全提示 | README 有独立 **Security** 章节，建议在用户可任意输入规则时先读 security docs（因校验限制只在 `all` 上强制） | 官方 README | 2026-09-21 |

**官方记录的校验限制表（`RRuleSet::all` 上强制，逐字）**：

| 项目 | 任意上限 | crate 上限 |
| --- | --- | --- |
| 年份范围 | `-10_000..=10_000` | `-262_000..=262_000`（Chrono） |
| `FREQ=YEARLY` 的 interval | 10_000 | 65_535 (`u16::MAX`) |
| `FREQ=MONTHLY` 的 interval | 1_000（约 83 年） | 65_535 |
| `FREQ=WEEKLY` 的 interval | 1_000（约 19 年） | 65_535 |
| `FREQ=DAILY` 的 interval | 10_000（约 27 年） | 65_535 |
| `FREQ=HOURLY` 的 interval | 10_000（约 416 天） | 65_535 |

**成熟度判断（诚实）**：
- **正面**：RRULE 本体功能面完整度看起来好——`BYSETPOS`、`BYDAY` 数字前缀、`RDATE`/`EXDATE`/`EXRULE`、`DTSTART` 解析都有，且有 122 万次累计下载与 95 stars。
- **风险**：**维护活跃度低**。0.14.0 发布于 2025-04-20，**仓库最后 push 与发布同日**，其后约 17 个月无任何推送；同时积压 34 个 open issues。这是"发布即停更"的典型形态——**不能假定它能快速跟进需求或修 bug**。
- **文档完整度**：docs.rs 显示该 crate 仅 **27.55%** 有文档（docs.rs 页面自报），低于 `sqlx` 的 100%。
- **未核实**：是否通过 RFC 5545 官方/第三方一致性测试套件；`EXDATE`/`RDATE` 的边界行为（如与 DST 转换叠加）；`TZID` 在 `DTSTART` 内的解析细节。

#### 与 rrule.js 对比

| 维度 | `rrule` (Rust) | `rrule` (npm / rrule.js) |
| --- | --- | --- |
| 当前版本 | 0.14.0 | **2.8.1**（npm `dist-tags.latest`） |
| 许可证 | MIT OR Apache-2.0 | **BSD-3-Clause** |
| 最后发布 | **2025-04-20** | **2023-11-10** |
| registry 最后修改 | 2025-04-20 | **2023-11-10T20:05:55.900Z**（即发布后无任何元数据变更） |
| 仓库 | github.com/fmeringdal/rust-rrule | github.com/jakubroztocil/rrule |
| 累计下载 | 1,225,467（crates.io） | 未核实（本次未取 npm 下载数） |
| 访问日期 | 2026-09-21 | 2026-09-21 |

**关于「rrule.js 是否基本无人维护」**：**证据支持这一判断**。npm registry 显示其最新版 `2.8.0` / `2.8.1` 均在 **2023-11-10** 同日发布，此后 `modified` 时间戳也停在 **2023-11-10**——即近 3 年无新发布、无元数据更新。版本序列的末端为 `2.6.9`(2022-03-03) → `2.7.0`(2022-06-05) → `2.7.1`(2022-07-11) → `2.7.2`(2023-02-10) → `2.8.0`/`2.8.1`(2023-11-10)，节奏明显放缓后停滞。（**未核实**：rrule.js 的 GitHub 仓库 commit 活动与 issue 响应情况，本次只用了 npm registry 一手数据。）

**哪一个更完整？——本次不做断言**。
可确证的事实是：两者都在 RFC 5545 RRULE 上提供了 `BYSETPOS` 与带数字前缀的 `BYDAY` 所需的表达能力（rrule.js 的对应能力本次**未核实**，故不逐项比较）；两者的发布节奏都已放缓，但 **rrule.js 停更更久（2023-11 vs 2025-04）**。
**未核实**：二者的逐特性完整度矩阵、对 `EXDATE`/`RDATE`/`TZID` 的覆盖差异，以及各自的已知缺陷清单。若项目需要据此选型，建议单独做一次特性对照测试。

**对本项目的实务建议**：既然要在 Rust 侧算重复规则（跨时区 + DST），`rrule` 0.14.0 可用且是当前 crates.io 上的主力实现；但要接受"上游不活跃"的现实，把重复规则的解析结果做缓存/落库，避免每次启动都重算，并在升级 Rust 工具链时留意其与 `chrono`/`chrono-tz` 的版本耦合。

### D2.4 uuid

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 | **1.26.1** / **Apache-2.0 OR MIT**（注意顺序与 serde 相反） | <https://crates.io/crates/uuid> | 2026-09-21 |
| 发布日期 / MSRV | 2026-09-10 / 1.85.0 | 同上 | 2026-09-21 |
| features（全量） | `atomic`, `borsh`, `default`, `fast-rng`, `js`, `macro-diagnostics`, `md5`, `rng`, `rng-getrandom`, `rng-rand`, `sha1`, `serde`, `std`, `v1`, `v3`, `v4`, `v5`, `v6`, `v7`, `v8`；`default = ["std"]` | 同上 | 2026-09-21 |
| **`v4` feature** | **存在**（逐字 `v4`） | 同上 | 2026-09-21 |
| **`v7` feature** | **存在**（逐字 `v7`） | 同上 | 2026-09-21 |
| **`serde` feature** | **存在**（逐字 `serde`） | 同上 | 2026-09-21 |

本项目建议写法（已正确）：`uuid = { version = "1", features = ["v4", "v7", "serde"] }`。

### D2.5 tokio

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 | **1.53.1** / **MIT**（crates.io 的 `license` 字段为单许可 `MIT`，非 `MIT OR Apache-2.0`） | <https://crates.io/crates/tokio> | 2026-09-21 |
| 发布日期 / MSRV | 2026-07-20 / 1.71 | 同上 | 2026-09-21 |
| 可用 features（全量） | `default`, `fs`, `full`, `io-std`, `io-uring`, `io-util`, `macros`, `net`, `process`, `rt`, `rt-multi-thread`, `schedule-latency`, `signal`, `sync`, `taskdump`, `test-util`, `time`；`default = []` | 同上 | 2026-09-21 |
| 本项目所需 feature 是否都存在 | `rt-multi-thread` ✅、`macros` ✅、`sync` ✅、`time` ✅、`fs` ✅ —— **全部存在** | 同上 | 2026-09-21 |

**说明**：`rt-multi-thread` + `macros` + `sync` + `time` + `fs` 是常见组合且均已核实存在。但"最小集"取决于 Tauri 自身对 tokio 的启用情况——`tauri 2.x` 本身会引入 tokio，实际编译进去的 feature 是各依赖的并集。**未核实**：Tauri 2.11.6 具体启用了 tokio 的哪些 feature，因此无法断言项目侧写成上述 5 项是否"最小"。本项目当前写的是 `["rt-multi-thread", "macros", "sync", "time"]`（缺 `fs`）——若 Rust 侧需要异步文件读写（备份/导出），需补 `fs`。

---

## D3. keyring（v4 架构变更，重点）

### D3.1 版本与 feature 结构

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 | **4.2.0** / MIT OR Apache-2.0，MSRV **1.88.0** | <https://crates.io/crates/keyring> | 2026-09-21 |
| 发布日期 | **2026-08-29** | 同上 | 2026-09-21 |
| 仓库 | <https://github.com/open-source-cooperative/keyring-rs>（注意组织名已从 `hwchen/keyring-rs` 迁出），最后 push 2026-09-15，768 stars，0 open issues，未归档 | GitHub REST API | 2026-09-21 |
| 维护者 | README：原作者 Walther Chen (hwchen)，**当前维护者 Dan Brotsky (brotskydotcom)** | 官方 README | 2026-09-21 |

**v4.2.0 的 features —— 只有两个（crates.io 逐字）：**

```toml
default = ["v1"]

v1 = [
  "apple-native-keyring-store/keychain",
  "windows-native-keyring-store",
  "zbus-secret-service-keyring-store",
]

cli = [
  "android-native-keyring-store",
  "apple-native-keyring-store/keychain",
  "apple-native-keyring-store/protected",
  "db-keystore",
  "dbus-secret-service-keyring-store",
  "keyring-core/sample",
  "linux-keyutils-keyring-store",
  "windows-native-keyring-store",
  "zbus-secret-service-keyring-store",
]
```

**逐字回答问题**：

| 问题 | 答案 | 证据 | 访问日期 |
| --- | --- | --- | --- |
| v4 有哪些 feature？ | **恰好两个：`v1` 与 `cli`**（`default = ["v1"]`） | crates.io features 字段 + docs.rs | 2026-09-21 |
| 旧的 `windows-native` feature 是否在 v4 被移除？ | **是，已移除**。v4.2.0 的 feature 集合中不存在 `windows-native` | crates.io features 字段 | 2026-09-21 |
| 是否正确/需要 `windows-native-keyring-store`？ | **是**。v4 已把各平台后端拆成独立 crate，Windows 后端就是 `windows-native-keyring-store`；`v1` feature 会**自动带上**它 | crates.io features 字段 | 2026-09-21 |
| 启用它的 feature 名是什么？ | 对 `keyring` 而言是 **`v1`**（默认）；对直接依赖 store crate 的路线，则是把 `windows-native-keyring-store` 作为**独立依赖**引入（其自身无"windows 平台开关"这类 feature，只有 `default = ["search"]` / `search = ["dep:regex"]`） | 同上 | 2026-09-21 |

### D3.2 v4 的两种模式与官方取舍指引（逐字）

来源：<https://docs.rs/keyring/latest/keyring/>，访问日期 2026-09-21。

- 该库按启用 `v1` 还是 `cli` 运行于两种模式之一，两者可同时启用但通常只用其一。
- **`v1` 模式**：*"If you enable the `v1` feature, this library behaves essentially the same as the `v1` version of Keyring behaved: it allows easy, platform-independent setting and reading of passwords/secrets on macOS, Windows, and \*nix platforms."*
- `v1` 模块文档进一步明确：*"On Windows, the secure credential store is the Windows Credential Manager."*，且该模块导出 `Entry` 类型用于 set/get/delete。
- **`cli` 模式**：为 Rust CLI 示例应用、`rust-native-keyring` Python 模块、`keyring-demo` 跨平台应用提供"访问所有可用凭据存储"的胶水代码。
- **关键取舍（逐字）**：*"Note that neither of these modes are either useful for or meant for use by applications which want to control which credential stores they use on which platforms... Such applications should not be linking to this library at all; they should instead be linking to the `keyring-core` library and any specific credential stores they want to use."*

**推论（对本项目的直接指导）**：
- 只想要"在 Windows 上读写一个密钥" ⇒ 用 `keyring 4` + 默认 `v1` feature 即可，一行依赖、API 与旧版一致。
- 想**明确控制只用 Windows Credential Manager**、不掺入 Apple/Secret Service 相关依赖 ⇒ 官方推荐路线是**不依赖 `keyring` 本身**，而是依赖 `keyring-core` + `windows-native-keyring-store`。

### D3.3 Windows Credential Manager 后端在 v4 中的正确用法

#### 路线 A：`keyring` 4 + 默认 `v1`（简单，推荐给本项目）

```toml
keyring = "4"          # default = ["v1"]，Windows 上自动用 Windows Credential Manager
```

代码侧使用 `keyring::Entry`（即 `v1` 模块导出的类型）做 set/get/delete。

#### 路线 B：`keyring-core` + `windows-native-keyring-store`（官方推荐给需要控制 store 的应用）

来源：<https://docs.rs/windows-native-keyring-store/latest/windows_native_keyring_store/>，访问日期 2026-09-21。

文档 **Usage** 小节逐字给出的初始化代码：

```rust
keyring_core::set_default_store(windows_native_keyring_store::Store::new().unwrap())
```

其余已核实要点（同一页面）：

| 项目 | 结论 | 访问日期 |
| --- | --- | --- |
| 后端 | *"uses the Windows Credential Manager as its back end"* | 2026-09-21 |
| 凭据映射 | 每条 entry 映射为一个 **generic credential**；Windows 侧唯一标识是 `target_name` | 2026-09-21 |
| `target_name` 生成规则 | 若创建时给了显式 target modifier 就用它；否则由 **prefix + user + delimiter + service + suffix** 拼接，默认 prefix/suffix 为**空串**、delimiter 为 **`.`** | 2026-09-21 |
| **碰撞风险（官方明示）** | *"service and user strings, by default, can contain the delimiter string, so it is possible for entries with different service and user strings to map to the same description (and thus the same credential in the store)."* 可通过配置禁止 service 中出现 delimiter 来规避 | 2026-09-21 |
| 持久化类型 | `CredPersist` 枚举：`Session` / `Local` / `Enterprise`；**默认 `Enterprise`**；仅在写入密钥时生效 | 2026-09-21 |
| 属性 | `target_name`（只读）、`target_alias`、`username`、`comment`；经 `get_attributes` 读、`update_attributes` 写 | 2026-09-21 |
| 搜索 | 支持按正则匹配 `target_name`；`search` 即其默认 feature（`default = ["search"]`） | 2026-09-21 |
| **官方警告 1** | *"operating on the same entry from different threads does not reliably sequence the operations... So be careful not to access the same entry on multiple threads simultaneously."* | 2026-09-21 |
| **官方警告 2** | 在读取前立刻更改凭据的 persistence 类型可能导致读取失败，尤其在凭据管理器多线程繁忙时 | 2026-09-21 |

> **对本项目的重要提示**：官方警告 1 直接命中 Tauri 场景——Tauri 命令可能在不同线程被并发调用。**对同一个 `Entry` 的读写必须串行化**（例如用 `tokio::sync::Mutex` 或单线程命令通道），不要假定多线程访问同一 entry 会按调用顺序执行。

配套 crate 版本：

| crate | 版本 | 许可证 | MSRV | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `keyring-core` | **1.0.0**（2026-04-21） | MIT OR Apache-2.0 | 1.85 | <https://crates.io/crates/keyring-core> | 2026-09-21 |
| `windows-native-keyring-store` | **1.1.0**（2026-05-24） | MIT OR Apache-2.0 | 1.88 | <https://crates.io/crates/windows-native-keyring-store> | 2026-09-21 |

`keyring-core` 的 features 只有 `sample = ["dep:dashmap","dep:ron","dep:chrono","dep:regex","dep:serde","dep:uuid"]`（用于示例程序），普通使用无需开启。

### D3.4 上一代大版本（v2 / v3）用的是什么

来源：crates.io 各版本 `features` 字段，访问日期 2026-09-21。

| 版本线 | 代表版本 | 发布日期 | Windows 相关 feature（逐字） | 说明 |
| --- | --- | --- | --- | --- |
| **v3.x** | **3.6.3**（v3 末版） | 2025-07-27 | **`windows-native = ["dep:windows-sys", "dep:byteorder"]`** | 单 crate 内建平台后端；v3 MSRV **1.75** |
| v3.x 其他 feature | 3.6.3 | — | `apple-native`, `linux-native`, `sync-secret-service`, `async-secret-service`, `crypto-rust`, `crypto-openssl`, `tokio`, `async-io`, `vendored`, `linux-native-sync-persistent`, `linux-native-async-persistent` | 注意 v3 **没有** `default` 项显式列出（crates.io 该版本未给出 `default` 键） |
| **v2.x** | **2.3.3**（v2 末版） | 2024-05-02 | **`platform-windows = ["windows-sys", "byteorder"]`**，且 `default = ["platform-all"]` | 平台粒度为 `platform-*` 系列 |

**v4 的发布节奏**（crates.io，访问日期 2026-09-21）：`4.0.0-alpha.1`(2025-03-12) → `4.0.0-beta.*`(2025-03) → … → `4.0.0`(**2026-04-26**) → `4.1.0`(2026-06-17) → `4.2.0`(**2026-08-29**，当前最新)。注意 `4.0.0-rc.1` 早在 2025-03-15 就出现，但正式 `4.0.0` 直到 2026-04-26 才发布——v4 的成熟化过程跨度超过一年。

### D3.5 该钉哪个版本？（给本项目的建议）

| 方案 | 依赖写法 | 优点 | 代价 |
| --- | --- | --- | --- |
| **推荐：`keyring` 4 + 默认 v1** | `keyring = "4"` | API 与旧版一致（`Entry`）；Windows 上自动用 Credential Manager；上游活跃（最后 push 2026-09-15） | v1 模式会一并拉入 apple/secret-service 相关 store crate，Windows-only 应用会编译进不需要的依赖 |
| 官方推荐：`keyring-core` + store | `keyring-core = "1"` + `windows-native-keyring-store = "1"` | 依赖树最干净，完全掌控后端；官方明确推荐给"要控制 store"的应用 | 需要自己写初始化（一行 `set_default_store`）；API 面比 `Entry` 更底层 |
| 保守：钉 v3 | `keyring = "3.6"` | MSRV 仅 1.75；单 crate、无 store 拆分；`windows-native` feature 直觉清晰 | **无新版修复**（v3 末版 2025-07-27，已停止演进）；与 v4 生态脱节 |

**本项目应选哪个**：`keyring 4`（`default` 即 `v1`）**或** `keyring-core` + `windows-native-keyring-store`。
**钉 v3 是否合理**：在不介意"不再收到修复"、且希望 MSRV 保持较低时是合理的保守选择；但对新项目**不推荐**——v3 自 2025-07 起无更新，而 v4 才是当前维护线。
**⚠️ 无论选哪个，本项目当前 `Cargo.toml` 里的 `features = ["windows-native"]` 在 v4 下必然编译失败**（详见 §D7）。

### D3.6 `tauri-plugin-keyring` 是否官方？——不是

| 项目 | 结论 | 证据 | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 发布 | **0.1.0**，**仅此一个版本**，发布于 **2024-12-23**（此后无更新） | crates.io API（`num_versions: 1`） | 2026-09-21 |
| 许可证 | MIT | 同上 | 2026-09-21 |
| 描述 | "A tauri plugin wrapper for the keyring crate" | 同上 | 2026-09-21 |
| **`repository` 字段** | **`null`**（未声明任何仓库） | crates.io API | 2026-09-21 |
| **Owner** | **`HuakunShen`（个人开发者）**，非 `tauri-apps` 组织 | crates.io `/owners` 端点 | 2026-09-21 |
| **是否在官方 `tauri-apps/plugins-workspace`？** | **否**。已列举该仓库 `plugins/` 目录全部 30 个子目录：`autostart, barcode-scanner, biometric, cli, clipboard-manager, deep-link, dialog, fs, geolocation, global-shortcut, haptics, http, localhost, log, nfc, notification, opener, os, persisted-scope, positioner, process, shell, single-instance, sql, store, stronghold, updater, upload, websocket, window-state` —— **其中没有 keyring** | GitHub REST API `/repos/tauri-apps/plugins-workspace/contents/plugins` | 2026-09-21 |
| 依赖的 keyring 版本 | **`keyring ^3.6.1`** | crates.io `/dependencies` 端点 | 2026-09-21 |
| 下载量 | 44,411（累计），recent_downloads 20,254 | crates.io API | 2026-09-21 |

**结论**：`tauri-plugin-keyring` 是**第三方社区/个人插件**，不是 Tauri 官方插件，也不在官方插件工作区中。它**已近 2 年未更新**，且**锁定在 `keyring ^3.6.1`**——与 `keyring` 4.x **不兼容**（4.0 是 major bump），因此它与本项目的 `keyring 4` 计划无法共存。README 也显示其 JS API 是 `getPassword` / `setPassword` / `deletePassword` 的简单包装。

**官方侧的相关能力**：官方插件列表中存在 **`stronghold`**（`tauri-plugin-stronghold`），是 Tauri 官方提供的密钥/敏感数据存储方案。**未核实**：`stronghold` 是否适合替代 Windows Credential Manager 的用例，本次未调研其文档。

---

## D4. reqwest

### D4.1 版本与许可证

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 | **0.13.5** / MIT OR Apache-2.0，MSRV **1.85.0** | <https://crates.io/crates/reqwest> | 2026-09-21 |
| 发布日期 | **2026-09-08** | 同上 | 2026-09-21 |
| 仓库 | <https://github.com/seanmonstar/reqwest>，最后 push 2026-09-22，11835 stars，未归档 | GitHub REST API | 2026-09-21 |

### D4.2 默认 TLS 后端 —— **0.13 已改为 rustls，不再是 native-tls/SChannel**

**这是 0.13 的破坏性变更，来源：官方 CHANGELOG <https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md>，`# v0.13.0` 小节 "Breaking changes" 逐字：**

> - **`rustls` is now the default TLS backend, instead of `native-tls`.**
> - `rustls` crypto provider defaults to aws-lc instead of `_ring_`. (`rustls-no-provider` exists if you want a different crypto provider)
> - **`rustls-tls` has been renamed to `rustls`.**
> - rustls roots features removed, `rustls-platform-verifier` is used by default.
> - To use different roots, call `tls_certs_only(your_roots)`.
> - `native-tls` now includes ALPN. To disable, use `native-tls-no-alpn`.
> - `query` and `form` are now crate features, disabled by default.
> - Long-deprecated methods and crate features have been removed (such as `trust-dns`, which was renamed `hickory-dns` a while ago).
> - Many TLS-related methods renamed to improve autocompletion and discovery, but previous name left in place with a "soft" deprecation. (just documented, no warnings) — *For example, prefer `tls_backend_rustls()` over `use_rustls_tls()`.*

**crates.io 的 feature 定义印证了这一点（逐字，访问日期 2026-09-21）**：

```toml
default    = ["default-tls", "charset", "http2", "system-proxy"]
default-tls = ["rustls"]                       # ← 0.13 中 default-tls 指向 rustls
rustls      = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]
rustls-no-provider = ["dep:rustls-platform-verifier", "__rustls"]
```

| 问题 | 结论 | 访问日期 |
| --- | --- | --- |
| Windows 上的默认 TLS 后端 | **rustls**（crypto provider = **aws-lc-rs**），根证书走 **`rustls-platform-verifier`**（即使用平台信任库）。**不是** native-tls / SChannel | 2026-09-21 |
| 想用 SChannel / native-tls 怎么办 | 显式启用 `native-tls` feature（`native-tls = ["__native-tls","__native-tls-alpn"]`），通常配 `default-features = false` | 2026-09-21 |

### D4.3 feature 存在性核对（题目点名项）

| feature | 是否存在 | 说明 | 访问日期 |
| --- | --- | --- | --- |
| `rustls-tls` | **不存在（已改名）** | 0.13.0 起重命名为 **`rustls`**。写 `rustls-tls` 会编译失败 | 2026-09-21 |
| `rustls` | **存在** | 新的 rustls 开关（`["__rustls-aws-lc-rs","dep:rustls-platform-verifier","__rustls"]`） | 2026-09-21 |
| `rustls-no-provider` | **存在** | 想换 crypto provider 时用 | 2026-09-21 |
| `default-tls` | **存在** | 但语义已变：`= ["rustls"]` | 2026-09-21 |
| `native-tls` | **存在** | 含 ALPN | 2026-09-21 |
| `native-tls-no-alpn` | **存在** | 关掉 ALPN | 2026-09-21 |
| `native-tls-vendored` / `-no-alpn` | **存在** | | 2026-09-21 |
| **`json`** | **存在** | `= ["dep:serde","dep:serde_json"]` | 2026-09-21 |
| **`blocking`** | **存在** | `= ["dep:futures-channel","futures-channel?/sink","dep:futures-util","futures-util?/io","futures-util?/sink","tokio/sync"]` | 2026-09-21 |
| 其他可用 features | `brotli`, `charset`, `cookies`, `deflate`, `form`, `gzip`, `hickory-dns`, `http2`, `http3`, `multipart`, `query`, `socks`, `stream`, `system-proxy`, `zstd` | | 2026-09-21 |

> **⚠️ 隐藏坑（对本项目重要）**：`system-proxy` 是 **`default` 的一部分**（`= ["hyper-util/client-proxy-system"]`），而 `query` 与 `form` 在 0.13 **改成了默认关闭的 feature**。本项目当前用 `default-features = false`，于是**同时失去系统代理支持、HTTP/2 与 charset**。中国的桌面用户经常依赖系统代理访问境外 API，**`default-features = false` 会导致代理设置被忽略**。见 §D7 建议。

### D4.4 超时配置 API（逐个核实存在）

来源：<https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html>，访问日期 2026-09-21。入口 `Client::builder() -> ClientBuilder`（文档原文：*"Creates a `ClientBuilder` to configure a `Client`. This is the same as `ClientBuilder::new()`."*，来源 <https://docs.rs/reqwest/latest/reqwest/struct.Client.html>）。

| 方法（逐字签名） | 语义（文档原文摘要） | 默认值 | 访问日期 |
| --- | --- | --- | --- |
| `pub fn timeout(self, timeout: Duration) -> ClientBuilder` | *"Enables a total request timeout. The timeout is applied from when the request starts connecting until the response body has finished. Also considered a total deadline."* | **无超时**（"Default is no timeout."） | 2026-09-21 |
| `pub fn connect_timeout(self, timeout: Duration) -> ClientBuilder` | *"Set a timeout for only the connect phase of a `Client`."* | `None` | 2026-09-21 |
| `pub fn read_timeout(self, timeout: Duration) -> ClientBuilder` | *"Enables a read timeout. The timeout applies to each read operation, and resets after a successful read. This is more appropriate for detecting stalled connections when the size isn't known beforehand."* | **无超时** | 2026-09-21 |
| `pub fn pool_idle_timeout<D>(self, val: D) -> ClientBuilder where D: Into<Option<Duration>>` | *"Set an optional timeout for idle sockets being kept-alive. Pass `None` to disable timeout."* | **90 秒** | 2026-09-21 |
| `pub fn pool_max_idle_per_host(self, max: usize) -> ClientBuilder` | Sets the maximum idle connection per host allowed in the pool. | `usize::MAX`（无限制） | 2026-09-21 |

**注意**：`connect_timeout` 文档注明 *"This requires the futures be executed in a tokio runtime with a tokio timer enabled."* —— 即必须启用 `tokio` 的 `time` feature。

**三者区别（实务）**：`timeout` 是**总截止时间**（含连接 + 读取 body）；`connect_timeout` 只管连接阶段；`read_timeout` 管**每次读操作**并在成功读取后重置，适合"响应体大小未知、要检测卡死"的场景。对 AI 流式（SSE）请求，**不要设总 `timeout`**，否则长连接会被整体掐断；应用 `read_timeout`。

其他已核实（0.13.x 新增，来自 CHANGELOG）：`ClientBuilder::http1_max_headers(usize)`（0.13.5 新增，默认 100）、`ClientBuilder::tls_sslkeylogfile(bool)`（0.13.4）、`ClientBuilder::http2_keep_alive_*`（0.13.4）、`Error::is_dns()`（0.13.5 新增）。

---

## D5. thiserror / anyhow

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `thiserror` 版本 / 许可证 | **2.0.20** / MIT OR Apache-2.0，MSRV 1.71 | <https://crates.io/crates/thiserror> | 2026-09-21 |
| `thiserror` features | 仅 `default = ["std"]` 与 `std` | 同上 | 2026-09-21 |
| `anyhow` 版本 / 许可证 | **1.0.104** / MIT OR Apache-2.0，MSRV 1.68 | <https://crates.io/crates/anyhow> | 2026-09-21 |
| `anyhow` features | `backtrace`, `default`, `std`；`default = ["std"]` | 同上 | 2026-09-21 |
| 仓库活跃度 | `dtolnay/thiserror` push 2026-09-22；`dtolnay/anyhow` push 2026-08-22；均未归档 | GitHub REST API | 2026-09-21 |

### D5.1 thiserror 2.x 的 `#[derive(Error)]` 与 `#[from]` —— 已确认

来源：<https://docs.rs/thiserror/latest/thiserror/>，访问日期 2026-09-21，逐字要点：

- *"This library provides a convenient derive macro for the standard library's `std::error::Error` trait."* 用法：`use thiserror::Error;` + `#[derive(Error, Debug)]`。
- 错误类型可以是 enum、带具名字段的结构体、元组结构体或单元结构体。
- 提供 `#[error("...")]` 生成 `Display`；支持插值简写 `#[error("{var}")]` → `write!("{}", self.var)`、`#[error("{0}")]`、`#[error("{var:?}")]`、`#[error("{0:?}")]`，并可附任意格式化参数（含引用字段的 `.var` / `.0` 形式）。
- **`#[from]` 支持**：*"A `From` impl is generated for each variant that contains a `#[from]` attribute."* 约束：*"The variant using `#[from]` must not contain any other fields beyond the source error (and possibly a backtrace...)"*。具名字段也可用 `#[from]`。
- `#[source]` / 名为 `source` 的字段用于 `source()`；*"The `#[from]` attribute always implies that the same field is `#[source]`, so you don't ever need to specify both attributes."*
- 设计定位（原文）：*"Thiserror deliberately does not appear in your public API. You get the same thing as if you had written an implementation of `std::error::Error` by hand, and switching from handwritten impls to thiserror or vice versa is not a breaking change."* —— 这正是"库作者用 thiserror"的理由。

### D5.2 何时用哪个（基于上面官方定位的指导）

| 场景 | 选择 | 依据 |
| --- | --- | --- |
| 库 / 模块对外暴露的**类型化**错误，调用方需要 `match` 具体变体 | **`thiserror`** | 官方定位就是"不污染公共 API 的 Error derive"，生成 `Display`/`From`/`source()`，编译产物与手写实现等价 |
| 应用的 `main` / 命令边界（Tauri command 的顶层、初始化流程） | **`anyhow`** | 应用层只需携带上下文向上抛，不关心具体类型 |
| 需要给错误追加人类可读上下文 | `anyhow::Context`（`.context(...)` / `.with_context(...)`） | 本项为 anyhow 的既有能力，**本次未逐字抓取 anyhow 文档**，标注 **未核实** |

**Tauri 场景的实务提示**：Tauri command 的返回值必须实现 `Serialize`，而 `anyhow::Error` **不实现** `Serialize`。因此常见做法是——内部用 `anyhow` 自由传播，**在 command 边界转换成 `thiserror` 定义的、可序列化的错误类型**（或 `Result<T, String>`）。另一端 `thiserror` 定义的错误若要在前端消费，也需为其实现 `Serialize` 或做转换。**本次未核实**：Tauri 官方对 command 错误类型的推荐写法。

---

## D6. CSV 与 PDF

### D6.1 CSV：`csv` 1.4.0

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **1.4.0** | <https://crates.io/crates/csv> | 2026-09-21 |
| 发布日期 | **2025-10-17** | 同上 | 2026-09-21 |
| 许可证 | **`Unlicense/MIT`**（crates.io 的 `license` 字段原样写法）；GitHub 仓库页面显示 `Unlicense` | <https://crates.io/crates/csv> · <https://github.com/BurntSushi/rust-csv> | 2026-09-21 |
| MSRV | **1.73** | crates.io API | 2026-09-21 |
| features | **无任何 feature**（`features` 为空对象） —— serde 集成是内置的，无需开关 | 同上 | 2026-09-21 |
| 仓库活跃度 | `BurntSushi/rust-csv` 最后 push **2026-08-04**，1960 stars，**未归档**，103 open issues | GitHub REST API | 2026-09-21 |
| 累计下载 | 249,912,439 | crates.io API | 2026-09-21 |
| 依赖 | `csv-core ^0.1.11`、`itoa ^1`、`ryu ^1`、**`serde_core ^1.0.221`** | <https://docs.rs/csv/1.4.0/> | 2026-09-21 |

#### serde 集成 API（已核实）

**`WriterBuilder`**（<https://docs.rs/csv/latest/csv/struct.WriterBuilder.html>，访问日期 2026-09-21）：

- *"Builds a CSV writer with various configuration knobs. This builder can be used to tweak the field delimiter, record terminator and more. Once a CSV Writer is built, its configuration cannot be changed."*
- 方法：`WriterBuilder::new()`，以及 *"To convert a builder into a writer, call one of the methods starting with `from_`"* —— 已核实 `from_path<P: AsRef<Path>>(&self, path: P) -> Result<Writer<File>>`（*"The file is truncated if it already exists."*）与 `from_writer(...)`。
- 配置项：`delimiter`、`has_headers` 等（文档提到可关掉自动表头）。

**`Writer::serialize`**（<https://docs.rs/csv/latest/csv/struct.Writer.html>，访问日期 2026-09-21）：

```rust
pub fn serialize<S: Serialize>(&mut self, record: S) -> Result<()>
```

*"Serialize a single record using Serde."*

> **⚠️ 常见误用**：`serialize` 挂在 **`Writer`** 上，**不在 `WriterBuilder` 上**。正确链路是
> `WriterBuilder::new().from_writer(...)` / `.from_path(...)` → 拿到 `Writer` → 再调用 `writer.serialize(record)`。
> 官方示例（逐字）：
> ```rust
> let mut wtr = Writer::from_writer(vec![]);
> wtr.serialize(Row { city: "Boston", country: "United States", population: 4628910 })?;
> let data = String::from_utf8(wtr.into_inner()?)?;
> // 输出（含自动表头）: "city,country,popcount\nBoston,United States,4628910\n"
> ```
> 表头由 struct 字段名生成，可用 `#[serde(rename = "...")]` 改名；用 `WriterBuilder` 的 `has_headers` 可关闭自动表头。

**结论**：`csv` 1.4.0 成熟、活跃、无 feature 负担，`WriterBuilder` + `serialize` 的组合即题目所述方案，**已核实可行**。对 Todo 导出（含中文）无需特殊配置——CSV 是纯文本，注意 Excel 兼容性时通常加 UTF-8 BOM（**未核实**：`csv` crate 是否提供 BOM 开关，通常由调用方自行写入 BOM）。

### D6.2 PDF：`printpdf` 与 `genpdf`

#### printpdf 0.12.8

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 发布日期 | **0.12.8** / **2026-09-05** | <https://crates.io/crates/printpdf> | 2026-09-21 |
| 许可证 | **MIT** | 同上 | 2026-09-21 |
| MSRV | **1.88** | crates.io API | 2026-09-21 |
| 仓库活跃度 | <https://github.com/fschutt/printpdf>，最后 push **2026-09-05**，1116 stars，**未归档**，15 open issues | GitHub REST API | 2026-09-21 |
| **维护状态** | **活跃**。`crate.updated_at` = 2026-09-05，与仓库 push 同日；累计下载 2,959,691 | crates.io API | 2026-09-21 |
| features | `default = ["html"]`；另有 `bmp`, `dds`, `gif`, `hdr`, `html_multithreaded`, `ico`, `images`, `jpeg`, `js-sys`, `png`, `pnm`, `rayon`, `svg`, **`text_layout`**, **`text_layout_hyphenation`**, `tga`, `tiff`, `webp` | crates.io API | 2026-09-21 |

**API 风格 = 低层级**。其 `font` 模块（<https://docs.rs/printpdf/latest/printpdf/font/index.html>，访问日期 2026-09-21）暴露的是 PDF 原语：`ParsedFont`、`OwnedGlyphParsedFont`、`Font`、`PdfFont`、`SubsetFont`（"Result of subsetting a font"）、`FontEmbeddingMode`、`BuiltinFont`（"Standard built-in PDF fonts"）、`FontType`（"Distinguishes TrueType fonts from OpenType CFF fonts"）、`FontMetrics`、`PrintpdfFontMeta`，以及 CID/CFF 处理函数如 `cff_charset_gid_to_cid_map`、`extract_cid_keyed_cff`、`extract_collection_face`、`get_normalized_widths_codes`、`get_normalized_widths_ttf`、`subset_font`。可见 **0.12 已引入 `azul-layout` 做字形处理**（模块中出现 `AzulParsedFont`，描述为 "azul-layout's raw font face"）。

#### genpdf 0.2.0 —— **事实上已停止维护**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **0.2.0** | <https://crates.io/crates/genpdf> | 2026-09-21 |
| **发布日期** | **2021-06-17** | crates.io API | 2026-09-21 |
| crate `updated_at` | **2021-06-17**（即 crate 元数据自 2021 年起未再变动） | crates.io API | 2026-09-21 |
| **完整版本历史** | 仅 5 个版本：`0.1.0`(2020-10-15)、`0.1.1`(2020-10-16)、`0.2.0-alpha.0`(2021-06-05)、`0.2.0-alpha.1`(2021-06-15)、**`0.2.0`(2021-06-17)**。此后 **5 年多无任何新版本** | crates.io API | 2026-09-21 |
| 许可证 | Apache-2.0 OR MIT | 同上 | 2026-09-21 |
| MSRV | **未声明** | 同上 | 2026-09-21 |
| 仓库 | <https://git.sr.ht/~ireas/genpdf-rs>（SourceHut，非 GitHub），作者 **Robin Krahl (~ireas)** | 同上 | 2026-09-21 |
| **仓库最新提交** | SourceHut 仓库页显示最新提交为 **"5 years ago"**（条目："Use ascent for vertical text positioning" 等），refs 只有 `master` 与 `v0.2.0` | <https://git.sr.ht/~ireas/genpdf-rs> | 2026-09-21 |
| 下载 | 累计 594,243，recent_downloads 145,811（说明仍有存量使用） | crates.io API | 2026-09-21 |
| **维护状态** | **事实上已停止维护**：最后发版 2021-06-17，仓库最后提交约 5 年前 | 综合上述 | 2026-09-21 |

**API 风格 = 高层级**。官方 README（SourceHut，访问日期 2026-09-21）逐字：

> *"`genpdf` is a high-level PDF generator built on top of `printpdf` and `rusttype`. It takes care of the page layout and text alignment and renders a document tree into a PDF document. All of its dependencies are written in Rust, so you don't need any pre-installed libraries or tools."*

示例用法显示它按**字体族**加载字体：

```rust
// Load a font from the file system
let font_family = genpdf::fonts::from_files("./fonts", "LiberationSans", None)
    .expect("Failed to load font family");
let mut doc = genpdf::Document::new(font_family);
```

即 **genpdf 同样要求你把字体文件放到磁盘上**，它不内置 CJK 字体。

**结构关系确认**：`genpdf` 建立在 `printpdf` + `rusttype` 之上（**高层**排版），`printpdf` 是**低层** PDF 原语库。题目所述关系成立。

#### 其他可验证的 PDF 途径

| 方案 | 版本 / 状态 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- |
| `typst`（作为 Rust crate / 编译器） | **0.15.1**，发布 2026-07-17 | Apache-2.0 | <https://crates.io/crates/typst> · <https://github.com/typst/typst> | 2026-09-21 |
| typst 仓库活跃度 | 最后 push **2026-09-22**，**56,188 stars**，未归档（**极活跃**） | — | GitHub REST API | 2026-09-21 |
| `wkhtmltopdf` | **仓库已归档（`archived: true`）**，最后 push **2022-11-22**，14,557 stars | LGPL-3.0 | <https://github.com/wkhtmltopdf/wkhtmltopdf> | 2026-09-21 |
| `puppeteer`（headless Chrome） | npm latest **25.11.0** | Apache-2.0 | <https://registry.npmjs.org/puppeteer/latest> | 2026-09-21 |
| `jspdf`（前端生成） | npm latest **4.2.1** | MIT | <https://registry.npmjs.org/jspdf/latest> | 2026-09-21 |
| `pdf-lib`（前端生成/编辑） | npm latest **1.17.1** | MIT | <https://registry.npmjs.org/pdf-lib/latest> | 2026-09-21 |

- **typst**：活跃度极高（56k stars，最后 push 2026-09-22），可作为 Rust 侧的高质量排版引擎，其自身对 CJK 的支持成熟。但它是一个**完整的排版系统/编译器**，作为库集成进 Tauri 的复杂度与体积远高于 `printpdf`/`genpdf`。**未核实**：`typst` 作为 library 嵌入的具体 API 与二进制体积影响，本次未调研。
- **wkhtmltopdf**：**已归档、停止维护**，不建议新项目采用。
- **headless Chrome / `puppeteer`**：在 Tauri 场景下通常**不需要额外引入**——Windows 上 Tauri 2 的 WebView 本身就是 Chromium 内核（WebView2），直接用它打印即可（见 §D6.3）。

### D6.3 推荐：Tauri 桌面应用应在前端还是 Rust 生成 PDF？

**推荐：优先在 FRONTEND 生成（浏览器打印 / 前端 PDF 库）；Rust 侧生成仅作为无界面/批量导出的备选。**

**依据（全部基于本次已验证的事实）**：

1. **中文渲染的核心难点在字体，而浏览器已经拥有字体**
   - `printpdf` 的**内置字体无法输出非 ASCII**。已核实的一手证据：issue **#273 "Built-in fonts emit UTF-8 bytes into a WinAnsiEncoding content stream (all non-ASCII text is mojibake'd)"**（已关闭，2026-06-30），修复 PR #274 *"fix(serialize): encode built-in font text as WinAnsi, not raw UTF-8"*（2026-07-01）。结论：`BuiltinFont` 走 **WinAnsi 编码**，中文必然乱码，必须**自带并嵌入 TTF/OTF 中文字体**。
   - `genpdf` 同样要求显式提供字体目录（`fonts::from_files(...)`），**不内置 CJK 字体**。
   - 反过来，**前端渲染走的是系统字体栈**，Windows 自带中文字体（Microsoft YaHei 等），CJK 开箱即用，**零字体工程**。

2. **printpdf 的 CJK 子集化/字形解析历史上反复出问题**（GitHub issue 一手证据）
   - #212 [closed] *"NotoCJK font does not parse glyphs from GLYF table"*（2025-03-20）
   - #222 [closed] *"Subsetting and shaping on its own worked in 0.7, but does not work in 0.8.2"*（2025-05-15）
   - #250 [closed] *"PdfSaveOption subset_fonts is ignored"*（2025-11-07）
   - **#283 [OPEN]** *"examples: add multiple_fonts POC — single CJK OTC, per-language glyph variants, subsetting"*（2026-07-24）—— 截至访问日仍是 **open** 状态，说明 CJK 多字体/子集化的示例级支持仍在推进中。
   - 若在 Rust 侧做，需自行承担"嵌入完整中文字体（单文件通常十几 MB）→ 必须做子集化 → 子集化路径本身有历史 bug"这条链路。

3. **Rust 侧 PDF 库的维护面**
   - `printpdf` 0.12.8 **活跃**（2026-09-05 发布，仓库同日 push），**是 Rust 侧唯一值得选的低层方案**，但 API 低层、MSRV 1.88、需要自管字体。
   - `genpdf` 0.2.0 **事实上已停止维护**（2021-06-17 最后发版，仓库最后提交约 5 年前）—— **不建议用于新项目**。
   - `wkhtmltopdf` **已归档**，排除。

4. **前端方案的额外好处**
   - 无额外 Rust 依赖、无 MSRV 抬升、无二进制体积膨胀。
   - 复用已有的 HTML/CSS 模板做排版，与界面样式同源，维护成本最低。
   - 需要程序化生成（非用户手动打印）时，可用 `pdf-lib` 1.17.1（MIT）或 `jspdf` 4.2.1（MIT）。

**需要注意的边界**：
- **未核实**：Tauri 2 在 Windows WebView2 上 `window.print()` 的确切行为与可用性（是否会弹出打印对话框、能否直接导出 PDF、静默打印是否需要额外权限）。**这一条应在实现前做最小验证**——它是本推荐的关键前提。
- 若产品需要**无界面批量导出**（例如定时备份成 PDF、后台任务），前端方案不适用，此时再用 `printpdf` 并**预先准备好可嵌入的中文字体（含子集化）**。

**结论一句话**：题目提到的"CJK 字体嵌入难度 vs 浏览器已有 CJK 字体"这个判断，**有充分的一手证据支持** —— `printpdf` 内置字体对非 ASCII 直接乱码、自带字体 + 子集化是必须且历史上多 bug 的路径，而浏览器侧零配置。**因此推荐前端生成**，并把 Rust 侧生成降级为备选。

---

## D7. 对本项目 `src-tauri/Cargo.toml` 的直接影响（高优先级）

调研过程中核对工作区现存配置（`D:\Todo\src-tauri\Cargo.toml`，读取日期 2026-09-21），发现**两处必然导致编译失败**的问题与若干建议项：

| # | 严重度 | 现状（行号） | 问题 | 依据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| 1 | **P0 编译失败** | 第 64 行 `keyring = { version = "4", features = ["windows-native"] }` | **`windows-native` feature 在 keyring 4.x 中不存在**（v4 只有 `v1` / `cli`），Cargo 会报 feature 不存在 | §D3.1（crates.io features 字段） | 改为 `keyring = "4"`（用默认 `v1`），或走 `keyring-core` + `windows-native-keyring-store` 路线 |
| 2 | **P0 编译失败** | 第 68 行 `features = [..., "rustls-tls", ...]` | **`rustls-tls` 在 reqwest 0.13 已重命名为 `rustls`** | §D4.2（官方 CHANGELOG v0.13.0）+ crates.io features | 改为 `"rustls"` |
| 3 | **P1 功能缺失** | 第 66-71 行 reqwest 用了 `default-features = false` | 同时失去 **`system-proxy`**（系统代理）、`http2`、`charset` —— 中国用户依赖系统代理访问境外 API 时会**静默直连失败** | §D4.3（`default = ["default-tls","charset","http2","system-proxy"]`） | 要么去掉 `default-features = false`（0.13 默认已是 rustls，无需额外指定 TLS），要么显式补 `"http2", "system-proxy"` |
| 4 | **P1 工具链不自洽** | 第 7 行 `rust-version = "1.77"` | `sqlx 0.9.0` 要求 MSRV **1.94.0**；`keyring 4.2.0`/`printpdf 0.12.8` 要求 1.88；`reqwest 0.13.5`/`uuid 1.26.1` 要求 1.85 | §0 MSRV 汇总 | 上调为 `rust-version = "1.94"` |
| 5 | **P1 运行时风险** | 第 45-53 行 sqlx `default-features = false`，features 未含 `any` | `PoolOptions`（即 `SqlitePoolOptions`）的 `max_connections` 等与 `Pool::begin` / `try_begin` 均标注 **`Available on crate feature any only`** | §D1.6 | 在 sqlx features 中补 **`"any"`**（否则连事务 `begin()` 都用不了） |
| 6 | P2 建议 | 第 42 行 tokio features 缺 `fs` | 若 Rust 侧需异步文件读写（备份、导出）需 `fs` | §D2.5 | 按需补 `"fs"` |
| 7 | P2 建议 | 迁移目录 | Windows 上 CRLF 会导致迁移哈希跨平台不可复现 | §D1.2（官方文档） | 加 `.gitattributes`：`*.sql text eol=lf`；并加 `build.rs` 打印 `cargo:rerun-if-changed=migrations` |
| 8 | P2 建议 | 第 63 行 keyring 用途注释 | Windows 后端是 **Windows Credential Manager**；注释里写的 "DPAPI" 需确认是否有官方依据 | §D3.3 | **未核实** DPAPI 是否为官方描述用词；官方文档只写 "Windows Credential Manager"。建议注释与官方措辞对齐 |

> 第 1、2 项是**确定性编译错误**，应在下次构建前修正。第 3、5 项是**静默功能缺失**，比编译错误更隐蔽。

---

## 附：未核实清单（明确不作推断的项）

| # | 未核实项 | 原因 |
| --- | --- | --- |
| 1 | `sqlx.toml` 的完整字段清单与 schema 定义 | 仅确认了官方文档列举的四类能力，未逐字抓取 configuration guide |
| 2 | `chrono-tz` 针对 Windows 的官方专属注意事项 | 官方 README/docs.rs 未提及；§D2.2 中的 Windows 分析已标注为分析性判断 |
| 3 | `rrule` 是否通过 RFC 5545 官方/第三方一致性测试套件 | 官方仅自我声明"follows RFC-5545"，未找到测试报告 |
| 4 | `rrule` 与 `rrule.js` 的逐特性完整度矩阵 | 本次只验证了 `rrule` 侧；rrule.js 的 `BYSETPOS`/`Nth BYDAY` 未逐项核实 |
| 5 | `rrule` 的 `EXDATE`/`RDATE` 边界行为、`DTSTART` 内 `TZID` 解析细节 | 未做行为测试 |
| 6 | rrule.js 的 GitHub commit 活动与 issue 响应情况 | 仅用 npm registry 一手数据（发布停滞已确证） |
| 7 | Tauri 2.11.6 具体启用 tokio 哪些 feature | 未解析 tauri 的 Cargo.toml；故无法断言项目侧 tokio 写作"最小集" |
| 8 | `anyhow::Context` 的逐字 API | 未抓取 anyhow 文档（其能力为已知常识，但未按本次方法逐字核实） |
| 9 | Tauri command 错误类型的官方推荐写法 | 未调研 Tauri 官方文档 |
| 10 | `tauri-plugin-stronghold` 是否适配"Windows 凭据存储"用例 | 未调研其文档 |
| 11 | **Tauri 2 / WebView2 上 `window.print()` 的确切行为** | §D6.3 推荐的关键前提，需实现前最小验证 |
| 12 | `typst` 作为 library 嵌入的 API 与体积影响 | 未调研 |
| 13 | `csv` 是否提供 UTF-8 BOM 开关（Excel 兼容） | 未核实，通常由调用方自行写 BOM |
| 14 | `keyring` v4 的 `Migration`/升级指南（v3→v4 代码改法） | 仓库根目录无 `CHANGELOG.md`（已列举根目录文件确认）；docs.rs 已给出模式说明，但未找到逐条迁移指南 |
| 15 | keyring 注释中 "DPAPI" 说法的官方依据 | 官方文档用词为 "Windows Credential Manager" |

---

## 一句话速查

| 主题 | 结论 |
| --- | --- |
| sqlx 0.9.0 | MIT OR Apache-2.0，**MSRV 1.94.0**；`migrate!` 需 `macros`+`migrate`，默认 `./migrations`（相对 `Cargo.toml` 所在根）；文件名 `<VERSION>_<DESCRIPTION>.sql`；**`sqlite` feature 默认捆绑静态 SQLite**（需 C 构建工具） |
| reqwest 0.13.5 | **默认 TLS 已是 rustls（非 SChannel）**；`rustls-tls` **已改名 `rustls`**；`default` 含 `system-proxy` |
| keyring 4.2.0 | 只剩 **`v1`（默认）/ `cli`** 两个 feature；**旧 `windows-native` 已移除**；Windows 用 Credential Manager；要控制 store 就用 `keyring-core` + `windows-native-keyring-store` |
| tauri-plugin-keyring | **第三方个人插件**（owner HuakunShen），不在官方 plugins-workspace，依赖 `keyring ^3.6.1`，已 2 年未更新 |
| rrule 0.14.0 | 支持 **`BYSETPOS`**（`by_set_pos`）与 **数字前缀 `BYDAY`**（`NWeekday::Nth(i16, Weekday)`）；但**上游 2025-04 后停更**；rrule.js 更早停（**2023-11-10**） |
| chrono-tz 0.10.4 | build script 静态生成 IANA 表；**"Dynamic tzdata loading" 仍是官方 Future Improvement** ⇒ 不读 OS 时区库 |
| PDF | **推荐前端生成**（浏览器已有 CJK 字体）；`printpdf` 内置字体对非 ASCII 乱码（issue #273），中文必须自带并嵌入字体 + 子集化（#283 仍 open）；**`genpdf` 已 5 年未发版**；`wkhtmltopdf` 已归档 |
| CSV | `csv` **1.4.0**（Unlicense/MIT，无 feature）；`WriterBuilder` → `Writer`，**`serialize` 在 `Writer` 上** |
