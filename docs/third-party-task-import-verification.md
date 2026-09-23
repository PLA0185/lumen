# 第三方任务平台官方导出格式 / 官方 API 核实报告

**核实对象**：Todoist、Google Tasks、Notion  
**用途**：Windows 桌面 Todo 应用（Tauri 2 + React + TS + SQLite）导入用户任务  
**访问日期**：2026-09-21（按任务要求统一标注。说明：本机系统时钟为 2026-09-23 11:15 UTC+8，实际 HTTP 抓取发生在该时刻，官方页面均在本次会话内实时抓取）

**核实方法**：直接 `Invoke-WebRequest` 抓取官方文档 URL（未使用搜索引擎首页结果作为依据）。`developer.todoist.com` 与 `developers.notion.com` 为 JS/SPA 渲染，采用「反转义 Redoc 内嵌规格 + Mintlify `.md` 直出版本」两种方式取正文。凡未能从官方来源确认的条目一律写 **未核实**。

---

## 0. 总览表

| 库/服务 | 确切版本或结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| Todoist API | **Todoist API v1**，base URL `https://api.todoist.com/api/v1`；官方原文「The Todoist API v1 is a new API that unifies the Sync API v9 and the REST API v2」 | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| Todoist 旧版文档 | `/rest/v2/`、`/sync/v9/`、`/guides/` 与 `/api/v1/` 返回**字节完全相同**的 HTML（均 4167217 bytes，SHA-256 前 32 位 `9A7DC3A8CEA9E6F24A5E09742B7BAB33`），即旧版独立文档实际已由 v1 文档取代 | https://developer.todoist.com/rest/v2/ · https://developer.todoist.com/sync/v9/ | 2026-09-21 |
| Todoist OAuth | authorization code flow（**无 PKCE**）：授权 `https://app.todoist.com/oauth/authorize`，换 token `https://api.todoist.com/oauth/access_token`（POST，`client_id` + `client_secret` + `code`） | https://developer.todoist.com/api/v1/ （tag: Authorization） | 2026-09-21 |
| Todoist 个人 API 令牌 | **仍可用且文档中未见弃用声明**；官方原文「you can obtain your personal API token from the integrations settings」；Backups 端点明确接受「the `backups:read` scope, the `data:read_write` scope, or a personal API token」 | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| Todoist CSV 导入模板 | **有官方文档化模板格式**，官方模板下载链接 `https://get.todoist.help/hc/article_attachments/21672045045916` | https://www.todoist.com/help/articles/import-or-export-a-project-as-a-csv-file-in-todoist-YC8YvN | 2026-09-21 |
| Todoist 备份 | 用户级：Settings → Backups（**Pro/Business**），**ZIP** 内为**每个活动项目一个 CSV**，最多 21 份；API：`GET /api/v1/backups`、`GET /api/v1/backups/download?file=<...>.zip` | https://www.todoist.com/help/articles/download-or-restore-backups-in-todoist-ywaJeQbN | 2026-09-21 |
| Google Tasks API | **Google Tasks API v1**，base `https://tasks.googleapis.com/tasks/v1` | https://developers.google.com/workspace/tasks/reference/rest/v1/tasks/list | 2026-09-21 |
| Google Tasks scope | `https://www.googleapis.com/auth/tasks`、`https://www.googleapis.com/auth/tasks.readonly` | https://developers.google.com/workspace/tasks/auth | 2026-09-21 |
| Google 桌面端 OAuth | client type 选 **Desktop app**；`redirect_uri` 用 **loopback IP** `http://127.0.0.1:port` 或 `http://[::1]:port`；**OOB（手动复制粘贴）已不再支持** | https://developers.google.com/identity/protocols/oauth2/native-app | 2026-09-21 |
| Google OOB 合规时间线 | 2022-02-28 阻止新用法；2022-09-05 用户警告；2022-10-03 对 2022-02-28 前创建的客户端弃用；**2023-01-31 所有既有客户端被阻止** | https://developers.google.com/identity/protocols/oauth2/resources/oob-migration | 2026-09-21 |
| Google Tasks 导出 | Google Takeout 支持 Tasks；归档文件类型为 **Zip 或 Tgz** | https://support.google.com/tasks/answer/10017961 · https://support.google.com/accounts/answer/3024190 | 2026-09-21 |
| Notion `Notion-Version` | **`2026-03-11`**（最新）——由 `const latestApiVersion = <code>2026-03-11</code>` 与该页 cURL 示例 `Notion-Version: 2026-03-11` 双重确认 | https://developers.notion.com/reference/versioning | 2026-09-21 |
| Notion 版本历史 | `2026-03-11`、`2025-09-03`、`2022-06-28`、`2022-02-22`、`2021-08-16`、`2021-05-13` | https://developers.notion.com/reference/changes-by-version | 2026-09-21 |
| Notion 查询数据库新端点 | `POST https://api.notion.com/v1/data_sources/{data_source_id}/query`；旧 `POST /v1/databases/{database_id}/query` **自 `2025-09-03` 起弃用** | https://developers.notion.com/reference/query-a-data-source · https://developers.notion.com/reference/post-database-query | 2026-09-21 |
| Notion 认证 | Bearer token；三种来源：internal connection（静态 installation token）、personal access token（用户级）、public connection（OAuth 2.0）；**public connection 未文档化 PKCE** | https://developers.notion.com/reference/authentication · https://developers.notion.com/guides/get-started/authorization | 2026-09-21 |
| Notion 导出 | 用户级 Export：**PDF / HTML / Markdown & CSV（ZIP）**；整库导出 `Export all workspace content`；**标准 REST API 无整库导出端点** | https://www.notion.com/help/export-your-content | 2026-09-21 |

