# AI 适配层 API 事实核查（DeepSeek / OpenAI / Anthropic）

> **调研日期（全部访问日期）：2026-09-23**
> **方法**：仅采信各服务商官方文档站点的原始页面。凡官方文档未记载者，明确标注 **未核实**，不作推断。
> **重要前提**：三家 API **互不兼容**，差异是结构性的而非参数级（详见 §C.3、§D.2）。适配层必须按 provider 分支实现，禁止共用同一套请求/响应模型。

---

## A. DeepSeek（深度求索）

### A.1 Base URL 与 `/v1` 语义

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| Base URL（OpenAI 格式） | `https://api.deepseek.com` | [Your First API Call](https://api-docs.deepseek.com/) | 2026-09-23 |
| Base URL（Anthropic 格式） | `https://api.deepseek.com/anthropic` | [Your First API Call](https://api-docs.deepseek.com/) | 2026-09-23 |
| Base URL（Beta 特性） | `https://api.deepseek.com/beta`（`strict` 工具模式必需） | [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls) | 2026-09-23 |
| Chat 端点路径 | `POST /chat/completions`（即 `https://api.deepseek.com/chat/completions`，**注意路径中无 `/v1`**） | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| `/v1` 的语义 | **未核实**。官方文档当前只给出上表两个 base_url，全站未出现 `/v1` 段，也未说明其与模型版本的关系 | [Your First API Call](https://api-docs.deepseek.com/) | 2026-09-23 |

> 实现建议：base_url 硬编码 `https://api.deepseek.com`，请求路径拼接 `/chat/completions`。**不要**自行加 `/v1`——官方文档未承诺该路径；如需容错可在配置层留出 base_url 覆盖项，但默认值按官方文档写。

### A.2 当前可用模型 ID

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 模型 ID（全部） | `deepseek-flash`、`deepseek-v4-pro` | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing)、[Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| `deepseek-flash` 对应模型版本 | DeepSeek-V4.1-Flash | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| `deepseek-v4-pro` 对应模型版本 | DeepSeek-V4-Pro-0813 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| 已退役的旧 ID | `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp` 仍被接受，但对应模型已下线，请求由 DeepSeek-V4.1-Flash 提供服务并按 Flash 价格计费 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |

**注意：`deepseek-chat` / `deepseek-reasoner` 在当前官方文档的模型列表中已不存在。** 适配层不得再默认使用这两个 ID。

### A.3 鉴权、兼容性、能力、上限、超时、限流、错误码

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 鉴权 Header | `Authorization: Bearer ${DEEPSEEK_API_KEY}`，配 `Content-Type: application/json` | [Your First API Call](https://api-docs.deepseek.com/) | 2026-09-23 |
| OpenAI 兼容 | 是，官方称 "uses an API format compatible with OpenAI/Anthropic" | [Your First API Call](https://api-docs.deepseek.com/) | 2026-09-23 |
| 上下文长度 | 1M tokens（两个模型相同） | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| 最大输出 | 384K；`max_tokens` 取值 1–393216 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing)、[Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| `max_tokens` 默认值 | 非思考模式 8K；思考模式 64K；`reasoning_effort: "max"` 时 128K | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| 思考模式开关 | `thinking: {"type": "enabled"｜"disabled"}`，**默认 enabled** | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| 推理强度 | `reasoning_effort`: `none`／`low`／`high`／`max`，默认 `high`；兼容映射：`minimal`→`low`、`medium`／`xhigh`→`high` | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| Function calling | 支持（`tools[].type` 仅 `function`），思考模式亦支持 | [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls) | 2026-09-23 |
| Tool `strict` 模式 | Beta，需用 `base_url=.../beta`，且**所有** function 都要 `strict: true` | [Tool Calls](https://api-docs.deepseek.com/guides/tool_calls) | 2026-09-23 |
| JSON 输出 | 支持 `response_format: {"type": "json_object"}`；需在 prompt 中自行包含 "json" 字样并提供示例；官方提示**偶发返回空内容** | [JSON Output](https://api-docs.deepseek.com/guides/json_mode) | 2026-09-23 |
| 结构化输出（json_schema） | **不支持**。官方仅提供 `json_object` 模式，无 `json_schema`／`strict` 响应格式 | [JSON Output](https://api-docs.deepseek.com/guides/json_mode)、[Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| Vision | 仅 `deepseek-flash` 支持，`deepseek-v4-pro` 不支持 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| FIM Completion（Beta） | 支持，仅非思考模式 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| `tool_choice` 限制 | 思考模式下 `required` 与指定具名工具**不支持**，返回 400 | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| 已废弃参数 | `frequency_penalty`、`presence_penalty` 不再支持，传入无效 | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| 超时 / 连接保持 | 请求发出后若 10 分钟内未开始推理，服务器关闭连接；非流式请求持续返回空行，流式请求持续返回 SSE keep-alive 注释 `: keep-alive`——**解析器必须能跳过空行与注释** | [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit) | 2026-09-23 |
| 限流（并发） | 按账号计：`deepseek-flash` 2500、`deepseek-v4-pro` 500；超限返回 HTTP 429 | [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit) | 2026-09-23 |
| 限流（RPM/TPM） | **未核实**。官方文档只给出并发连接数限制，未公布 RPM/TPM 数值 | [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit) | 2026-09-23 |
| 用户隔离参数 | `user_id`（正则 `[a-zA-Z0-9\-_]+`，≤512）；OpenAI SDK 需放 `extra_body`；Anthropic 格式放 `metadata.user_id` | [Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit) | 2026-09-23 |

**错误码表（官方全表）** — 来源：[Error Codes](https://api-docs.deepseek.com/quick_start/error_codes)，访问日期 2026-09-23

| CODE | 含义 |
| --- | --- |
| 400 - Invalid Format | 请求体格式错误 |
| 401 - Authentication Fails | API key 错误 |
| 402 - Insufficient Balance | 余额不足 |
| 422 - Invalid Parameters | 请求参数无效 |
| 429 - Rate Limit Reached | 请求过快（含并发超限） |
| 500 - Server Error | 服务端错误 |
| 503 - Server Overloaded | 服务过载 |

### A.4 列出模型接口

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 是否有 GET /models | **有**。`GET /models`（相对 base_url，即 `https://api.deepseek.com/models`） | [Lists Models](https://api-docs.deepseek.com/api/list-models) | 2026-09-23 |
| 响应结构 | `{ "object": "list", "data": [ { "id": string, "object": "model", "owned_by": string } ] }` | [Lists Models](https://api-docs.deepseek.com/api/list-models) | 2026-09-23 |
| 附带接口 | 另有 `GET /user/balance`（查询余额） | [Get User Balance](https://api-docs.deepseek.com/api/get-user-balance) | 2026-09-23 |

### A.5 定价与计费单位

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 定价页 | `https://api-docs.deepseek.com/quick_start/pricing` | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| 计费单位 | **每 1M tokens**，且按「缓存命中 / 缓存未命中 / 输出」三类分别计价，每类再分「高峰 / 非高峰」 | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing) | 2026-09-23 |
| `deepseek-flash` 输入（缓存命中） | 非高峰 $0.003 / 高峰 $0.006 | 同上 | 2026-09-23 |
| `deepseek-flash` 输入（缓存未命中） | 非高峰 $0.15 / 高峰 $0.3 | 同上 | 2026-09-23 |
| `deepseek-flash` 输出 | 非高峰 $0.6 / 高峰 $1.2 | 同上 | 2026-09-23 |
| `deepseek-v4-pro` 输入（缓存命中） | 非高峰 $0.022 / 高峰 $0.044 | 同上 | 2026-09-23 |
| `deepseek-v4-pro` 输入（缓存未命中） | 非高峰 $0.66 / 高峰 $1.32 | 同上 | 2026-09-23 |
| `deepseek-v4-pro` 输出 | 非高峰 $1.98 / 高峰 $3.96 | 同上 | 2026-09-23 |
| 高峰时段定义 | 01:00–04:00 与 06:00–10:00 UTC，周一至周五，**不含中国法定节假日**；其余时间（含周末与中国法定节假日全天）为非高峰 | 同上 | 2026-09-23 |
| 非高峰折扣 | 非高峰费率为高峰费率的一半 | 同上 | 2026-09-23 |
| 扣费顺序 | 优先扣赠送余额，再扣充值余额 | 同上 | 2026-09-23 |

> **UI 费用提示要点**：DeepSeek 是三家唯一「按时段浮动 + 缓存命中率影响单价」的 provider。要在 UI 给出准确估算，必须同时知道当前 UTC 时段、是否高峰日，以及 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` 的比例。

---

## B. OpenAI

### B.1 Base URL 与 API 选型

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| Base URL | `https://api.openai.com/v1` | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| 数据驻留区域端点 | `https://us.api.openai.com/v1`、`https://eu.api.openai.com/v1`（需资格审批，2026-03-05 及以后发布的合格模型加价 10%） | [Data controls](https://developers.openai.com/api/docs/guides/your-data) | 2026-09-23 |
| 当前推荐 API | **Responses API（`POST /v1/responses`）**。官方 API Overview 明确列为「direct model requests, tool use, audio, image, and text inputs, and stateful interactions」的首选入口 | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| Chat Completions 状态 | `POST /v1/chat/completions` **仍受支持**，未标注 deprecated；旗舰模型 `gpt-6-astra` 的 endpoint 表同时列出 Chat Completions 与 Responses 为 Supported | [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra) | 2026-09-23 |
| 旧版 Completions | `v1/completions` 标为 legacy，`gpt-6-astra` **不支持** | [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra) | 2026-09-23 |
| REST 版本 | `v1`；响应头 `openai-version` 当前值 `2020-10-01` | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |

> 适配层建议：新实现走 Responses API；Chat Completions 作为兼容路径保留（第三方 OpenAI 兼容服务大多只实现 Chat Completions）。

### B.2 当前可用模型 ID

来源：[Models](https://developers.openai.com/api/docs/models)、[Pricing](https://developers.openai.com/api/docs/pricing)，访问日期 2026-09-23。

**最新旗舰与主力（文本 / 推理）**

| 模型 ID | 定位 |
| --- | --- |
| `gpt-6-astra` | **当前旗舰**，最复杂端到端工作；上下文 1,050,000 / 最大输入 922,000 / 最大输出 128,000；知识截止 Apr 30, 2026 |
| `gpt-6-sol` | 面向复杂编码与 agentic 工作流 |
| `gpt-6-luna` | 聚焦高吞吐任务的最经济模型 |
| `gpt-5.6-sol` | 上一代旗舰级（Cyber 组亦引用） |
| `gpt-5.6-terra` | 智能与成本平衡 |
| `gpt-5.6-luna` | 成本敏感 / 高吞吐 |
| `gpt-5.6-cyber` | 授权漏洞研究与安全测试 |
| `gpt-5.5` / `gpt-5.5-pro` / `gpt-5.5-cyber` | 上一代 |
| `gpt-5.4` / `gpt-5.4-pro` | 更经济的编码与专业工作 |

**mini / nano 系列（题目要求）**

| 模型 ID | 说明 |
| --- | --- |
| `gpt-5.4-mini` | "strongest mini model yet"，面向编码、computer use、subagents |
| `gpt-5.4-nano` | 最便宜的 GPT-5.4 级模型，简单高吞吐任务 |
| `gpt-5-mini` | 强智能、低成本低延迟 |
| `gpt-5-nano` | 最快最省 |
| `gpt-4.1-mini` / `gpt-4.1-nano` | 上一代小型模型 |
| `gpt-4o-mini` | 更早一代 |

**其它同代可用 ID（部分）**：`gpt-5.2`、`gpt-5.2-pro`、`gpt-5.1`、`gpt-5`、`gpt-5-pro`、`gpt-4.1`、`gpt-4o`、`o3`、`o4-mini`、`o3-pro`、`o1`、`o1-pro`。

**别名（会随新模型发布而漂移，UI 中应避免展示为稳定选项）**
- `gpt-daybreak-blue-latest` → 当前指向 `gpt-5.6-sol`
- `gpt-daybreak-red-latest` → 当前指向 `gpt-5.6-cyber`

**Codex 系列**：`gpt-5.3-codex`、`gpt-5.2-codex`、`gpt-5.1-codex`、`gpt-5.1-codex-max`、`gpt-5.1-codex-mini`、`gpt-5-codex`、`codex-mini-latest`。

**ChatGPT 别名**：`chat-latest`、`gpt-5.3-chat-latest`、`gpt-5.2-chat-latest`、`gpt-5.1-chat-latest`、`gpt-5-chat-latest`。

> 完整目录以官方 Models 页为准；模型下线日期可通过 `/v1/models` 的 `shutdown_date` 字段动态获取（见 §B.4）。

### B.3 鉴权、结构化输出、function calling、错误码、限流头

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 鉴权 | HTTP Bearer：`Authorization: Bearer OPENAI_API_KEY_OR_ACCESS_TOKEN`。也支持 workload identity federation 签发的短期 token | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| 组织 / 项目头 | 可选 `OpenAI-Organization: $ORGANIZATION_ID`、`OpenAI-Project: $PROJECT_ID` | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| 自定义请求 ID | 可选 `X-Client-Request-Id`（仅 ASCII，≤512 字符，需全局唯一；超限返回 400） | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| Header 体积上限 | 请求头总量 < 64 KiB；单个自定义头与自定义头合计建议 ≤ 60 KiB | [API Overview](https://developers.openai.com/api/reference/overview) | 2026-09-23 |
| 结构化输出（Responses） | `text: { format: { type: "json_schema", name: "<name>", strict: true, schema: {…} } }` | [Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) | 2026-09-23 |
| 结构化输出（Chat Completions） | `response_format: { type: "json_schema", … }` | [Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) | 2026-09-23 |
| JSON mode（无 schema 约束） | `text.format = { type: "json_object" }`；官方建议尽可能改用 Structured Outputs | [Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) | 2026-09-23 |
| 拒答字段 | 结构化输出被安全拒答时，响应中出现 `refusal` 字段（不符合所给 schema），**适配层必须显式处理** | [Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) | 2026-09-23 |
| Function calling | 支持；`gpt-6-astra` 的 supported features 含 `function_calling`、`structured_outputs`、`streaming`、`prompt_caching`、`file_search`、`image_input`、`web_search` | [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra) | 2026-09-23 |
| 不支持项（gpt-6-astra） | Realtime、Assistants、Fine-tuning、Embeddings、Images、Videos、Audio、Moderations、legacy Completions | [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra) | 2026-09-23 |

**限流响应头** — 来源：[API Overview](https://developers.openai.com/api/reference/overview)，访问日期 2026-09-23

`x-ratelimit-limit-requests`、`x-ratelimit-limit-tokens`、`x-ratelimit-remaining-requests`、`x-ratelimit-remaining-tokens`、`x-ratelimit-reset-requests`、`x-ratelimit-reset-tokens`，以及项目级 token 限流存在时出现的 `x-ratelimit-limit-project-tokens`、`x-ratelimit-remaining-project-tokens`、`x-ratelimit-reset-project-tokens`。

**其它诊断头**：`openai-organization`、`openai-processing-ms`、`openai-version`（当前 `2020-10-01`）、`x-request-id`。

**错误码** — 来源：[Error codes](https://developers.openai.com/api/docs/guides/error-codes)，访问日期 2026-09-23

| Code | 标识 | 说明 |
| --- | --- | --- |
| 400 | `invalid_request_error`（`error.param=service_tier`） | 请求或解析出的 service tier 不被项目允许 |
| 401 | — | 鉴权无效 / API key 错误 / 非组织成员 / IP 未授权 |
| 403 | — | 国家、地区或地域不受支持 |
| 429 | `credit_balance_exhausted` | 预付额度耗尽 |
| 429 | — | 请求速率超限，附 `Retry-After` 时须遵守 |
| 429 | `rate_limit_error` / `slow_down` | 请求速率爬升过快（即使未超 RPM/TPM 也可能触发） |
| 429 | `organization_spend_limit_exceeded` | 组织级消费上限 |
| 429 | `project_spend_limit_exceeded` | 项目级消费上限 |
| 429 | `organization_usage_limit_exceeded` | OpenAI 分配的用量上限 |
| 500 | — | 服务端错误 |
| 503 | `service_unavailable_error` / `server_is_overloaded` | 模型暂时过载，附 `Retry-After` |

> **重试策略注意**：计费 / 消费 / 配额类错误重试不会恢复访问，必须改额度或限额。

### B.4 GET /v1/models 响应结构

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 端点 | `GET /models`（即 `https://api.openai.com/v1/models`） | [List models](https://developers.openai.com/api/reference/resources/models/methods/list) | 2026-09-23 |
| 响应结构 | `{ "object": "list", "data": [ { "id": string, "created": number, "object": "model", "owned_by": string, "shutdown_date": string \| null } ] }` | 同上 | 2026-09-23 |
| 特别字段 | `shutdown_date`：模型下线日期，未公布时为 `null`——**可直接用于 UI 提示「即将下线」** | 同上 | 2026-09-23 |

---

## C. Anthropic Claude

### C.1 Base URL 与版本头

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| Base URL | `https://api.anthropic.com`（消息端点 `POST /v1/messages`） | [Create a Message](https://platform.claude.com/docs/en/api/http/beta/messages/create)、[Claude API errors](https://platform.claude.com/docs/en/api/errors) | 2026-09-23 |
| **`anthropic-version` 确切值** | **`2023-06-01`** | [Versions](https://platform.claude.com/docs/en/api/versioning) | 2026-09-23 |
| 版本策略 | 同一版本内保证不破坏既有输入/输出参数；可能新增可选输入、新增输出值、新增枚举变体（例如**流式事件类型**）——解析器必须优雅忽略未知事件类型 | [Versions](https://platform.claude.com/docs/en/api/versioning) | 2026-09-23 |
| 旧版本 | `2023-01-01` 为初始版本，已视为 deprecated，新用户可能不可用 | [Versions](https://platform.claude.com/docs/en/api/versioning) | 2026-09-23 |
| 文档站域名 | 官方 Claude 文档现位于 `platform.claude.com/docs/...`（`docs.anthropic.com` 已 301 至该域） | [Messages](https://platform.claude.com/docs/en/api/messages) | 2026-09-23 |

### C.2 当前可用模型 ID

来源：[Models overview](https://platform.claude.com/docs/en/models/overview)、[List Models](https://platform.claude.com/docs/en/api/models/list)，访问日期 2026-09-23。

| 模型 | Claude API ID | 上下文 | 最大输出 | 定价（输入/输出，每 MTok） |
| --- | --- | --- | --- | --- |
| Claude Fable 5.1 | `claude-fable-5-1` | 1M | 128K | $10 / $50 |
| Claude Opus 5.5 | `claude-opus-5-5` | 1M | 128K | $4 / $20 |
| Claude Sonnet 5 | `claude-sonnet-5` | 1M | 128K | $2 / $10 |
| Claude Haiku 4.5 | `claude-haiku-4-5-20251001`（别名 `claude-haiku-4-5`） | 200K | 64K | $1 / $5 |

**仍在售的历史模型**：`claude-fable-5`、`claude-opus-5`、`claude-opus-4-8`、`claude-opus-4-7`、`claude-opus-4-6`、`claude-opus-4-5`、`claude-sonnet-4-6`、`claude-sonnet-4-5`。

**其他平台 ID 形态（同一模型）**：Amazon Bedrock 用 `anthropic.claude-opus-5-5`（Bedrock Messages-API 端点）；Google Cloud 用 `claude-haiku-4-5@20251001` 形式；Microsoft Foundry 与 Claude Platform on AWS 用 Claude API 的 dateless 形态。

**退役承诺**：Fable 5.1 不早于 2027-09-01；Opus 5.5 不早于 2027-09-22；Sonnet 5 不早于 2027-06-30；Haiku 4.5 不早于 2026-10-15。

**结构化输出支持的模型**（官方列出）：`claude-fable-5-1`、`claude-mythos-5-1`、`claude-fable-5`、`claude-mythos-5`、`claude-mythos-preview`、`claude-opus-5-5`、`claude-opus-5`、`claude-opus-4-8`、`claude-opus-4-7`、`claude-opus-4-6`、`claude-sonnet-5`、`claude-sonnet-4-6`、`claude-sonnet-4-5-20250929`、`claude-opus-4-5-20251101`、`claude-haiku-4-5-20251001`。来源：[Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)，访问日期 2026-09-23。

### C.3 与 OpenAI 的关键差异（适配层核心）

**请求体结构** — 来源：[Create a Message](https://platform.claude.com/docs/en/api/http/beta/messages/create)，访问日期 2026-09-23

| 项目 | Anthropic | OpenAI 对照 |
| --- | --- | --- |
| 端点 | `POST /v1/messages` | `POST /v1/responses` / `POST /v1/chat/completions` |
| system 参数位置 | **顶层 `system` 字段**（`string` 或 text block 数组）。官方原文：*"there is no `"system"` role for input messages in the Messages API"* | OpenAI 把 system 作为 `messages`/`input` 里的一条 role 消息 |
| 对话中间 system 消息 | **现支持** `role: "system"` 的消息（属 ZDR/HIPAA 合格特性），另有 `mid-conversation-system-clear-at-2026-08-21` 等 beta 头 | Responses API 支持；Chat Completions 亦支持插入 system 消息 |
| `messages` 格式 | `{ role: "user"｜"assistant", content: string ｜ ContentBlock[] }`；`content` 为字符串时等价于单个 `{"type":"text","text":…}` block | OpenAI 的 content 分区模型不同（`input_text` / `output_text` 等） |
| `max_tokens` | **必填**（官方 API 参考中 `max_tokens: number` 未标注 optional，而同页 `system`、`thinking` 等标注了 optional；最小值 0，设为 0 可仅预热 prompt cache） | 可选 |
| 消息数上限 | 单请求最多 100,000 条消息 | 未核实 |
| 前缀续写（prefill） | **Claude 4.6 及之后模型不支持** assistant 消息 prefill，返回 400 `invalid_request_error`。需改用 structured outputs、system 指令或 `output_config.format` | — |
| 强制工具调用 | Opus 5.5 / Fable 5.1 / Mythos 5.1 **不支持** `tool_choice: {"type":"any"}` 或具名工具，返回 400；仅接受 `auto`（默认）与 `none` | OpenAI 支持 `required` 与具名强制 |
| 思考配置 | `thinking: {"type":"adaptive"｜"enabled"}` + `output_config.effort`；Claude 4.7+ 已**移除** `type:"enabled"`（返回 400）；Fable 5.1 / Opus 5.5 等 **thinking 恒开**，传 `disabled` 返回 400 | OpenAI 用 `reasoning.effort` |
| 请求体大小上限 | Messages API 32 MB；Token Counting API 32 MB；Batch 256 MB；Files 500 MB，超限 413 `request_too_large` | 未核实 |

**鉴权头差异** — 来源：[Create a Message](https://platform.claude.com/docs/en/api/http/beta/messages/create)、[Claude API errors](https://platform.claude.com/docs/en/api/errors)

| 项目 | Anthropic | OpenAI |
| --- | --- | --- |
| 主鉴权头 | **`x-api-key: $ANTHROPIC_API_KEY`** | `Authorization: Bearer $OPENAI_API_KEY` |
| 版本头 | **必需** `anthropic-version: 2023-06-01` | 无（版本含在 URL `/v1` 中） |
| Beta 头 | 可选 `anthropic-beta`（数组，形如 `structured-outputs-2025-11-13`） | 无对应机制 |
| 工作区头 | 可选 `anthropic-workspace-id` | 可选 `OpenAI-Organization` / `OpenAI-Project` |
| 用户档案头 | 可选 `anthropic-user-profile-id`（需 user-profiles beta 头） | — |

> **适配层务必记住：Anthropic 用 `x-api-key`，不是 Bearer。** 这是最常见的对接错误。另有 Claude Platform on AWS 走 SigV4，与直连 API 完全不同。

**响应结构差异** — 来源：[Create a Message](https://platform.claude.com/docs/en/api/http/beta/messages/create)、[Streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)

- Anthropic 顶层返回 **`content` 数组**（content blocks），每个 block 有 `type`：`text`、`tool_use`、`thinking`、`redacted_thinking`、`server_tool_use`、`web_search_tool_result` 等；配套 `stop_reason`、`stop_sequence`、`usage`。
- 文本不在 `choices[0].message.content` 里，需遍历 `content` 取 `type === "text"` 的 block 拼接。
- **thinking block 完整性要求**：最新 assistant 消息中的 `thinking` / `redacted_thinking` block 若被编辑、重排、过滤或重建，请求返回 400。若上层代码按 type 过滤 block 再回传，必须同时保留 `thinking` 与 `redacted_thinking`。
- 在 `thinking-binding-controls-2026-08-01` beta 下，thinking block 会与对话前缀绑定；历史被改动时默认报 400（可设 `prefix_mismatch_behavior: "drop_block"`）。**对话历史必须保持 append-only。**

**结构化输出方案** — 来源：[Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)，访问日期 2026-09-23

| 项目 | 结论 |
| --- | --- |
| 是否已支持原生 structured outputs | **是，已 GA**（`status: ga`） |
| JSON 输出参数 | `output_config: { format: { type: "json_schema", schema: {…} } }` |
| 旧参数（过渡期） | beta 头 `structured-outputs-2025-11-13` 与 `output_format` 字段仍被接受；Python SDK v1.0+ 在 `beta.messages.create()` / `count_tokens()` 上不再接受 `output_format={…}`，会抛 `TypeError` |
| 严格工具调用 | `tools[].strict = true`（与 JSON 输出相互独立，可同时使用） |
| 结果位置 | 合法的 JSON 字符串位于响应的 **text content block** 中 |
| grammar 缓存 | 编译后的 grammar 自最后使用起缓存 24 小时；schema 结构、工具集合变化会失效（仅改 `name`/`description` 不失效） |
| 隐式成本 | 使用 structured outputs 时，Claude 会自动收到一段额外的 system prompt 解释输出格式——**输入 token 略增**（按 system prompt 正常计费）；改动 `output_config.format` 会使该会话的 prompt cache 失效 |
| JSON Schema 限制 | 支持 object/array/string/integer/number/boolean/null、`enum`、`const`、`anyOf`/`allOf`、`$ref`/`$defs`、`default`、`required`、字符串格式（date-time/time/date/duration/email/hostname/uri/ipv4/ipv6/uuid）、数组 `minItems`（仅 0 与 1）；**不支持**递归 schema、enum 内复杂类型、外部 `$ref`、数值约束（`minimum`/`maximum`/`multipleOf`）、字符串长度约束（`minLength`/`maxLength`）。对象的 `additionalProperties` **必须为 `false`** |
| PHI 注意 | schema 会被编译并缓存，**不得在 JSON schema 定义中放入 PHI**（含属性名、`enum`、`const`、`pattern`） |

**SSE 流式事件格式差异** — 来源：[Streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)、[Versions](https://platform.claude.com/docs/en/api/versioning)

| 项目 | Anthropic |
| --- | --- |
| 事件命名 | **全部为命名事件**（`event: message_start` 等），data 内另有与之匹配的 `type` 字段 |
| `data: [DONE]` | **不存在**。自 `2023-06-01` 版本起已移除该终止事件 |
| delta 语义 | **增量**。例如依次为 `" Hello"`、`" my"`、`" name"`…（旧版 `2023-01-01` 为累积式 `" Hello"`、`" Hello my"`…） |
| 事件流顺序 | `message_start` → 若干（`content_block_start` → `content_block_delta`* → `content_block_stop`）→ 若干 `message_delta` → `message_stop` |
| ping | 流中可能穿插任意数量 `ping` 事件（`data: {"type":"ping"}`） |
| content_block_delta 类型 | `text_delta`（字段 `text`）、`input_json_delta`（字段 `partial_json`，**分片的部分 JSON 字符串**，需累积后整体解析）、`thinking_delta`（字段 `thinking`）、`signature_delta`（字段 `signature`，在 `content_block_stop` 前发出） |
| 工具调用入参 | `tool_use` block 的入参以 `input_json_delta` 的 `partial_json` 分片下发；最终 `tool_use.input` 始终是**对象**。当前模型一次只完整发出一个 key/value 属性，事件间可能有延迟 |
| usage 语义 | `message_delta` 中的 `usage` token 计数是**累积值**，不是增量 |
| 流中错误 | `event: error` + `data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`；此时 HTTP 已是 200，**错误处理不走标准 HTTP 错误机制** |
| 未知事件 | 版本策略允许新增事件类型，解析器必须优雅忽略未知类型 |
| 断流恢复 | Claude 4.5 及更早：把已收到的部分响应作为 assistant 消息回传续写；**Claude 4.6 及之后改用 user 消息**（例如「Your previous response was interrupted... Continue from where you left off.」）。tool use 与 thinking block 无法部分恢复，只能从最近的 text block 续起 |

### C.4 GET /v1/models 与分页

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 端点 | `GET /v1/models` | [List Models](https://platform.claude.com/docs/en/api/models/list) | 2026-09-23 |
| 排序 | 较新发布的模型排在前面 | 同上 | 2026-09-23 |
| 分页方式 | **游标分页**（非 offset） | 同上 | 2026-09-23 |
| 分页参数 | `after_id`（取该对象之后一页）、`before_id`（取该对象之前一页）、`limit`（默认 20，范围 1–1000） | 同上 | 2026-09-23 |
| 响应结构 | `{ data: [ { type: "model", id, display_name, created_at, max_input_tokens, max_tokens, capabilities: { batch, citations, code_execution, context_management, effort, image_input, pdf_input, structured_outputs, thinking } } ], first_id, last_id, has_more }` | 同上 | 2026-09-23 |
| 分页用法 | `first_id` 可作为上一页的 `before_id`；`last_id` 可作为下一页的 `after_id` | 同上 | 2026-09-23 |
| `capabilities` 价值 | 可编程判断某模型是否支持 `structured_outputs`、`image_input`、`pdf_input`、`effort` 档位、thinking 类型等——**适配层应以此动态裁剪 UI 能力，而非硬编码** | 同上 | 2026-09-23 |
| 请求头 | 需带 `anthropic-version: 2023-06-01` 与 `X-Api-Key` | 同上 | 2026-09-23 |

### C.5 错误码与限流（含 `retry-after`）

**错误码全表** — 来源：[Claude API errors](https://platform.claude.com/docs/en/api/errors)，访问日期 2026-09-23

| HTTP | `error.type` | 说明 |
| --- | --- | --- |
| 400 | `invalid_request_error` | 请求格式或内容问题；也用于其他未列出的 4XX。达到**自设** spend limit 时返回 400（Claude Code 工作区例外，可返回 429） |
| 401 | `authentication_error` | API key 问题（格式错误、已吊销、已过期）；Claude Platform on AWS 上亦可能是 AWS 凭证或 SigV4 签名问题 |
| 402 | `billing_error` | 账单或支付信息问题 |
| 403 | `permission_error` | 凭证无权访问指定资源 |
| 404 | `not_found_error` | 资源不存在，检查路径与资源 ID |
| 409 | `conflict_error` | 与资源当前状态冲突（并发修改或唯一值被占用） |
| 413 | `request_too_large` | 超过字节上限（直连 Claude API 时由 Cloudflare 在到达 API 前返回） |
| 429 | `rate_limit_error` | 触发限流、达到用量层级月度消费上限，或 Claude Code 工作区 limit。**层级消费上限型 429 不带 `retry-after`，会持续失败直到权限恢复** |
| 500 | `api_error` | Anthropic 内部错误，指数退避重试 |
| 504 | `timeout_error` | 处理超时，建议改用流式 Messages API |
| 529 | `overloaded_error` | API 暂时过载（全用户高流量时出现） |

**错误响应体形状**（始终 JSON）：

```json
{
  "type": "error",
  "error": { "type": "not_found_error", "message": "The requested resource could not be found." },
  "request_id": "req_011CSHoEeqs5C35K2UUqR7Fy"
}
```

**请求 ID**：每个响应都带 `request-id` 头（形如 `req_018EeWyXxfu5pfWkrYcMdjWG`），与错误体中的 `request_id` 相同。Claude Platform on AWS 上还有 `x-amzn-requestid`（CloudTrail 用）。

**限流机制** — 来源：[Rate limits](https://platform.claude.com/docs/en/api/rate-limits)，访问日期 2026-09-23

| 项目 | 结论 |
| --- | --- |
| 限流维度 | 按模型类别分别限制 **RPM（请求/分）**、**ITPM（输入 token/分）**、**OTPM（输出 token/分）**；按组织级生效 |
| 算法 | **token bucket**，容量持续补充而非固定时间窗重置 |
| 用量层级 | Start / Build / Scale / Custom；新组织或用量历史有限者可能先处于 Evaluation 层级 |
| 月度消费上限 | Start $500、Build $1,000、Scale $200,000；Custom 无上限 |
| 缓存感知 ITPM | 多数模型**仅未缓存输入 token 计入 ITPM**：`input_tokens` 与 `cache_creation_input_tokens` 计入，`cache_read_input_tokens` **不计入**（Claude Haiku 3.5 例外，其缓存读取也计入） |
| 总输入 token 计算 | `total_input_tokens = cache_read_input_tokens + cache_creation_input_tokens + input_tokens`（`input_tokens` 只代表最后一个缓存断点之后的 token） |
| OTPM | 实时按实际产出 token 评估，`max_tokens` **不影响** OTPM 计算 |
| 短时突发 | 可能按更短区间强制（如 60 RPM 实际按 1 请求/秒执行） |
| 加速限流 | 用量骤增可能触发 429（acceleration limits），应渐进提升流量 |

**限流响应头**（官方全表）

| Header | 说明 |
| --- | --- |
| `retry-after` | 需等待的秒数；提前重试会失败。**层级消费上限型 429 不返回该头** |
| `anthropic-ratelimit-requests-limit` / `-remaining` / `-reset` | 请求数限制、剩余量、完全补充时间（RFC 3339） |
| `anthropic-ratelimit-tokens-limit` / `-remaining` / `-reset` | token 限制（取当前生效的最严格限制；剩余量舍入到千） |
| `anthropic-ratelimit-input-tokens-limit` / `-remaining` / `-reset` | 输入 token 限制 |
| `anthropic-ratelimit-output-tokens-limit` / `-remaining` / `-reset` | 输出 token 限制 |
| `anthropic-priority-input-tokens-*` / `anthropic-priority-output-tokens-*` | 仅 Priority Tier |
| `anthropic-workspace-id` | 该请求计入的工作区 ID |

> **429 细分很重要**：层级消费上限触发的 429 **没有** `retry-after` 且重试无效（官方 SDK 的自动重试也会一直失败）；而普通限流 429 带 `retry-after`，应遵守。Messages API 上可通过 `error.details.error_code == "enforced_spend_limit_reached"` 区分。官方 SDK 默认自动重试 2 次（连接错误、限流、5xx，指数退避，遵守 `retry-after`）。

---

## D. 通用

### D.1 数据使用 / 隐私政策要点（三家默认策略不同）

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| **OpenAI** 是否用 API 数据训练 | **否**。官方原文：*"As of March 1, 2023, data sent to the OpenAI API is not used to train or improve OpenAI models (unless you explicitly opt in to share data with us)."* | [Data controls](https://developers.openai.com/api/docs/guides/your-data) | 2026-09-23 |
| OpenAI 滥用监控日志保留 | 默认对全部 API 功能生成，保留**最多 30 天**（法律要求或保护服务/第三方免受伤害所需时更长） | 同上 | 2026-09-23 |
| OpenAI 应用状态保留 | `/v1/chat/completions` 与 `/v1/responses` 表格标注 Application state retention 为 "None, see below for exceptions"；但 `/v1/responses` 章节又写明"默认或 `store=true` 时有 30 天应用状态保留"。**官方文档此处两处表述不一致**，实现上建议显式传 `store: false` 以取得可预期行为 | 同上 | 2026-09-23 |
| OpenAI ZDR / MAM | 需 OpenAI 事先批准并接受附加要求；启用 ZDR 后 `store` 参数一律按 `false` 处理 | 同上 | 2026-09-23 |
| OpenAI 数据驻留 | 按项目配置，需资格审批；非美国区域还需通过滥用监控控制审批并签署 Modified Retention 附件 | 同上 | 2026-09-23 |
| **Anthropic** 是否用 API 数据训练 | **否**。官方承诺：*"Retained data is never used for model training without your express permission."* | [API and data retention](https://platform.claude.com/docs/en/manage-claude/api-and-data-retention) | 2026-09-23 |
| Anthropic 默认保留 | **默认不保留**对话内容（用户的 prompt 与 Claude 的输出）。例外：**Covered Models**（Claude Fable 5.1 / Mythos 5.1 / Fable 5 / Mythos 5）要求 **30 天**保留 | 同上 | 2026-09-23 |
| Anthropic Covered Models 影响 | 这些模型**不适用 ZDR**，除非得到 Anthropic 明确授权；ZDR 组织的请求会返回 400 `invalid_request_error`（"your organization or workspace must have data retention enabled"）。可为单个工作区开启 30 天保留而不影响组织其他工作区 | 同上 | 2026-09-23 |
| Anthropic ZDR | 按组织启用，需联系销售；**ZDR 组织不支持 CORS**，浏览器端调用必须走自建后端代理 | 同上 | 2026-09-23 |
| Anthropic 标记内容保留 | 即使有 ZDR 或 HIPAA 安排，被信任与安全系统标记的会话，输入输出可能保留**最多 2 年** | 同上 | 2026-09-23 |
| Anthropic 隐私政策 | `https://www.anthropic.com/legal/privacy`；商业数据保留政策：`https://privacy.claude.com/en/articles/7996866-how-long-do-you-store-my-organization-s-data` | 同上 | 2026-09-23 |
| Anthropic 结构化输出保留 | ZDR 下为 "Yes (qualified)"：prompt 与输出不存储，**仅 JSON schema 被缓存，自最后使用起最多 24 小时** | 同上 | 2026-09-23 |
| **DeepSeek** 是否用 API 数据训练 | **默认会用于改进**（可退出）。Terms of Use 4.3：*"Under the premise of secure encryption technology processing, strict de-identification rendering, and irreversibility to identify specific individuals, we may, to a minimal extent, use Inputs and Outputs to provide, maintain, operate, develop or improve the Services or the underlying technologies supporting the Services. If you refuse to allow us to process the data in the manner described above, you can opt out by turning off 'Improve the model for everyone'."* | [DeepSeek Terms of Use](https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html)（Last Update: March 27, 2026） | 2026-09-23 |
| DeepSeek 隐私政策用途 | 明确将 "to train and improve our technology, such as our machine learning models and algorithms" 列为处理目的；用户享有 "the right to opt-out of using your Personal Data for training our models or optimizing our technologies" | [DeepSeek Privacy Policy](https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html)（Last Update: Feb 10, 2026） | 2026-09-23 |
| DeepSeek 数据存储地 | **中华人民共和国**（"we directly collect, process and store your Personal Data in People's Republic of China"） | 同上 | 2026-09-23 |
| DeepSeek open platform 免责范围 | 隐私政策明确：通过开放平台服务开发的下游系统/应用中，终端用户的个人数据处理**不在**该隐私政策覆盖范围内；开发者作为处理控制者须自行向终端用户披露保护政策 | 同上 | 2026-09-23 |
| DeepSeek 保留期限 | 未给出固定天数；按提供服务所需、合同与法律义务、正当商业利益（含改进与开发服务、安全与稳定）以及法律主张的行使/抗辩所需保留。账户类数据在账户存续期间保留。未设具体 TTL 数字 → **未核实（无明确期限）** | 同上 | 2026-09-23 |
| DeepSeek 模型机制披露 | `https://cdn.deepseek.com/policies/en-US/model-algorithm-disclosure.html` | [Terms of Use](https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html) | 2026-09-23 |
| DeepSeek 输出权属 | 用户保留 Inputs 的权利；DeepSeek 将 Outputs 的权利转让给用户；明确允许将 Inputs/Outputs 用于个人使用、学术研究、衍生品开发、**训练其他模型（如蒸馏）** 等合法用途 | 同上 | 2026-09-23 |

> **UI 合规提示（重要）**：三家的默认数据处理姿态**不一致**，不能统一文案。
> - OpenAI / Anthropic：默认不用于训练。
> - DeepSeek：默认可用于改进服务与底层技术，需用户主动关闭 "Improve the model for everyone" 才退出。
> 桌面 Todo 应用若要处理用户私密内容，UI 应至少区分展示这三点，并对 DeepSeek 明确提示数据存储在中国境内。

### D.2 通用 SSE 流式增量解析差异要点

| 项目 | DeepSeek | OpenAI（Responses API） | Anthropic |
| --- | --- | --- | --- |
| 事件命名 | **data-only**（无 `event:` 名） | **语义化命名事件**，每个事件带 `type` | **命名事件**（`event:` 名 + data 内同名 `type`） |
| 终止标记 | `data: [DONE]` | `response.completed` 事件 | **无 `[DONE]`**，以 `message_stop` 结束 |
| 关键增量事件 | `choices[0].delta.content` | `response.output_text.delta`（取 `event.delta`） | `content_block_delta` + `delta.type == "text_delta"`（取 `delta.text`） |
| 增量语义 | 增量 | 增量 | **增量**（`2023-06-01` 起；旧 `2023-01-01` 为累积式） |
| 工具入参增量 | `tool_calls[].function.arguments` 分片字符串 | `response.function_call_arguments.delta` | `input_json_delta.partial_json` 分片字符串 |
| 心跳 / 保活 | 可能持续收到 SSE 注释行 **`: keep-alive`**（须跳过；非流式请求则表现为持续空行） | 未核实 | 可能穿插任意数量 `ping` 事件 |
| usage 位置 | 末个内容 chunk（`stream_options.include_usage` 时所有 chunk 都带 `usage`，仅最后一个非 null）；**不单独发 usage-only chunk** | 未逐一核实 | `message_start` 与 `message_delta` 均带 usage；**`message_delta` 的 usage 是累积值** |
| 流中错误 | 未核实 | `error` 事件 | `event: error` + `data: {"type":"error","error":{…}}`（HTTP 已 200） |
| 生命周期事件 | — | `response.created`、`response.output_text.delta`、`response.completed`、`error` | `message_start`、`content_block_start/delta/stop`、`message_delta`、`message_stop` |
| 未知事件容忍 | 应容忍 | 应容忍（官方保证会新增事件类型） | **必须**容忍（官方版本策略明确保留新增事件类型的权利） |

来源：[DeepSeek Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion)、[DeepSeek Rate Limit & Isolation](https://api-docs.deepseek.com/quick_start/rate_limit)、[OpenAI Streaming](https://developers.openai.com/api/docs/guides/streaming-responses)、[OpenAI API Overview](https://developers.openai.com/api/reference/overview)、[Anthropic Streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)、[Anthropic Versions](https://platform.claude.com/docs/en/api/versioning)。访问日期均为 2026-09-23。

**未核实项**：OpenAI Chat Completions（`/v1/chat/completions`）的当前 SSE 具体事件形态（是否仍为 data-only + `data: [DONE]`）未在本次官方页面中逐条确认；DeepSeek 的流中错误事件格式亦未核实。

> **适配层结论**：三家 SSE **不能共用同一个解析器**。建议抽象为 `StreamEvent` 归一化层，各自实现 parser：
> - DeepSeek parser 需处理 `: keep-alive` 注释与 `[DONE]`
> - OpenAI(Responses) parser 基于 `type` 字段分发
> - Anthropic parser 基于 `event:` 名分发，忽略未知类型，且需累积 `partial_json`

### D.3 token 计数方案（能否估算消耗）

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| **Anthropic** | **有**。`POST /v1/messages/count_tokens`，返回 `{ "input_tokens": number }` | [Token counting](https://platform.claude.com/docs/en/build-with-claude/token-counting) | 2026-09-23 |
| Anthropic count_tokens 细节 | 接受与创建消息相同的结构化输入（含 system、tools、images、PDF）；**免费**但有独立 RPM 限制（Start 5,000 / Build 10,000 / Scale 20,000），与消息创建限流相互独立 | 同上 | 2026-09-23 |
| Anthropic count_tokens 限制 | 对以下输入返回 `invalid_request_error`：server tools（web search / web fetch / code execution / tool search，advisor 工具除外）、MCP connector、`url` 或 `file` 来源的 image/document block（图片与 PDF 需以 base64 传入） | 同上 | 2026-09-23 |
| Anthropic 计数精度 | 官方明示为**估算**，实际输入 token 可能有小幅差异；计数可能包含 Anthropic 自动追加的系统优化 token，但**系统追加的 token 不计费** | 同上 | 2026-09-23 |
| Anthropic tokenizer 变更 | Claude 4.7 及之后模型与 Claude Mythos Preview 使用新 tokenizer，**同一输入文本约比早前模型多 30% token**。官方建议针对实际使用的模型重新计数，不要复用旧模型的计数结果 | 同上 | 2026-09-23 |
| Anthropic prompt caching 交互 | token 计数**不使用**缓存逻辑，仅给估算；即便请求里带 `cache_control`，缓存也只在真实消息创建时发生 | 同上 | 2026-09-23 |
| Anthropic thinking 计数 | 之前 assistant 轮次的 thinking block：在保留全部历史轮次的模型上计入输入 token；在只保留最后一轮的模型上会被 API 剥离、**不计入**。当前 assistant 轮次的 thinking **计入**输入 token | 同上 | 2026-09-23 |
| **DeepSeek** | **无 count_tokens 端点**。官方提供离线 tokenizer 压缩包 `deepseek_v4_tokenizer.zip`（`https://cdn.deepseek.com/api-docs/deepseek_v4_tokenizer.zip`）供离线计算 | [Token & Token Usage](https://api-docs.deepseek.com/quick_start/token_usage) | 2026-09-23 |
| DeepSeek 换算参考 | 1 个英文字符 ≈ 0.3 token；1 个中文字符 ≈ 0.6 token。实际以 API 返回的 usage 为准 | 同上 | 2026-09-23 |
| DeepSeek usage 字段 | `prompt_tokens`、`completion_tokens`、`total_tokens`、`prompt_cache_hit_tokens`、`prompt_cache_miss_tokens`、`prompt_tokens_details.cached_tokens`、`completion_tokens_details.reasoning_tokens` | [Chat Completions API](https://api-docs.deepseek.com/api/create-chat-completion) | 2026-09-23 |
| **OpenAI** | **未发现官方 count_tokens 端点**。本次核查的官方 API Overview、Models、Pricing、Structured Outputs、Error codes、Your data 页面均未提及该端点 → 标注为**未核实 / 疑似不提供**。token 用量以响应中的 `usage` 字段为准（如结构化输出示例中的 `usage.input_tokens` / `output_tokens` / `total_tokens` / `output_tokens_details.reasoning_tokens`） | [API Overview](https://developers.openai.com/api/reference/overview)、[Structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs) | 2026-09-23 |

> **UI「估算消耗」实现建议**：
> - Anthropic：服务端预检可精确调用 `count_tokens`（免费，注意其独立 RPM）；注意新 tokenizer 约 +30%，且需按目标模型分别计数。
> - DeepSeek：只能离线跑官方 tokenizer，或按字符比例粗估；**费用估算必须区分高峰/非高峰时段与缓存命中比例**。
> - OpenAI：无预检端点，只能依赖响应返回的 `usage` 做**事后**统计；若需事前估算，需自行集成 tokenizer（官方页面未提供该方案说明 → 未核实）。

---

## 附：一页速查（供适配层直接编码）

| 维度 | DeepSeek | OpenAI | Anthropic |
| --- | --- | --- | --- |
| Base URL | `https://api.deepseek.com` | `https://api.openai.com/v1` | `https://api.anthropic.com` |
| 对话端点 | `POST /chat/completions` | `POST /v1/responses`（推荐）/ `POST /v1/chat/completions` | `POST /v1/messages` |
| 鉴权头 | `Authorization: Bearer <key>` | `Authorization: Bearer <key>` | **`x-api-key: <key>`** |
| 必备版本头 | 无 | 无 | **`anthropic-version: 2023-06-01`** |
| 模型 ID | `deepseek-flash`、`deepseek-v4-pro` | `gpt-6-astra`、`gpt-5.6-terra`、`gpt-5.4-mini` … | `claude-fable-5-1`、`claude-opus-5-5`、`claude-sonnet-5`、`claude-haiku-4-5-20251001` |
| `max_tokens` | 可选（默认 8K/64K/128K） | 可选 | **必填** |
| 列模型 | `GET /models` | `GET /v1/models` | `GET /v1/models`（游标分页） |
| 结构化输出 | 仅 `json_object` | `text.format` / `response_format` 的 `json_schema` + `strict` | `output_config.format` 的 `json_schema`（已 GA） |
| token 计数端点 | 无（离线 tokenizer） | 未见 | `POST /v1/messages/count_tokens` |
| SSE 终止 | `data: [DONE]` | `response.completed` | `message_stop`（无 `[DONE]`） |
| API 数据训练默认 | **默认用于改进，可 opt-out** | 不用于训练（除非 opt-in） | 不用于训练 |
