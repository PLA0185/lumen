# 调研文档索引

本目录存放开发前必须完成的官方文档调研结果（任务书 §2.2、§2.3、§12）。

> **统一说明**：所有条目的官方 URL 与访问日期均为 **2026-09-23**（本机系统时钟实测）。
> 凡官方文档未记载者，一律明确标注 **未核实**，不作推测填充——这是任务书 §2.2
> 「不要凭记忆固定模型名或假设不同提供商完全兼容」的直接要求。

## 文件清单

| 文件 | 内容 | 对应任务书条款 |
| --- | --- | --- |
| [`0.4.0-rc-closure.md`](./0.4.0-rc-closure.md) | 0.4.0 RC 最终收口：提交范围、P0/P1、完整门禁、桌面验收和视觉截图 | 0.4.0 RC 最终收口任务书 |
| [`0.4.1-backlog.md`](./0.4.1-backlog.md) | RC 阶段确认不阻断发布的 P2/P3 清单 | 0.4.0 RC 最终收口任务书 |
| [`api-research.md`](./api-research.md) | **AI 提供商适配层事实核查**：DeepSeek / OpenAI / Anthropic 的 Base URL、鉴权方式、当前模型 ID、结构化输出能力、SSE 流式差异、错误码、限流、计费单位与数据训练姿态 | §2.2、§6 |
| [`api-research-tauri2.md`](./api-research-tauri2.md) | **Tauri 2 事实核查**：稳定版本与 MSRV、官方插件矩阵、窗口能力 API 签名、托盘 API、打包与卸载数据行为、Windows 通知机制与限制、Windows 构建前置要求 | §2.1、§2.3、§8 |
| [`api-research-deps.md`](./api-research-deps.md) | **前端依赖与第三方导入格式**：React/Vite/TS 版本、功能库选型与许可、FullCalendar v7 破坏性变更、Todoist/Microsoft To Do/Google Tasks/Notion 官方接口与导出格式 | §2.3、§7、§9 |
| [`api-research-rust-crates.md`](./api-research-rust-crates.md) | **Rust crates 详版**：sqlx、chrono、rrule、keyring、reqwest 等的版本、MSRV、许可与已核实 API 事实（含 15 项未核实清单） | §2.4、§10 |
| [`third-party-task-import-verification.md`](./third-party-task-import-verification.md) | **第三方任务迁移专题**：各平台官方 API / 官方导出文件可用性、OAuth 落地路径排序、无授权时的降级方案 | §9 |

## 几条改变实现决策的关键结论

开发过程中，以下结论直接改变了代码或计划（详见对应文档）：

### 1. 模型 ID 与记忆中的完全不同（`api-research.md`）

| 常见记忆 | 2026-09-23 官方实际 |
| --- | --- |
| DeepSeek `deepseek-chat` / `deepseek-reasoner` | **已不存在**，现为 `deepseek-flash`、`deepseek-v4-pro` |
| OpenAI 旗舰 `gpt-5.x` | `gpt-6-astra`，官方当前推荐 **Responses API** |
| Claude `claude-3-5-sonnet` | `claude-fable-5-1`、`claude-opus-5-5`、`claude-sonnet-5` |
| Anthropic 文档在 `docs.anthropic.com` | 已迁至 `platform.claude.com` |

**适配层三处致命差异**：Anthropic 用 `x-api-key`（不是 `Bearer`）且 `max_tokens` 必填、
响应文本在 `content` 数组里；三家 SSE **不能共用解析器**（DeepSeek 有 `: keep-alive`
注释与 `data: [DONE]`，Anthropic 是命名事件且**无** `[DONE]`）。

**合规提示**：三家数据训练姿态不一致——OpenAI/Anthropic 默认不用于训练，而
**DeepSeek 默认可用于改进服务，需用户主动 opt-out，且数据存储在中国境内**。
因此 UI 提示文案不能三家统一。

### 2. Windows 通知的点击回调在桌面端不受支持（`api-research-tauri2.md`）

`tauri-plugin-notification` 底层确实走 WinRT Toast，但官方 Actions API 标注
**Mobile Only**，`desktop.rs` 源码注释原文：`scheduling, grouping and action related
options are ignored`。→ 任务书 §4.3 的「通知点击能定位任务」**无法通过插件回调实现**，
需要改用 `tauri-winrt-notification` 或"应用启动时检查未处理提醒"的替代路径。
此项已记为待验证风险，不会假装已实现。

### 3. 卸载数据保留是安装器原生行为（`api-research-tauri2.md`）

NSIS 卸载页有 **"Delete app data" 复选框，默认不勾选（即保留）**；勾选且非更新模式时
才执行 `RmDir /r "$APPDATA\${BUNDLEID}"`（目录名是 **identifier**，不是 productName）。
→ 任务书 §10「卸载后的数据保留/删除选项」由 Tauri 直接满足，无需自行实现卸载脚本。
这也印证了把数据库放在 `app_data_dir()` 而非安装目录的决定是正确的。

### 4. 微软并未宣布 Microsoft To Do 停用（`api-research-deps.md`）

官方 FAQ 原文："There is no impact on existing user scenarios or functionality of To Do."
真正被停用的是 **Project for the web**（2025-08 已停）与 **Project Online**（2026-09-30）。
→ To Do 导入功能**不必按紧急项排期**。但要注意 To Do **没有官方导出文件**，
OAuth 是唯一途径，因此它排在导入功能的第二优先位。

### 5. 依赖 feature 名与记忆中不同（已实际踩坑并修正）

以下都是**编译期被证伪**的记忆偏差，均已修正代码：

| 依赖 | 记忆中的写法 | 实际正确写法 |
| --- | --- | --- |
| `reqwest` 0.13 | `rustls-tls`、可加 `webpki-roots` | `rustls`（无 `webpki-roots` 这个 feature） |
| `keyring` 4.x | `features = ["windows-native"]` | 无该 feature，Windows 后端默认启用 |
| `sqlx` 0.9 | — | MSRV = **1.94.0**，`sqlite` feature 即 `sqlite-bundled` |
| `@vitejs/plugin-react` 6.x | 可配 Vite 7 | peer 要求 **Vite ^8.0.0** |
| ESLint | 9.x + flat config | **9.x 已于 2026-08-06 EOL**，主线为 10.x |

### 6. 官方文档自身存在的矛盾（已记录，实现时显式规避）

`api-research.md` 发现 OpenAI 官方文档对 `/v1/responses` 的应用状态保留期有两处
不一致表述。→ 实现时**显式传 `store: false`** 以获得可预期行为，而不是依赖默认值。

## 未核实项汇总

各文档末尾均列有"未核实"清单（`api-research-tauri2.md` 附录 B 共 8 项、
`api-research-rust-crates.md` 共 15 项、`api-research-deps.md` 若干）。
这些项目**不得**在功能验收表中标记为已完成，实机验证后才能改为完成。