---

## A. Todoist

### A1. 当前官方 API 版本与 base URL

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 当前 API 名称/版本 | **Todoist API v1**（Redoc 规格版本徽标显示 `Todoist API ( 1 )`） | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| 当前 base URL | `https://api.todoist.com/api/v1` | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| 与旧版关系 | v1 **统一了** Sync API v9 与 REST API v2；v1 文档「Migrating from v9」章节原文：「The Todoist API v1 is a new API that unifies the Sync API v9 and the REST API v2.」 | https://developer.todoist.com/api/v1/#tag/Migrating-from-v9 | 2026-09-21 |
| 旧 base URL `api.todoist.com/rest/v2` | 文档仅作为历史参考提及；v1 明确「After Todoist API v1, we will only focus on `api.todoist.com` as the subdomain」 | 同上 | 2026-09-21 |
| 旧文档站点可用性 | `https://developer.todoist.com/rest/v2/` 与 `https://developer.todoist.com/sync/v9/` 当前返回与 `/api/v1/` **完全相同的 HTML**（4167217 bytes，SHA-256 `9A7DC3A8CEA9E6F24A5E09742B7BAB33`），即旧版独立参考文档实际已不再单独提供 | https://developer.todoist.com/rest/v2/ | 2026-09-21 |

### A2. OAuth 2.0 流程与端点（逐字取自官方规格）

| 环节 | 确切值 | 官方 URL | 访问日期 |
|---|---|---|---|
| 授权端点 | `https://app.todoist.com/oauth/authorize` | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| 授权必填参数 | `client_id`、`scope`、`state` | 同上 | 2026-09-21 |
| 授权可选参数 | `redirect_uri`（应用配置了多个 redirect URI 时**必填**，缺失则 `invalid_request`）、`response_type`（可省略或 `code`；其他值返回 `unsupported_response_type`） | 同上 | 2026-09-21 |
| 令牌端点 | `https://api.todoist.com/oauth/access_token`（POST） | 同上 | 2026-09-21 |
| 令牌必填参数 | `client_id`、`client_secret`、`code` | 同上 | 2026-09-21 |
| 令牌响应 | `access_token`、`token_type: "Bearer"`；开启 refresh token 的新应用（**新建应用默认开启**）额外返回 `expires_in: 3600` 与 `refresh_token`、`scope`；未开启的旧应用返回 `expires_in: 315360000`（10 年兼容值）且**无** `refresh_token` | 同上 | 2026-09-21 |
| 可用 scope | `task:add`、`data:read`、`data:read_write`、`data:delete`、`project:delete`、`backups:read` | 同上 | 2026-09-21 |
| **PKCE** | **未文档化**（规格中仅有 `client_secret` 机密客户端流程，无 `code_challenge` / `code_verifier`） | 同上 | 2026-09-21 |
| 应用注册 | 需在 **App Management Console** 创建应用，获得 `Client ID` 与 `Client Secret`，并「configure one or more valid OAuth2 redirect URLs」 | 同上 | 2026-09-21 |
| redirect URL 限制细则 | 官方仅要求「configure one or more valid OAuth2 redirect URLs」，**未记载**是否允许 `http://127.0.0.1` / `localhost` 回环地址、自定义 scheme、是否强制 HTTPS、是否有数量上限 → **未核实** | 同上 | 2026-09-21 |

### A3. 个人 API 令牌是否仍可用 / 是否弃用

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 个人 API 令牌（Settings → Integrations） | **仍可用**。原文：「you can obtain your personal API token from the integrations settings for your account」 | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| 是否弃用 | 在 v1 规格全文检索 `personal API token` 与 `deprecat`，**未见任何针对个人 API 令牌的弃用声明**；`GET /api/v1/backups` 明确接受「…or a personal API token」 | 同上 | 2026-09-21 |
| `Migrate Personal Token` 端点真实语义 | `POST /api/v1/access_tokens/migrate_personal_token`，请求体 `client_id`(必填)、`client_secret`(必填)、`personal_token`(必填)、`scope`(必填)。官方描述：「Tokens obtained via the **old email/password authentication method** can be migrated to the new OAuth access token.」→ 迁移的是**旧的邮箱/密码认证方式**产生的令牌，**不是** Integrations 设置里的个人 API 令牌 | 同上 | 2026-09-21 |

### A4. 官方 CSV 模板 / 导出与备份格式

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| CSV 模板是否官方文档化 | **是**，官方帮助文档给出完整列定义与模板下载 | https://www.todoist.com/help/articles/import-or-export-a-project-as-a-csv-file-in-todoist-YC8YvN | 2026-09-21 |
| 官方模板下载链接 | `https://get.todoist.help/hc/article_attachments/21672045045916`（链接取自官方文章正文；该主机在本环境不可达，**文件内容未直接验证**） | 同上 | 2026-09-21 |
| CSV 列（逐字） | `TYPE`、`CONTENT`、`DESCRIPTION`、`PRIORITY`、`INDENT`、`AUTHOR`、`RESPONSIBLE`、`DATE`、`DATE_LANG`、`TIMEZONE`、`DURATION`、`DURATION_UNIT`、`meta`、`DEADLINE`、`DEADLINE_LANG`、`IS_COLLAPSED` | 同上 | 2026-09-21 |
| 关键列取值 | `TYPE` ∈ `task`/`section`/`note`（**区分大小写，必须小写**）；`PRIORITY` ∈ `1`/`2`/`3`/`4`（留空默认 `p1`）；`INDENT` 1..4；`IS_COLLAPSED` `TRUE`/`FALSE`（仅 section，可省略）；`meta` 例 `view_style=board` | 同上 | 2026-09-21 |
| 编码要求 | **必须 UTF-8** | 同上 | 2026-09-21 |
| 导入限制 | 每个项目**最多 300 个任务**，超出无法完成导入，需分片 | 同上 | 2026-09-21 |
| 用户级 CSV 导出 | Manage data → **Export as CSV**；**不包含已完成任务**；导出文件不含 labels、deadlines、comments、attachments、reminders；循环日期不保存起始日期 | 同上 | 2026-09-21 |
| 用户级备份格式 | Settings → Backups（**Pro/Business**）；「Up to 21 backups are stored as **ZIP files**」，「Your projects are backed up as **CSV files**」；每日 00:00 本地时区生成；含活动项目与任务、日期时间、描述、时长、deadline、循环日期（不含起始日）、每任务最多 500 条评论、附件链接；**不含已完成任务与已归档项目** | https://www.todoist.com/help/articles/download-or-restore-backups-in-todoist-ywaJeQbN | 2026-09-21 |
| API 备份端点 | `GET /api/v1/backups`（接受 `backups:read`、`data:read_write` 或个人 API token；`backups:read` 绕过 MFA，启用 MFA 的账号用 `data:read_write`/令牌时需 `mfa_token`）；`GET /api/v1/backups/download?file=<...>.zip`（需 `data:read_write`，校验归属后 302 到 **1 分钟**过期的签名 CloudFront URL） | https://developer.todoist.com/api/v1/ | 2026-09-21 |
| 项目模板导出端点 | `GET /api/v1/templates/file?project_id=...&use_relative_dates=...` → 「Get a template for a project as a **CSV file**」；`GET /api/v1/templates/url` → 可分享模板 URL | 同上 | 2026-09-21 |
| 备份 ZIP 内部 JSON schema | **未核实**（官方只说明 ZIP 内为每项目一个 CSV） | — | 2026-09-21 |

### A5. Todoist 两个必答问题

1. **是否可在桌面应用内通过用户自建 OAuth 应用访问？**  
   **可以（有条件）**。开发者可在 App Management Console 注册自有应用，走 **authorization code flow**：浏览器打开 `https://app.todoist.com/oauth/authorize` → 回调拿 `code` → `POST https://api.todoist.com/oauth/access_token` 换取 access/refresh token。  
   **注意**：官方文档**只提供机密客户端流程**（必须携带 `client_secret`），**未文档化 PKCE**；因此桌面应用要么内置 `client_secret`（无法真正保密），要么另想办法。**redirect URL 是否允许 `http://127.0.0.1:port` 回环或自定义 scheme，官方文档未记载 → 未核实**，这是本项目落地前必须实机验证的第一风险点。
2. **是否有官方导出文件（CSV/JSON/ZIP）作为无 OAuth 的降级路径？**  
   **有**。①每项目 **CSV** 导出（Manage data → Export as CSV，格式与官方导入模板同源，可直接回导）；②**Pro/Business** 的 Settings → Backups 提供 **ZIP（内含每项目一个 CSV）**。但两者均为**用户手动导出**，不是静默 API；无需 OAuth 的自动化读取路径只有**用户手工填写的个人 API 令牌**（官方仍支持）。

---

## B. Google Tasks

### B1. API v1、scope、端点

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| API 版本 | **Google Tasks API v1** | https://developers.google.com/workspace/tasks/overview | 2026-09-21 |
| base URL | `https://tasks.googleapis.com/tasks/v1` | https://developers.google.com/workspace/tasks/reference/rest/v1/tasks/list | 2026-09-21 |
| scope | `https://www.googleapis.com/auth/tasks`（读+写全部任务）、`https://www.googleapis.com/auth/tasks.readonly`（只读） | https://developers.google.com/workspace/tasks/auth | 2026-09-21 |
| 列出任务 | `GET https://tasks.googleapis.com/tasks/v1/lists/{tasklist}/tasks`；查询参数 `completedMax`、`completedMin`、`dueMax`、`dueMin`、`maxResults`(默认 20，最大 100)、`pageToken`、`showCompleted`、`showDeleted`、`showHidden`、`updatedMin`、`showAssigned` | https://developers.google.com/workspace/tasks/reference/rest/v1/tasks/list | 2026-09-21 |
| 列出任务清单 | `GET https://tasks.googleapis.com/tasks/v1/users/@me/lists`；`maxResults` 默认 1000（最大 1000） | https://developers.google.com/workspace/tasks/reference/rest/v1/tasks | 2026-09-21 |
| 配额/容量 | 每个 list 最多 20,000 个非隐藏任务，账号总计 100,000 个任务；最多 2000 个 list | 同上 | 2026-09-21 |
| 页面新鲜度 | `auth` 与 `overview` 页均显示「Last updated 2026-09-03 UTC」 | https://developers.google.com/workspace/tasks/auth | 2026-09-21 |

### B2. 桌面应用 OAuth（loopback）与 OOB 政策

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 能否自建 OAuth 客户端 | **可以**。需在 Google Cloud Console 启用 API 并创建凭据；client type 选 **Desktop app**，「Recommended usage: macOS, Linux, and Windows desktop (but not Universal Windows Platform) apps」 | https://developers.google.com/identity/protocols/oauth2/native-app | 2026-09-21 |
| 桌面端 redirect_uri | **Loopback IP address**：`http://127.0.0.1:port` 或 `http://[::1]:port`，可「start an HTTP listener on a random available port」；也可用 `localhost` 替代回环 IP，但「this configuration may cause issues with client firewalls」 | 同上 | 2026-09-21 |
| 自定义 scheme | 「Custom URI schemes are **no longer supported** due to the risk of app impersonation」（Android/Chrome app 语境明确；桌面端官方推荐 loopback） | 同上 | 2026-09-21 |
| PKCE | 官方桌面/installed app 流程内含「generating a code verifier and challenge」步骤 | 同上 | 2026-09-21 |
| loopback 对移动端的弃用 | 「support for the loopback IP address redirect option on **mobile apps** is DEPRECATED」；另注「The loopback IP address redirect option is DEPRECATED for **Android, Chrome app and iOS** OAuth client types」→ **Windows 桌面不受此弃用影响** | 同上 | 2026-09-21 |
| OOB（out-of-band / 手动复制粘贴） | 「The manual copy/paste option, also referred to as an out of band (OOB) redirect method, is **no longer supported**」；错误码 `invalid_request` 中亦提示 OOB「has been deprecated and is no longer supported」 | 同上 | 2026-09-21 |
| OOB 关键合规日期 | 2022-02-28 新 OAuth 用法被阻止；2022-09-05 可向不合规请求显示用户警告；2022-10-03 对 2022-02-28 之前创建的客户端弃用；**2023-01-31 所有既有客户端被阻止**（含此前豁免者）；「desktop clients to the loopback IP address flow」 | https://developers.google.com/identity/protocols/oauth2/resources/oob-migration | 2026-09-21 |
| 页面新鲜度 | 桌面端 OAuth 页「Last updated 2026-09-14 UTC」 | https://developers.google.com/identity/protocols/oauth2/native-app | 2026-09-21 |

### B3. 官方导出（Google Takeout）

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| Tasks 是否支持导出 | **支持**。「Export your data from Google Tasks — You can export and download your data from Google Tasks. You can't export deleted user data.」 | https://support.google.com/tasks/answer/10017961 | 2026-09-21 |
| 导出包含字段（官方清单） | Assignee emails、Assigner emails、Associated task recurrence、Completed timestamps、Creation timestamps、Creator emails、Descriptions、Due dates、IDs、Last updated timestamps、Link descriptions、Link types、Links、List IDs、List titles、Parent IDs、Schedule of task recurrences、Sort order of task lists (for web)、Source IDs、Source names、Starring timestamps、Task types、Titles、Visibility of task lists in the fullscreen web view、Whether task was completed or needs action、Whether the task was starred、Whether the task was created with the help of AI features、Whether the task recurrence was stopped、Whether a task is an active instance of a task recurrence | 同上 | 2026-09-21 |
| 归档文件类型 | **Zip files** 或 **Tgz files**（Takeout「File type」选项；Tgz 在 Windows 上可能需额外软件） | https://support.google.com/accounts/answer/3024190 | 2026-09-21 |
| 交付方式 | 邮件下载链接 / Add to Drive / Add to Dropbox / Add to Microsoft OneDrive / Add to Box；可设「Archive size」上限，超出会生成多个归档 | 同上 | 2026-09-21 |
| Tasks 导出文件的确切文件名/扩展名（如 `Tasks.json`） | **未核实**：官方 Tasks 帮助页只列字段，未写文件格式；Takeout 产品清单页 `https://takeout.google.com/` 需登录，无法匿名读取 | — | 2026-09-21 |
| 是否有官方 Tasks 导入功能 | 官方未提供导入（帮助页只有导出）；**未核实**是否有其他官方导入途径 | https://support.google.com/tasks/answer/10017961 | 2026-09-21 |

### B4. Google Tasks 两个必答问题

1. **是否可在桌面应用内通过用户自建 OAuth 应用访问？**  
   **可以，且路径最清晰**。流程为 **authorization code + PKCE + loopback redirect**：在 Google Cloud Console 创建 **Desktop app** 类型 OAuth 客户端（自有 client，可用自有 consent screen），本机监听 `http://127.0.0.1:<随机端口>` 接收 `code`，再用 `code_verifier` 换 token。**不要**使用 OOB / 手动复制粘贴（自 2023-01-31 起所有客户端被阻止）。
2. **是否有官方导出文件（CSV/JSON/ZIP）作为无 OAuth 的降级路径？**  
   **有，但为人工路径**：Google Takeout 支持导出 Tasks，归档为 **Zip/Tgz**。文档未给出 Tasks 数据文件的确切格式名（很可能为 JSON，**未核实**），因此导入器应做成「解压后扫描归档目录、按内容特征识别」而非硬编码单一文件名。

---

## C. Notion

### C1. `Notion-Version` 最新版本号

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 最新 `Notion-Version` | **`2026-03-11`** | https://developers.notion.com/reference/versioning | 2026-09-21 |
| 证据 1（权威变量） | 页面编译产物中 `const latestApiVersion=<code>2026-03-11</code>`，正文两处引用该变量：「our latest version is {latestApiVersion}」「The most recent `Notion-Version` is {latestApiVersion}」 | 同上 | 2026-09-21 |
| 证据 2（示例） | 同页 cURL 示例 `-H "Notion-Version: 2026-03-11"`；JS SDK 注释 `notionVersion: "2026-03-11"` | 同上 | 2026-09-21 |
| 证据 3（第三方页交叉确认） | Authentication 页示例亦为 `-H "Notion-Version: 2026-03-11"` | https://developers.notion.com/reference/authentication | 2026-09-21 |
| 证据 4（版本清单） | 「Changes by version」列出 `2026-03-11`、`2025-09-03`、`2022-06-28`、`2022-02-22`、`2021-08-16`、`2021-05-13`，`2026-03-11` 为最新 | https://developers.notion.com/reference/changes-by-version | 2026-09-21 |
| 易混淆项（非 API 版本） | `Notion-Beta: notion-as-code-2026-07-31` 是**beta 功能开关**，不是 `Notion-Version`；页面上出现的 `2026-09-22`/`2026-07-29` 是页面的 `dateModified` 元数据 | https://developers.notion.com/reference/versioning | 2026-09-21 |
| 版本管理语义 | 「Versioning is only for backwards incompatible changes」；**新增字段/端点等向后兼容变更对所有版本同时生效**，pin 版本不能延迟它们 | 同上 | 2026-09-21 |

### C2. 数据库查询端点弃用与新端点

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 旧端点状态 | `POST /v1/databases/{database_id}/query` **已弃用**：「**Deprecated as of version 2025-09-03** — This page describes the API for versions up to and including `2022-06-28`. In the new `2025-09-03` version, the concepts of databases and data sources were split up」 | https://developers.notion.com/reference/post-database-query | 2026-09-21 |
| 新端点 | **`POST https://api.notion.com/v1/data_sources/{data_source_id}/query`** | https://developers.notion.com/reference/query-a-data-source | 2026-09-21 |
| 新端点带参示例（逐字） | `https://api.notion.com/v1/data_sources/[DATA_SOURCE_ID]/query?filter_properties[]=title` / `...&filter_properties[]=status` | 同上 | 2026-09-21 |
| 导航结构 | 文档侧栏现为 `Databases`、`Data sources`，并有单独的 **`Databases (deprecated)`** 分组 | https://developers.notion.com/reference/query-a-data-source | 2026-09-21 |
| 权限前提 | 查询前必须把 data source/database「Add connections」共享给连接，否则返回 **404**；缺少 read content capability 返回 **403** | 同上 | 2026-09-21 |
| Wiki 特例 | Wiki 的 data source 子项可能是 page 或 database；返回 database 子项时返回其 data sources 而非直接结果，可用 `result_type` 过滤 `"page"` / `"data_source"` | 同上 | 2026-09-21 |

### C3. 认证模型与自建 OAuth 集成

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 令牌载体 | 所有请求使用 `Authorization: Bearer <token>`，且 `Notion-Version` 头**必填** | https://developers.notion.com/reference/authentication | 2026-09-21 |
| 令牌三种来源 | ① **internal connection**（工作区级静态 installation token，以 bot 身份操作，需手动把页面 Add connections）；② **personal access token**（用户级静态令牌，以创建者身份操作）；③ **public connection**（OAuth 2.0，按授权用户身份操作，使用 OAuth page picker 选页面） | https://developers.notion.com/reference/authentication · https://developers.notion.com/guides/get-started/public-connections | 2026-09-21 |
| 能否自建 OAuth 集成 | **可以**：在 Developer portal `https://app.notion.com/developers/connections` 创建 **public connection**，填写名称、开发工作区、**Redirect URI(s)**、installation scope（`Any workspace` / `Selected workspaces only`，**创建后不可更改**）、capabilities | https://developers.notion.com/guides/get-started/public-connections | 2026-09-21 |
| 授权端点 | `https://api.notion.com/v1/oauth/authorize?owner=user&client_id=<...>&redirect_uri=<...>&response_type=code`；参数 `client_id`(必)、`redirect_uri`(必)、`response_type`(必，恒为 `code`)、`owner`(必，恒为 `user`)、`state`(选) | https://developers.notion.com/guides/get-started/authorization | 2026-09-21 |
| 令牌端点 | `POST https://api.notion.com/v1/oauth/token`，使用 **HTTP Basic Authentication**（`CLIENT_ID:CLIENT_SECRET`，base64）；请求体 JSON 含 `grant_type` 等 | https://developers.notion.com/reference/create-a-token | 2026-09-21 |
| `redirect_uri` 传参规则 | 若授权 URL 带了 `redirect_uri` **或** connection 配置了多个 redirect URI → token 请求体**必须**带 `redirect_uri`；若只配置了一个且授权 URL 未带 → **不允许**在请求体里带 | 同上 | 2026-09-21 |
| **PKCE** | **未文档化**：authorization 指南与 create-a-token 参考中 `PKCE`、`code_challenge`、`code_verifier` 命中数均为 **0**（PKCE 仅出现在 Notion MCP 客户端指南） | https://developers.notion.com/guides/get-started/authorization · https://developers.notion.com/reference/create-a-token | 2026-09-21 |
| 回环/localhost 重定向 | 官方示例仅给出 `https://example.com/auth/notion/callback`；authorization 指南中 `localhost`、`127.0.0.1`、`loopback` 命中数均为 **0** → 桌面端回环重定向是否被接受 **未核实** | https://developers.notion.com/guides/get-started/authorization | 2026-09-21 |

### C4. 官方导出格式与 API 导出能力

| 项目 | 确切结论 | 官方 URL | 访问日期 |
|---|---|---|---|
| 用户级导出格式 | **PDF**、**HTML**、**Markdown & CSV**；「Full page databases will be exports as a CSV file, with Markdown files for each subpage」；Markdown & CSV 导出结果为 **ZIP**（含 CSV 与 Markdown） | https://www.notion.com/help/export-your-content | 2026-09-21 |
| 整库导出 | Settings → Workspace → General → **Export all workspace content**；「You can export all your pages as HTML, Markdown, or CSV (for databases)」；下载链接 **7 天**过期；处理最长约 **30 小时**；仅桌面/网页端可用 | 同上 | 2026-09-21 |
| 标准 REST API 能否整库导出 | **不能**。官方原文将 API 导出限定在 Admin API：「Start a workspace export with the admin API … it's **in beta on the Enterprise Plan**, and an organization owner has to set up an admin bot and token first」 | 同上 | 2026-09-21 |
| Admin API 整库导出端点（Enterprise/beta） | `POST https://api.notion.com/admin/v1/spaces/{space_id}/exports`，需 scope **`workspace:export`**；`export_type` ∈ `html`/`markdown`/`pdf`；另有 `collection_view_export_type`、`flatten_export_filetree`、`include_comments`、`include_contents`、`locale`、`pdf_format` 等参数；状态查询见 `Get workspace export status` | https://developers.notion.com/reference/admin/enqueue-space-export | 2026-09-21 |
| 导出限制 | 每页只能导出当前视图或默认视图（不能导出全部 views）；**Form view 不可导出**；不能重新上传导出内容来「重建」工作区；Windows 超长路径可能导致 ZIP 打不开 | https://www.notion.com/help/export-your-content | 2026-09-21 |
| 普通 API 拉取数据的替代做法 | 用 `POST /v1/data_sources/{data_source_id}/query` + `POST /v1/search`（需分别授权）逐对象读取，**不存在**一次性导出文件端点 | https://developers.notion.com/reference/query-a-data-source | 2026-09-21 |

### C5. Notion 两个必答问题

1. **是否可在桌面应用内通过用户自建 OAuth 应用访问？**  
   **可以（工程上有注意事项）**。需注册 **public connection**，走 **authorization code flow**，且为**机密客户端**：token 交换使用 HTTP Basic（`client_id:client_secret`），**官方未文档化 PKCE**，因此桌面应用必须内置 `client_secret`。**回环重定向（`http://127.0.0.1:port`）是否被 Notion 接受，官方文档未记载 → 未核实**，需实机在 Developer portal 里试填 Redirect URI 验证。
2. **是否有官方导出文件（CSV/JSON/ZIP）作为无 OAuth 的降级路径？**  
   **有，但为人工路径**：用户可在 Notion 内 Export 得到 **ZIP（Markdown + CSV）** 或 HTML/PDF，整库可用 **Export all workspace content**，完全不需要 OAuth。反之，**通过 API 做整库导出不属于标准 API 能力**：只有 Enterprise 计划的 Admin API（beta，`workspace:export` scope）能入队整库导出。

---

## D. 未核实清单（不要猜测）

| 条目 | 状态 | 原因 |
|---|---|---|
| Todoist OAuth redirect URL 是否允许 `http://127.0.0.1` / `localhost` / 自定义 scheme / 是否强制 HTTPS | **未核实** | 官方仅写「configure one or more valid OAuth2 redirect URLs」；App Management Console（`https://developer.todoist.com/appconsole.html`）需登录且页面报「couldn't load the required files」 |
| Todoist 备份 ZIP 内部结构（是否含 JSON、清单文件） | **未核实** | 官方只说明 ZIP 内为每项目一个 CSV |
| Todoist 官方 CSV 模板文件内容 | **未直接验证** | 链接来自官方文章，但 `get.todoist.help` 在本环境连接被重置 |
| Google Takeout 中 Tasks 数据文件的确切文件名/扩展名 | **未核实** | Tasks 帮助页只列字段；`takeout.google.com` 需登录 |
| Google Tasks 是否存在官方导入能力 | **未核实** | 官方帮助页只有导出说明 |
| Notion public connection 是否接受回环重定向 | **未核实** | authorization 指南中 `localhost`/`127.0.0.1`/`loopback` 命中数为 0，示例仅 HTTPS 域名 |
| Notion public connection 是否支持 PKCE | **未核实（文档未记载）** | 授权指南与 token 端点文档中无 `code_challenge`/`code_verifier` |

---

## E. 对桌面导入功能的三条直接结论

1. **三家都允许桌面应用注册并使用自建 OAuth 应用**，但**只有 Google 官方明确支持「授权码 + PKCE + 回环重定向」这一公共客户端模型**；Todoist 与 Notion 均为**机密客户端**流程（必须携带 `client_secret`），且**回环重定向支持均未文档化**。
2. **三家都有官方导出文件可作为无 OAuth 降级路径**：Todoist = 每项目 **CSV**（与导入模板同源，可回导）/ Pro 备份 **ZIP(CSV)**；Google Tasks = **Takeout Zip/Tgz**；Notion = **ZIP(Markdown+CSV)** / HTML / PDF。全部为**用户手动**触发下载，导入器需提供「选择本地文件/解压目录」入口。
3. **结构最稳定、最适合自动化的是 Todoist CSV**（列名与语义官方逐字文档化，且同一格式可直接回写导入）；Google Takeout 与 Notion 导出为通用归档，需按内容特征做容错解析。
