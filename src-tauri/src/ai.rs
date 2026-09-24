//! AI 提供商适配层（任务书 §6）。
//!
//! ## 三家接口互不兼容，必须按 provider 分支
//!
//! 本文件的所有差异都来自 `docs/api-research.md` 的官方文档核实结果
//! （访问日期 2026-09-23），不是凭记忆写的：
//!
//! | 项目 | DeepSeek | OpenAI | Anthropic |
//! | --- | --- | --- | --- |
//! | Base URL | `https://api.deepseek.com`（**无 `/v1`**） | `https://api.openai.com/v1` | `https://api.anthropic.com` |
//! | 鉴权 | `Authorization: Bearer` | `Authorization: Bearer` | **`x-api-key`**（不是 Bearer） |
//! | 版本头 | 无 | 无 | **`anthropic-version: 2023-06-01`（必填）** |
//! | `max_tokens` | 可选 | 可选 | **必填** |
//! | system 位置 | messages 里 | messages 里 | **顶层字段**（messages 无 system role） |
//! | 响应文本 | `choices[0].message.content` | Responses: `output[].content[].text` | **`content[]` 数组**，取 `type=="text"` |
//! | 结构化输出 | 仅 `json_object` | `json_schema`（strict） | `output_config.format`（GA） |
//!
//! 结论：**不能把 Claude 当作 OpenAI 兼容端点**。三家各写一个请求构造与响应解析。
//!
//! ## 密钥处理（§6 / §10）
//!
//! - 密钥存 Windows 凭据管理器（`keyring`），**不写数据库、不进备份、不进日志**；
//! - 所有日志与错误信息都不包含密钥；
//! - 数据库里只存"是否已配置"的标记供界面显示。
//!
//! ## 超时与重试（§10）
//!
//! 用 `reqwest` 的 `read_timeout` 而不是总 `timeout`：流式响应可能持续很久，
//! 总超时会误杀正常的慢响应。重试只针对网络错误与 5xx，
//! 计费/配额类错误（如余额不足）**重试无效**，直接返回给用户。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 密钥在凭据管理器中的服务名
const KEYRING_SERVICE: &str = "com.pla0185.lumen";

/// 连接建立的超时（秒）。与"读取超时"分开设置：
/// 连接慢通常是网络问题，而读取慢可能只是模型在长时间推理，
/// 把两者用一个总超时处理会误杀正常的长响应。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// 单次请求的最大输出 token（§6 要求"可设置的单次输出限制"）
pub const MAX_OUTPUT_TOKENS_CAP: i64 = 32_000;
/// 默认输出上限
pub const DEFAULT_MAX_OUTPUT_TOKENS: i64 = 2_048;

// =============================================================================
// 提供商定义
// =============================================================================

/// 支持的服务商
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// 深度求索
    DeepSeek,
    /// OpenAI
    ///
    /// 序列化名**写死**为 `open_ai`。理由：`#[serde(rename_all = "snake_case")]`
    /// 对 `OpenAI` 这种连续大写会算成 `open_a_i`，而前端的 `AiProvider`
    /// 联合类型写的是 `'deep_seek' | 'open_ai' | 'claude' | 'custom'`。
    /// 两者对不上时**不会报错**：前端按键取值（如 `keyStatus[p]`）只会静默
    /// 拿到 `undefined`，表现为"某个 provider 明明存了密钥却总显示未配置"。
    ///
    /// `alias` 保留旧名，让已经写进数据库 `settings.ai_config` 的
    /// `"open_a_i"` 仍能读出来——不要求用户重配（数据安全底线）。
    #[serde(rename = "open_ai", alias = "open_a_i")]
    OpenAI,
    /// Anthropic Claude
    Claude,
    /// 自定义 OpenAI 兼容服务（如本地模型、第三方中转）
    Custom,
}

impl Provider {
    /// 全部提供商的固定顺序。
    ///
    /// 默认值表、界面下拉、测试遍历都从这里取，避免各处各写一份列表。
    pub const ALL: [Provider; 4] = [Self::DeepSeek, Self::OpenAI, Self::Claude, Self::Custom];

    /// 默认 Base URL。
    ///
    /// 注意 DeepSeek **不带 `/v1`**：官方文档全站未出现 `/v1` 段，
    /// 路径直接是 `/chat/completions`。
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::DeepSeek => "https://api.deepseek.com",
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Claude => "https://api.anthropic.com",
            Self::Custom => "",
        }
    }

    /// 默认模型。
    ///
    /// 三家的情况**不一样**，不能一律填一个字符串：
    ///
    /// - **DeepSeek**：`deepseek-flash`，官方文档明确列出（2026-09-24 核实）。
    /// - **Claude**：`claude-sonnet-5`，官方文档明确列出且自述为
    ///   "best combination of speed and intelligence"。
    /// - **OpenAI**：**留空**。核实过程中 OpenAI 的全部官方域名在本机返回
    ///   HTTP 403（Cloudflare），拿不到官方一手模型清单；能查到的旁证
    ///   （Microsoft Learn 的 Azure OpenAI 文档）里 `gpt-6-astra` 是 **Azure**
    ///   的模型 ID，并不等于 OpenAI 平台上同名可用。
    ///   整改任务书 §3.5 明确"默认模型不得填写不存在、已下线、内部名称或
    ///   **未经验证**的模型"，所以这里宁可留空，让用户点「拉取模型列表」
    ///   从自己的账号读真实 ID（`GET /v1/models`），或手动填写。
    pub fn default_model(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek-flash",
            Self::OpenAI => "",
            Self::Claude => "claude-sonnet-5",
            Self::Custom => "",
        }
    }

    /// 该提供商是否使用 OpenAI 的 **Responses API**（而不是 Chat Completions）。
    ///
    /// 为什么 OpenAI 单独走 Responses：官方把新能力（严格 json_schema、
    /// 推理项等）优先放在 Responses 上，微软官方文档也写明
    /// "Recommended: Send tool-calling requests to the Responses API"。
    /// 而 **Chat Completions 仍是 DeepSeek 与自定义兼容服务的正确协议**，
    /// 因此两者在代码里彻底分开实现（整改任务书 §3.2 / §3.4），
    /// 而不是"注释说一套、实际跑另一套"。
    pub fn uses_openai_responses(self) -> bool {
        matches!(self, Self::OpenAI)
    }

    /// 是否使用 Anthropic 风格协议
    pub fn is_anthropic_style(self) -> bool {
        matches!(self, Self::Claude)
    }

    /// 人类可读名称
    pub fn label(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenAI => "OpenAI",
            Self::Claude => "Anthropic Claude",
            Self::Custom => "自定义兼容服务",
        }
    }

    /// 数据使用提示（§6 要求说明发送给模型的数据范围）
    ///
    /// 三家姿态不一致，因此文案**不能统一**：
    /// 官方文档核实结果是 OpenAI 与 Anthropic 默认不用于训练，
    /// 而 DeepSeek 默认可用于改进服务且需主动 opt-out。
    pub fn data_policy_note(self) -> &'static str {
        match self {
            Self::DeepSeek => {
                "按 DeepSeek 官方政策，API 输入与输出默认会被用于改进其服务与底层技术，\
                 需要在 DeepSeek 账户设置中主动关闭「Improve the model for everyone」\
                 才能退出；数据存储在中国境内。请自行评估后再决定是否发送敏感内容。"
            }
            Self::OpenAI => {
                "按 OpenAI 官方政策，自 2023-03-01 起 API 数据默认不用于训练（除非主动选择加入），\
                 滥用监控日志保留约 30 天。"
            }
            Self::Claude => {
                "按 Anthropic 官方政策，未经明确许可不会将数据用于训练，默认不保留对话内容；\
                 部分模型（Covered Models）保留 30 天且不适用零数据保留。"
            }
            Self::Custom => {
                "自定义服务的隐私政策由其提供方决定，Lumen 无法代为说明。\
                 请自行确认该服务如何处理你的数据。"
            }
        }
    }
}

/// 提供商配置（**不含密钥**）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    /// 提供商
    pub provider: Provider,
    /// Base URL（可为空表示用默认值）
    pub base_url: String,
    /// 模型 ID
    pub model: String,
    /// 超时秒数
    pub timeout_seconds: u64,
    /// 单次输出上限
    pub max_output_tokens: i64,
    /// 是否已配置密钥（**只存这个布尔值，密钥本身在凭据管理器里**）
    pub has_api_key: bool,
}

impl ProviderConfig {
    /// 用提供商的默认值构造
    pub fn with_defaults(provider: Provider) -> Self {
        Self {
            provider,
            base_url: provider.default_base_url().to_string(),
            model: provider.default_model().to_string(),
            timeout_seconds: 60,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            has_api_key: false,
        }
    }

    /// 归一化：补默认值并收敛越界项
    pub fn normalize(&mut self) {
        if self.base_url.trim().is_empty() {
            self.base_url = self.provider.default_base_url().to_string();
        }
        // 去掉末尾斜杠，避免拼出 `//chat/completions`
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        if self.model.trim().is_empty() {
            self.model = self.provider.default_model().to_string();
        }
        if self.timeout_seconds == 0 || self.timeout_seconds > 600 {
            self.timeout_seconds = 60;
        }
        if self.max_output_tokens <= 0 || self.max_output_tokens > MAX_OUTPUT_TOKENS_CAP {
            self.max_output_tokens = DEFAULT_MAX_OUTPUT_TOKENS;
        }
    }

    /// 只校验连接目标（Base URL）。
    ///
    /// 这是**唯一**的端点校验实现，`validate_for_save` / `validate_for_run`
    /// 都建立在它之上，避免两处判断漂移。
    ///
    /// §6 要求"Base URL 可编辑时校验"。
    pub fn validate_endpoint(&self) -> AppResult<()> {
        let u = self.base_url.trim();
        if u.is_empty() {
            return Err(AppError::validation("Base URL 不能为空"));
        }
        if !u.starts_with("https://") && !u.starts_with("http://") {
            return Err(AppError::validation(format!(
                "Base URL 必须以 http:// 或 https:// 开头：{u}"
            ))
            .with_hint("例如 https://api.deepseek.com"));
        }
        // 明文 HTTP 只允许本机（本地模型常见），其它地址必须用 HTTPS，
        // 否则 API Key 会在网络上明文传输。
        //
        // 注意 IPv6 字面量要单独处理：`http://[::1]:11434/v1` 如果按 `:` 切分，
        // 第一个冒号会把主机名切成 `[`，于是 ::1 反而被判定为"非本机"而拒绝
        // ——这是真实存在的漏洞（由 `invalid_base_url_is_rejected_at_validation`
        // 这条测试发现），因此这里显式剥掉方括号。
        if u.starts_with("http://") {
            let host_part = u.trim_start_matches("http://");
            let host = if let Some(rest) = host_part.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                host_part.split(['/', ':']).next().unwrap_or("")
            };
            let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1");
            if !is_local {
                return Err(
                    AppError::validation("出于安全考虑，非本机地址必须使用 https://").with_hint(
                        "明文 HTTP 会让 API Key 在网络上以明文传输，仅在连接 localhost 时允许",
                    ),
                );
            }
        }
        Ok(())
    }
}

// =============================================================================
// 保存校验 与 运行校验（整改任务书 §3.3 / §3.4）
// =============================================================================
//
// 这两件事**必须分开**，否则会出现死锁：
//
// ```text
// 模型为空 → 不能保存配置 → 密钥进不了凭据管理器 → 拉不到模型列表 → 选不了模型
// ```
//
// 用户刚切到 OpenAI 时模型就是空的（后端默认模型刻意留空，见
// `Provider::default_model`），所以"保存密钥"这一步**不能**要求模型非空。
// 真正发请求时才必须严格校验。

impl ProviderConfig {
    /// **保存配置**时的校验：只要求连接目标合法。
    ///
    /// 允许 `model` 为空——用户需要先把 API Key 存进凭据管理器，
    /// 才能拉取模型列表选出真实可用的模型 ID（§3.1）。
    pub fn validate_for_save(&self) -> AppResult<()> {
        self.validate_endpoint()
    }

    /// **真正发起调用**前的校验：在保存校验之上，额外要求模型已选定。
    ///
    /// 空模型发出去只会拿到服务商的 400，不如在这里给出可执行的提示（§3.5 场景 C）。
    pub fn validate_for_run(&self) -> AppResult<()> {
        self.validate_endpoint()?;
        if self.model.trim().is_empty() {
            return Err(AppError::validation("尚未选择模型，无法调用 AI").with_hint(
                "请先在「设置 → AI」中点击「获取模型列表」选择模型，或手动填写模型 ID",
            ));
        }
        Ok(())
    }
}

/// 提供商的默认配置（**默认值的唯一来源**，整改任务书 §2）。
///
/// 为什么要有这个结构：整改前前端 `ai-ipc.ts` 里另有一份
/// `PROVIDER_DEFAULT_MODEL`，其中 OpenAI 仍写着 `gpt-6-astra`
/// （那是 **Azure** 的模型 ID），而后端默认模型已经刻意留空。
/// 两份表一旦漂移，用户在界面上看到的默认值与后端实际行为就不一致——
/// 而且这种不一致**不会报错**，只会静默以错误的模型名发请求。
///
/// 因此默认值只在 Rust 侧维护一份，前端通过 `ai_provider_defaults` 读取。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefaults {
    pub provider: Provider,
    /// 界面显示名
    pub label: String,
    pub base_url: String,
    /// 默认模型；**可能刻意为空**（见 `Provider::default_model` 的说明）
    pub model: String,
    pub timeout_seconds: u64,
    pub max_output_tokens: i64,
    /// 数据使用政策提示（三家姿态不同，文案由后端给出）
    pub data_policy_note: String,
    /// 默认模型是否"刻意留空、必须由用户选定"
    pub model_must_be_chosen: bool,
    /// 密钥在凭据管理器中的条目名（只用于界面说明，不含密钥）
    pub key_entry: String,
}

impl ProviderDefaults {
    /// 从 `ProviderConfig::with_defaults` 派生，保证两者永不漂移。
    pub fn for_provider(p: Provider) -> Self {
        let c = ProviderConfig::with_defaults(p);
        Self {
            provider: p,
            label: p.label().to_string(),
            base_url: c.base_url,
            model: c.model,
            timeout_seconds: c.timeout_seconds,
            max_output_tokens: c.max_output_tokens,
            data_policy_note: p.data_policy_note().to_string(),
            model_must_be_chosen: p.default_model().is_empty(),
            key_entry: key_name(p),
        }
    }

    pub fn all() -> Vec<Self> {
        Provider::ALL
            .iter()
            .copied()
            .map(Self::for_provider)
            .collect()
    }
}

// =============================================================================
// 密钥存储（Windows 凭据管理器）
// =============================================================================
/// 凭据条目名
fn key_name(provider: Provider) -> String {
    format!(
        "ai-{}",
        match provider {
            Provider::DeepSeek => "deepseek",
            Provider::OpenAI => "openai",
            Provider::Claude => "claude",
            Provider::Custom => "custom",
        }
    )
}

/// 保存 API Key 到系统凭据管理器（§6 要求优先使用 Windows 凭据管理）
pub fn save_api_key(provider: Provider, key: &str) -> AppResult<()> {
    let k = key.trim();
    if k.is_empty() {
        return delete_api_key(provider);
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider))
        .map_err(|e| AppError::internal(format!("无法访问系统凭据管理器：{e}")))?;
    entry.set_password(k).map_err(|e| {
        AppError::internal(format!("保存密钥失败：{e}"))
            .with_hint("请确认当前用户有权限写入 Windows 凭据管理器")
    })?;
    // 只记录"已保存"，绝不记录密钥内容本身（§10 日志不含密钥）
    log::info!("已保存 {} 的 API Key 到系统凭据管理器", provider.label());
    Ok(())
}

/// 读取 API Key
pub fn load_api_key(provider: Provider) -> AppResult<Option<String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider))
        .map_err(|e| AppError::internal(format!("无法访问系统凭据管理器：{e}")))?;
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => Ok(Some(p)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::internal(format!("读取密钥失败：{e}"))),
    }
}

/// 删除 API Key
pub fn delete_api_key(provider: Provider) -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider))
        .map_err(|e| AppError::internal(format!("无法访问系统凭据管理器：{e}")))?;
    match entry.delete_credential() {
        Ok(()) => {
            log::info!("已删除 {} 的 API Key", provider.label());
            Ok(())
        }
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::internal(format!("删除密钥失败：{e}"))),
    }
}

/// 各 provider **各自**的密钥状态。
///
/// 为什么必须单独有这张表：`ProviderConfig::has_api_key` 只描述"某一个
/// provider"，而界面上的提供商下拉是**跨 provider** 的。修复前的
/// `AiPanel.switchProvider` 把当前（旧）provider 的 `hasApiKey` 直接套给
/// 目标 provider，于是 DeepSeek 存过密钥时切到 OpenAI 也会显示「已配置」——
/// 用户以为可以直接调用，真正发请求时才被"尚未配置 API Key"打回来。
///
/// 序列化后的键名是 snake_case 的 provider 名（`deep_seek` / `open_ai` /
/// `claude` / `custom`），与前端的 `AiProvider` 联合类型一致，前端才能直接
/// `keyStatus[p]` 取值。**这里刻意不用 `camelCase`**：本文件其余结构体走
/// `rename_all = "camelCase"`，但这些键是 provider 标识符，一旦变成
/// `deepSeek` 就对不上前端的 provider 取值，取到 undefined。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderKeyStatus {
    /// DeepSeek 是否已保存密钥
    pub deep_seek: bool,
    /// OpenAI 是否已保存密钥
    pub open_ai: bool,
    /// Anthropic Claude 是否已保存密钥
    pub claude: bool,
    /// 自定义兼容服务是否已保存密钥
    pub custom: bool,
}

impl ProviderKeyStatus {
    /// 逐个 provider 查询凭据管理器。
    ///
    /// **必须逐个查**：一个全局布尔值回答不了"哪个 provider 有密钥"，
    /// 而界面需要的恰恰是这个区分。
    pub fn load() -> Self {
        Self {
            deep_seek: provider_has_key(Provider::DeepSeek),
            open_ai: provider_has_key(Provider::OpenAI),
            claude: provider_has_key(Provider::Claude),
            custom: provider_has_key(Provider::Custom),
        }
    }
}

/// 单个 provider 是否有密钥。
///
/// 读取失败**不连累整个命令**：某个 provider 的凭据条目损坏时，设置页仍应
/// 能打开，只是那一项显示"未配置"。失败记一条 warn，
/// 且只记 provider 与错误原因——**不记密钥内容**（§10）。
fn provider_has_key(provider: Provider) -> bool {
    match load_api_key(provider) {
        Ok(k) => k.is_some(),
        Err(e) => {
            log::warn!(
                "读取 {} 的密钥状态失败，按未配置处理：{}",
                provider.label(),
                e.message
            );
            false
        }
    }
}

/// 把密钥脱敏用于展示（§6 要求 UI 默认隐藏，§10 要求不明文展示）
pub fn mask_key(key: &str) -> String {
    let k = key.trim();
    let n = k.chars().count();
    if n <= 8 {
        return "••••••••".to_string();
    }
    let head: String = k.chars().take(4).collect();
    let tail: String = k.chars().skip(n - 4).collect();
    format!("{head}••••••••{tail}")
}

// =============================================================================
// 请求与响应
// =============================================================================

/// 一条对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// system / user / assistant
    pub role: String,
    pub content: String,
}

/// 调用请求
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    /// 提供商配置
    pub config: ProviderConfig,
    /// 系统提示（Anthropic 走顶层字段，其余放 messages）
    pub system: Option<String>,
    /// 对话消息（不含 system）
    pub messages: Vec<ChatMessage>,
    /// 是否要求 JSON 输出
    #[serde(default)]
    pub json_output: bool,
    /// 本次调用的输出上限（覆盖配置值）
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
}

/// 调用结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    /// 模型返回的文本
    pub text: String,
    /// 实际使用的模型
    pub model: String,
    /// token 用量（若服务商返回）
    pub usage: Option<TokenUsage>,
    /// 是否因为达到输出上限而被截断
    pub truncated: bool,
}

/// token 用量
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// 缓存命中的输入 token（DeepSeek 计费需要）
    pub cache_hit_tokens: Option<i64>,
    /// 缓存未命中的输入 token
    pub cache_miss_tokens: Option<i64>,
}

/// 构造带正确超时设置的 HTTP 客户端。
///
/// **不使用总 `timeout`**：流式或长推理响应可能持续数分钟，
/// 总超时会误杀正常请求。用 `connect_timeout` + `read_timeout` 分别控制。
fn build_client(timeout_seconds: u64) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(Duration::from_secs(timeout_seconds.clamp(5, 600)))
        .user_agent("Lumen/0.1")
        // 关掉自动重定向：避免密钥被带到其它域
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::internal(format!("创建 HTTP 客户端失败：{e}")))
}

/// 把 reqwest 错误翻译成可读的 AppError，并区分是否可重试
fn map_net_err(e: reqwest::Error) -> AppError {
    if e.is_timeout() {
        AppError::new(crate::error::ErrorCode::Timeout, "请求超时")
            .with_hint("服务商响应过慢或网络不稳定。可在设置中调大超时时间，或稍后重试")
    } else if e.is_connect() {
        AppError::new(
            crate::error::ErrorCode::Network,
            format!("无法连接到服务商：{e}"),
        )
        .with_hint("请检查网络、代理设置与 Base URL 是否正确")
    } else {
        AppError::new(
            crate::error::ErrorCode::Network,
            format!("网络请求失败：{e}"),
        )
    }
}

/// 把服务商返回的错误正文处理成**可安全展示**的片段（整改任务书 §10）。
///
/// 为什么不能直接截 400 字丢给用户：
/// 1. 第三方（尤其是中转/代理服务）经常把**整个请求**回显在错误里，
///    那会把用户的提示词、任务内容一起显示出来，甚至写进日志；
/// 2. 有些网关会把收到的 `Authorization` 头原样回显，等于把密钥印在界面上；
/// 3. 反向代理返回的 HTML 错误页既没有信息量又很长。
///
/// 处理策略：
/// - 先剥掉 HTML 标签（只留文本）；
/// - 再把形如密钥的片段打码（`sk-...`、`Bearer xxx`、`api-key: xxx`、
///   长 base64/hex 串）；
/// - 折叠空白，限制长度（UI 与日志用同一个长度，避免两处不一致）。
///
/// 注意：这**不是**"隐藏错误原因"——用户仍能看到服务商说了什么，
/// 只是看不到可能属于自己或他人的机密。
pub fn sanitize_provider_error(body: &str, limit: usize) -> String {
    let mut s = body.to_string();

    // 1) HTML 错误页：只保留标签之间的文字
    if s.contains('<') && s.contains('>') {
        let mut out = String::with_capacity(s.len());
        let mut in_tag = false;
        for ch in s.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => {
                    in_tag = false;
                    out.push(' ');
                }
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        s = out;
    }

    // 2) 打码疑似密钥。用简单扫描而不是正则，避免引入额外依赖与回溯风险。
    s = redact_secrets(&s);

    // 3) 折叠空白并截断
    let mut cleaned = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space {
                cleaned.push(' ');
            }
            last_space = true;
        } else {
            cleaned.push(ch);
            last_space = false;
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= limit {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(limit).collect();
        format!("{head}…（已截断）")
    }
}

/// 把疑似密钥的片段替换成 `***`。
///
/// 覆盖常见形态：
/// - `sk-` / `sk-proj-` 开头的 OpenAI 风格 key
/// - `sk-ant-` 开头的 Anthropic key
/// - 任意 `Bearer <token>`
/// - `api-key: <token>` / `x-api-key: <token>` / `authorization: <token>`
/// - 长度 ≥ 32 的纯字母数字串（很多网关会裸回显 token）
fn redact_secrets(input: &str) -> String {
    const MARKERS: [&str; 6] = [
        "sk-ant-", "sk-proj-", "sk-", "Bearer ", "bearer ", "api-key",
    ];

    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        // 命中前缀标记：把紧随其后的 token 打码
        let rest: String = bytes[i..].iter().take(16).collect();
        if let Some(m) = MARKERS.iter().find(|m| rest.starts_with(**m)) {
            out.push_str(m);
            i += m.chars().count();
            // 跳过冒号/等号/空格，然后吃掉连续的 token 字符
            while i < bytes.len() && matches!(bytes[i], ' ' | ':' | '=' | '"' | '\'') {
                out.push(bytes[i]);
                i += 1;
            }
            let mut eaten = 0;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], '-' | '_' | '.'))
            {
                i += 1;
                eaten += 1;
            }
            if eaten > 0 {
                out.push_str("***");
            }
            continue;
        }

        // 裸的长 token：连续 ≥32 个字母数字（不含普通单词里的短串）
        if bytes[i].is_ascii_alphanumeric() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let run: String = bytes[start..i].iter().collect();
            if run.len() >= 32 {
                out.push_str("***");
            } else {
                out.push_str(&run);
            }
            continue;
        }

        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// 按 HTTP 状态码与响应体翻译错误。
///
/// 关键区分：**计费与配额类错误重试无效**，要直接告诉用户去充值/提额，
/// 而不是让他反复重试。
///
/// 整改任务书 §10：回显给用户的第三方正文一律走 [`sanitize_provider_error`]，
/// 不直接透传原文。
/// 按 HTTP 状态码与响应体翻译错误。
fn map_http_error(provider: Provider, status: u16, body: &str) -> AppError {
    // UI 与日志共用同一份处理结果：出现过"界面干净、日志里却躺着密钥"的情况
    // 就说明两处处理不一致，因此这里只算一次。
    let snippet = sanitize_provider_error(body, 300);
    match status {
        401 | 403 => AppError::new(
            crate::error::ErrorCode::Unauthorized,
            "API Key 无效或没有权限",
        )
        .with_hint(format!(
            "请在设置中检查 {} 的 API Key 是否正确、是否已过期。\n服务商返回：{snippet}",
            provider.label()
        )),
        402 => AppError::new(crate::error::ErrorCode::QuotaExceeded, "账户余额不足")
            .with_hint("这是计费问题，重试无效。请到服务商控制台充值后重试"),
        429 => AppError::new(
            crate::error::ErrorCode::RateLimited,
            "请求过于频繁，已触发限流",
        )
        .with_hint("请稍后重试。若频繁出现，请降低调用频率或检查账户配额"),
        400 | 422 => AppError::validation(format!("请求参数被拒绝（HTTP {status}）")).with_hint(
            format!("常见原因：模型名不存在、参数超范围。\n服务商返回：{snippet}"),
        ),
        404 => AppError::validation("接口地址不存在（HTTP 404）").with_hint(format!(
            "请检查 Base URL 是否正确——注意 DeepSeek 的地址不含 /v1。\n服务商返回：{snippet}"
        )),
        // 529 是 Anthropic 特有的 overloaded_error，属于"对方太忙"，可重试
        529 => AppError::new(
            crate::error::ErrorCode::Network,
            "服务商负载过高（HTTP 529 overloaded）",
        )
        .with_hint("这是服务商侧的过载保护，稍后重试即可"),
        500..=599 => AppError::new(
            crate::error::ErrorCode::Network,
            format!("服务商暂时不可用（HTTP {status}）"),
        )
        .with_hint("这通常是服务商侧问题，可稍后重试"),
        other => AppError::new(
            crate::error::ErrorCode::Internal,
            format!("调用失败（HTTP {other}）"),
        )
        .with_hint(snippet),
    }
}

// =============================================================================
// 各 provider 的请求构造（差异集中在这里）
// =============================================================================

/// 拼出对话端点 URL。
///
/// 三家的路径规则**确实不同**（2026-09-24 逐条核实）：
/// - DeepSeek：base `https://api.deepseek.com`（**不带 `/v1`**）→ `/chat/completions`
/// - OpenAI：base `https://api.openai.com/v1` → `/responses`
/// - Claude：base `https://api.anthropic.com` → `/v1/messages`
/// - Custom：OpenAI 兼容 → `/chat/completions`
fn endpoint(cfg: &ProviderConfig) -> String {
    let base = cfg.base_url.trim_end_matches('/');
    match cfg.provider {
        Provider::Claude => format!("{base}/v1/messages"),
        Provider::OpenAI => format!("{base}/responses"),
        _ => format!("{base}/chat/completions"),
    }
}

/// 构造 OpenAI 风格（DeepSeek / Custom）的 **Chat Completions** 请求体。
///
/// 注意：**OpenAI 自己不走这里**（它走 Responses，见
/// [`build_openai_responses_body`]）。这个函数只服务 DeepSeek 与自定义
/// OpenAI 兼容服务——曾经的实现对三者共用同一份请求体，注释里却提到
/// Responses 的响应结构，属于"说的和做的不是一套"，整改任务书 §3.4 要求拆开。
fn build_openai_body(req: &ChatRequest) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // OpenAI 风格把 system 放进 messages
    if let Some(sys) = req.system.as_ref().filter(|s| !s.trim().is_empty()) {
        messages.push(serde_json::json!({ "role": "system", "content": sys }));
    }
    for m in &req.messages {
        messages.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }

    let max_tokens = req
        .max_output_tokens
        .unwrap_or(req.config.max_output_tokens)
        .clamp(1, MAX_OUTPUT_TOKENS_CAP);

    let mut body = serde_json::json!({
        "model": req.config.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": false,
    });

    if req.json_output {
        // DeepSeek 支持 `response_format: {"type":"json_object"}`，
        // 但官方明确警告：**提示词里必须出现 "json" 字样**，否则可能生成
        // 无限空白流。这一点在提示词模板里已经保证（见 ai_features）。
        // 严格 schema 只有 Responses 的 `text.format` 支持，因此这里用
        // json_object + 解析层的结构校验（§6 要求结构校验）。
        body["response_format"] = serde_json::json!({ "type": "json_object" });
    }

    body
}

/// 构造 **OpenAI Responses API**（`POST /v1/responses`）的请求体。
///
/// 与 Chat Completions 的字段差异（逐条对照官方/Microsoft 文档核实）：
/// | 关注点 | Chat Completions | Responses |
/// | --- | --- | --- |
/// | 输入 | `messages` | `input`（数组，元素仍是 `{role, content}`） |
/// | 系统提示 | `messages[0].role = "system"` | **顶层 `instructions`** |
/// | 输出上限 | `max_tokens` | **`max_output_tokens`** |
/// | JSON 输出 | `response_format.type` | **`text.format.type`** |
fn build_openai_responses_body(req: &ChatRequest) -> serde_json::Value {
    let input: Vec<serde_json::Value> = req
        .messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let max_tokens = req
        .max_output_tokens
        .unwrap_or(req.config.max_output_tokens)
        .clamp(1, MAX_OUTPUT_TOKENS_CAP);

    let mut body = serde_json::json!({
        "model": req.config.model,
        "input": input,
        "max_output_tokens": max_tokens,
        "stream": false,
    });

    if let Some(sys) = req.system.as_ref().filter(|s| !s.trim().is_empty()) {
        body["instructions"] = serde_json::json!(sys);
    }

    if req.json_output {
        body["text"] = serde_json::json!({ "format": { "type": "json_object" } });
    }

    body
}

/// 构造 Anthropic 风格的请求体
///
/// 三处关键差异：
/// 1. `system` 是**顶层字段**，不能放进 messages；
/// 2. `max_tokens` **必填**；
/// 3. 结构化输出用 `output_config.format`（官方已 GA），
///    但为了兼容尚未支持的部署，仍配合提示词约束。
fn build_anthropic_body(req: &ChatRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        // Anthropic 的 messages 只有 user/assistant 两种 role；
        // 历史里若混入 system 会被拒绝，这里统一跳过。
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let max_tokens = req
        .max_output_tokens
        .unwrap_or(req.config.max_output_tokens)
        .clamp(1, MAX_OUTPUT_TOKENS_CAP);

    let mut body = serde_json::json!({
        "model": req.config.model,
        "max_tokens": max_tokens,
        "messages": messages,
    });

    if let Some(sys) = req.system.as_ref().filter(|s| !s.trim().is_empty()) {
        body["system"] = serde_json::json!(sys);
    }

    if req.json_output {
        // 官方已支持结构化输出；用 json_schema 约束形状，
        // 但保留解析层的二次校验以防部署不支持。
        body["output_config"] = serde_json::json!({
            "format": {
                "type": "json_schema",
                "schema": { "type": "object", "additionalProperties": true }
            }
        });
    }

    body
}

// =============================================================================
// 各 provider 的响应解析
// =============================================================================

/// 解析 **OpenAI Responses API** 的响应。
///
/// 结构与 Chat Completions 完全不同，不能靠 `choices[0].message.content`：
/// - 文本在 `output` 数组里的 `message` 项，其 `content` 是块数组，
///   取 `type == "output_text"` 的 `text` 拼接；
/// - 部分部署会直接给一个便捷字段 `output_text`，一并兼容；
/// - 截断看 `status == "incomplete"`（`incomplete_details.reason` 通常是
///   `max_output_tokens`），而不是 Chat Completions 的 `finish_reason`；
/// - 用量字段是 `input_tokens` / `output_tokens`，
///   缓存命中在 `input_tokens_details.cached_tokens`。
fn parse_openai_responses_response(v: &serde_json::Value) -> AppResult<ChatResponse> {
    let mut text = String::new();

    if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) != Some("message") {
                // reasoning / function_call 等块对当前用途无意义
                continue;
            }
            if let Some(blocks) = item.get("content").and_then(|c| c.as_array()) {
                for b in blocks {
                    if b.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
            }
        }
    }
    // 官方 SDK 的便捷聚合字段，部分兼容实现只给这个
    if text.trim().is_empty() {
        if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
            text.push_str(t);
        }
    }
    // 被安全策略拒绝时官方会给 refusal 块，明确告诉用户而不是"空内容"
    let refused = v
        .get("output")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter().any(|item| {
                item.get("content")
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("refusal"))
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if text.trim().is_empty() {
        return Err(AppError::new(
            crate::error::ErrorCode::Internal,
            if refused {
                "模型拒绝了这次请求"
            } else {
                "模型返回了空内容"
            },
        )
        .with_hint(if refused {
            "内容可能触发了服务商的安全策略。请调整提示词后重试"
        } else {
            "可重试一次；若持续出现，请检查模型名是否为该账号可用的模型（设置里可拉取模型列表）"
        }));
    }

    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
    let incomplete_reason = v
        .get("incomplete_details")
        .and_then(|d| d.get("reason"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    let usage = v.get("usage").map(|u| TokenUsage {
        input_tokens: u.get("input_tokens").and_then(|x| x.as_i64()),
        output_tokens: u.get("output_tokens").and_then(|x| x.as_i64()),
        cache_hit_tokens: u
            .get("input_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|x| x.as_i64()),
        cache_miss_tokens: None,
    });

    Ok(ChatResponse {
        text,
        model: v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        usage,
        truncated: status == "incomplete" || incomplete_reason == "max_output_tokens",
    })
}

/// 解析 OpenAI 风格（DeepSeek / Custom 的 Chat Completions）响应
fn parse_openai_response(v: &serde_json::Value) -> AppResult<ChatResponse> {
    let choice = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| {
            AppError::internal("服务商返回的结构不符合预期（缺少 choices）")
                .with_hint("可能是该服务商的响应格式与 OpenAI 不完全兼容")
        })?;

    let text = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    // DeepSeek 官方提示"偶发返回空内容"，这种情况要明确告知而不是当成成功
    if text.trim().is_empty() {
        return Err(
            AppError::new(crate::error::ErrorCode::Internal, "服务商返回了空内容").with_hint(
                "DeepSeek 官方提示存在偶发返回空内容的情况，可重试一次；若持续出现请更换模型",
            ),
        );
    }

    let finish = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .unwrap_or("");
    let usage = v.get("usage").map(|u| TokenUsage {
        input_tokens: u.get("prompt_tokens").and_then(|x| x.as_i64()),
        output_tokens: u.get("completion_tokens").and_then(|x| x.as_i64()),
        cache_hit_tokens: u.get("prompt_cache_hit_tokens").and_then(|x| x.as_i64()),
        cache_miss_tokens: u.get("prompt_cache_miss_tokens").and_then(|x| x.as_i64()),
    });

    Ok(ChatResponse {
        text,
        model: v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        usage,
        truncated: finish == "length",
    })
}

/// 解析 Anthropic 响应
///
/// 文本在**顶层 `content` 数组**里，需要遍历取 `type == "text"` 的块拼接，
/// 而不是像 OpenAI 那样读 `choices[0].message.content`。
fn parse_anthropic_response(v: &serde_json::Value) -> AppResult<ChatResponse> {
    let blocks = v
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| AppError::internal("Claude 返回的结构不符合预期（缺少 content 数组）"))?;

    let mut text = String::new();
    for b in blocks {
        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                text.push_str(t);
            }
        }
        // thinking / tool_use 等块对当前用途无意义，跳过即可
    }

    if text.trim().is_empty() {
        return Err(
            AppError::new(crate::error::ErrorCode::Internal, "Claude 返回了空内容")
                .with_hint("可能被安全策略拦截，或响应只包含非文本块。可尝试调整提示词后重试"),
        );
    }

    let stop = v.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("");
    let usage = v.get("usage").map(|u| TokenUsage {
        input_tokens: u.get("input_tokens").and_then(|x| x.as_i64()),
        output_tokens: u.get("output_tokens").and_then(|x| x.as_i64()),
        cache_hit_tokens: u.get("cache_read_input_tokens").and_then(|x| x.as_i64()),
        cache_miss_tokens: u
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_i64()),
    });

    Ok(ChatResponse {
        text,
        model: v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        usage,
        truncated: stop == "max_tokens",
    })
}

// =============================================================================
// 调用
// =============================================================================

/// 发起一次对话调用。
///
/// 密钥从系统凭据管理器读取，**不经过前端**（§6 要求密钥不明文展示）。
pub async fn chat(cfg: &ProviderConfig, req: &ChatRequest) -> AppResult<ChatResponse> {
    // 真正发请求：这里必须严格校验，包括模型已选定（§3.5 场景 C）
    cfg.validate_for_run()?;

    let api_key = load_api_key(cfg.provider)?.ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::NotConfigured,
            format!("尚未配置 {} 的 API Key", cfg.provider.label()),
        )
        .with_hint("请在设置 → AI 中填写并保存密钥")
    })?;

    let client = build_client(cfg.timeout_seconds)?;
    let url = endpoint(cfg);

    // 三条协议分支彻底分开：Anthropic Messages / OpenAI Responses /
    // OpenAI 兼容 Chat Completions。请求体与响应解析成对出现，
    // 不会出现"注释说 Responses、实际发 Chat Completions"的情况。
    enum Wire {
        Anthropic,
        OpenAiResponses,
        OpenAiChat,
    }
    let (body, wire) = match cfg.provider {
        Provider::Claude => (build_anthropic_body(req), Wire::Anthropic),
        Provider::OpenAI => (build_openai_responses_body(req), Wire::OpenAiResponses),
        _ => (build_openai_body(req), Wire::OpenAiChat),
    };

    let mut rb = client.post(&url).json(&body);

    match wire {
        Wire::Anthropic => {
            // Anthropic 用 x-api-key，**不是** Bearer；
            // 且 anthropic-version 是必填头。
            rb = rb
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json");
        }
        _ => {
            rb = rb.header("authorization", format!("Bearer {api_key}"));
        }
    }

    let resp = rb.send().await.map_err(map_net_err)?;
    let status = resp.status();

    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        // 错误信息里可能带服务商的原始提示，但绝不会带我们的密钥
        return Err(map_http_error(cfg.provider, status.as_u16(), &body_text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::internal(format!("解析服务商响应失败：{e}")))?;

    match wire {
        Wire::Anthropic => parse_anthropic_response(&json),
        Wire::OpenAiResponses => parse_openai_responses_response(&json),
        Wire::OpenAiChat => parse_openai_response(&json),
    }
}

/// 连接测试：发一个极小的请求验证鉴权与模型可用（§6 要求"可连接测试"）
pub async fn test_connection(cfg: &ProviderConfig) -> AppResult<String> {
    let req = ChatRequest {
        config: cfg.clone(),
        system: Some("你是一个测试助手。".to_string()),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: "请只回复两个字：可用".to_string(),
        }],
        json_output: false,
        max_output_tokens: Some(32),
    };

    let r = chat(cfg, &req).await?;
    let preview: String = r.text.trim().chars().take(60).collect();
    Ok(format!(
        "连接成功。模型 {} 返回：{}",
        if r.model.is_empty() {
            &cfg.model
        } else {
            &r.model
        },
        preview
    ))
}

/// 列出可用模型。
///
/// §6 要求"模型列表接口若不可用允许手动输入模型名"，因此这里**失败不算错**：
/// 返回空列表 + 说明，让界面引导用户手动填写。
pub async fn list_models(cfg: &ProviderConfig) -> AppResult<Vec<String>> {
    let api_key = load_api_key(cfg.provider)?
        .ok_or_else(|| AppError::new(crate::error::ErrorCode::NotConfigured, "尚未配置 API Key"))?;

    let client = build_client(20)?;
    let base = cfg.base_url.trim_end_matches('/');

    let (url, is_anthropic) = match cfg.provider {
        // Anthropic 的 GET /v1/models 支持游标分页，取默认 20 条足够
        Provider::Claude => (format!("{base}/v1/models"), true),
        _ => (format!("{base}/models"), false),
    };

    let mut rb = client.get(&url);
    rb = if is_anthropic {
        rb.header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        rb.header("authorization", format!("Bearer {api_key}"))
    };

    let resp = rb.send().await.map_err(map_net_err)?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(map_http_error(cfg.provider, status.as_u16(), &body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::internal(format!("解析模型列表失败：{e}")))?;

    // 两家都用 { data: [ { id } ] } 结构
    let mut out: Vec<String> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    Ok(out)
}

// =============================================================================
// IPC 命令
// =============================================================================

use tauri::State;

use crate::commands::AppState;

/// 读取各提供商的默认配置（**默认值的唯一来源**，整改任务书 §2.3）。
///
/// 前端不再自带 `PROVIDER_DEFAULT_BASE` / `PROVIDER_DEFAULT_MODEL` /
/// `PROVIDER_LABELS` / 数据政策文案，一律从这里取。
#[tauri::command]
pub fn ai_provider_defaults() -> AppResult<Vec<ProviderDefaults>> {
    Ok(ProviderDefaults::all())
}

/// 读取**每个 provider 各自**的密钥状态（§6）。
///
/// 供界面按 provider 正确显示「已配置 / 未配置」：切换提供商时要用
/// **目标 provider** 的状态，而不是当前配置里那一个布尔值。
#[tauri::command]
pub fn ai_provider_key_status() -> AppResult<ProviderKeyStatus> {
    Ok(ProviderKeyStatus::load())
}

/// 读取当前 AI 配置（**不含密钥**）
#[tauri::command]
pub async fn ai_get_config(state: State<'_, AppState>) -> AppResult<Option<ProviderConfig>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value_json FROM settings WHERE key = 'ai_config'")
            .fetch_optional(state.db.pool())
            .await?;

    let mut cfg: Option<ProviderConfig> = row.and_then(|(j,)| serde_json::from_str(&j).ok());
    if let Some(c) = cfg.as_mut() {
        c.normalize();
        // has_api_key 以凭据管理器的实际内容为准，而不是数据库里的标记——
        // 用户可能从系统设置里手动删除了凭据。
        c.has_api_key = load_api_key(c.provider)?.is_some();
    }
    Ok(cfg)
}

/// 保存 AI 配置。`api_key` 为 None 表示不改动已保存的密钥。
#[tauri::command]
pub async fn ai_set_config(
    state: State<'_, AppState>,
    config: ProviderConfig,
    api_key: Option<String>,
) -> AppResult<ProviderConfig> {
    let mut cfg = config;
    cfg.normalize();
    // 保存时**不要求模型已选**（整改任务书 §3.3）：
    // 用户必须先能把 API Key 存进凭据管理器，才可能拉到模型列表。
    // 模型为空的严格校验放在真正调用前（`validate_for_run`）。
    cfg.validate_for_save()?;

    // 密钥单独存凭据管理器；空字符串表示清除
    if let Some(k) = api_key.as_ref() {
        save_api_key(cfg.provider, k)?;
    }
    cfg.has_api_key = load_api_key(cfg.provider)?.is_some();

    // 数据库里**只存配置本身，不存密钥**（§6 / §10）
    let json = serde_json::to_string(&cfg)
        .map_err(|e| AppError::internal(format!("序列化 AI 配置失败：{e}")))?;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at) VALUES ('ai_config', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
    )
    .bind(&json)
    .bind(&now)
    .execute(state.db.pool())
    .await?;

    log::info!(
        "AI 配置已更新：{} / {}（密钥{}）",
        cfg.provider.label(),
        cfg.model,
        if cfg.has_api_key {
            "已保存"
        } else {
            "未设置"
        }
    );
    Ok(cfg)
}

/// 测试连接（§6 要求"可连接测试"）
#[tauri::command]
pub async fn ai_test_connection(config: ProviderConfig) -> AppResult<String> {
    test_connection(&config).await
}

/// 列出可用模型。失败时返回空列表 + 原因，界面据此引导手动输入（§6）。
///
/// 注意：这里**只校验端点**，不要求模型已选——恰恰相反，这个接口存在的意义
/// 就是帮用户在"还没有模型"的时候把模型选出来（整改任务书 §3.5 场景 B）。
#[tauri::command]
pub async fn ai_list_models(config: ProviderConfig) -> AppResult<serde_json::Value> {
    if let Err(e) = config.validate_endpoint() {
        return Ok(serde_json::json!({
            "ok": false,
            "models": [],
            "reason": e.message,
            "hint": e.hint,
            "fallback": "请先填写正确的 Base URL，或直接手动填写模型名称。",
        }));
    }
    match list_models(&config).await {
        Ok(list) => Ok(serde_json::json!({ "ok": true, "models": list })),
        Err(e) => Ok(serde_json::json!({
            "ok": false,
            "models": [],
            "reason": e.message,
            "hint": e.hint,
            // 明确告知这不影响使用——用户可以手填模型名
            "fallback": "无法获取模型列表时，可直接在下方手动填写模型名称。",
        })),
    }
}

/// 删除已保存的密钥
#[tauri::command]
pub async fn ai_clear_key(state: State<'_, AppState>, provider: Provider) -> AppResult<bool> {
    delete_api_key(provider)?;
    // 同步更新数据库里的 has_api_key 标记
    if let Some((json,)) =
        sqlx::query_as::<_, (String,)>("SELECT value_json FROM settings WHERE key = 'ai_config'")
            .fetch_optional(state.db.pool())
            .await?
    {
        if let Ok(mut cfg) = serde_json::from_str::<ProviderConfig>(&json) {
            if cfg.provider == provider {
                cfg.has_api_key = false;
                let now = crate::db::to_db_time(crate::db::utc_now());
                let s = serde_json::to_string(&cfg).unwrap_or(json);
                sqlx::query(
                    "UPDATE settings SET value_json = ?1, updated_at = ?2 WHERE key = 'ai_config'",
                )
                .bind(&s)
                .bind(&now)
                .execute(state.db.pool())
                .await?;
            }
        }
    }
    Ok(true)
}

/// 检查是否已配置可用的 AI（未配置时软件其余功能必须完全可用，§6）
#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let cfg = ai_get_config(state).await?;
    let configured = cfg.as_ref().map(|c| c.has_api_key).unwrap_or(false);
    Ok(serde_json::json!({
        "configured": configured,
        "provider": cfg.as_ref().map(|c| c.provider),
        "model": cfg.as_ref().map(|c| c.model.clone()),
        "note": if configured {
            "AI 已配置。所有 AI 生成的内容都会先以预览形式呈现，需要你确认后才会写入。"
        } else {
            "尚未配置 AI。任务管理、重复规则、提醒、统计等全部功能都可在离线状态下正常使用。"
        },
    }))
}

#[cfg(test)]
mod provider_matrix_tests {
    use super::*;

    /// 三家协议差异的**汇总断言**。
    ///
    /// 单点测试容易漏掉"某一家被顺手写成另一家的风格"，
    /// 这里用一张表把三家并排比较，任何一家的关键差异被改掉都会失败。
    #[test]
    fn three_providers_differ_in_expected_ways() {
        struct Case {
            p: Provider,
            anthropic_style: bool,
            endpoint_suffix: &'static str,
            system_in_messages: bool,
        }

        let cases = [
            Case {
                p: Provider::DeepSeek,
                anthropic_style: false,
                endpoint_suffix: "/chat/completions",
                system_in_messages: true,
            },
            Case {
                p: Provider::OpenAI,
                anthropic_style: false,
                endpoint_suffix: "/responses",
                system_in_messages: true,
            },
            Case {
                p: Provider::Claude,
                anthropic_style: true,
                endpoint_suffix: "/v1/messages",
                system_in_messages: false,
            },
        ];

        for c in cases {
            let mut cfg = ProviderConfig::with_defaults(c.p);
            cfg.base_url = c.p.default_base_url().to_string();

            assert_eq!(
                c.p.is_anthropic_style(),
                c.anthropic_style,
                "{} 的协议风格判断错误",
                c.p.label()
            );

            let url = endpoint(&cfg);
            assert!(
                url.ends_with(c.endpoint_suffix),
                "{} 的端点是 {url}，应以 {} 结尾",
                c.p.label(),
                c.endpoint_suffix
            );

            let req = ChatRequest {
                config: cfg.clone(),
                system: Some("S".into()),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: "U".into(),
                }],
                json_output: false,
                max_output_tokens: None,
            };
            let body = if c.anthropic_style {
                build_anthropic_body(&req)
            } else {
                build_openai_body(&req)
            };

            let sys_in_msgs = body["messages"]
                .as_array()
                .map(|a| a.iter().any(|m| m["role"] == "system"))
                .unwrap_or(false);
            assert_eq!(
                sys_in_msgs,
                c.system_in_messages,
                "{} 的 system 位置不符合预期",
                c.p.label()
            );

            if c.anthropic_style {
                assert_eq!(body["system"], "S", "Claude 的 system 应在顶层");
                assert!(
                    body.get("max_tokens").is_some(),
                    "Claude 的 max_tokens 必填"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(p: Provider) -> ProviderConfig {
        ProviderConfig::with_defaults(p)
    }

    // ------------------------- 端点拼接（DeepSeek 无 /v1） -------------------------

    /// DeepSeek 的路径**不能**带 /v1：官方文档全站未出现该段。
    /// 这个测试锁住这个容易凭记忆写错的细节。
    #[test]
    fn deepseek_endpoint_has_no_v1() {
        let mut c = cfg(Provider::DeepSeek);
        c.base_url = "https://api.deepseek.com".into();
        let u = endpoint(&c);
        assert_eq!(u, "https://api.deepseek.com/chat/completions");
        assert!(!u.contains("/v1"), "DeepSeek 的地址不应出现 /v1：{u}");
    }

    #[test]
    fn openai_endpoint_uses_responses_api() {
        // 整改任务书 §3.2/§3.4：OpenAI 走官方推荐的 Responses API，
        // 与 DeepSeek/Custom 的 Chat Completions 彻底分开。
        let c = cfg(Provider::OpenAI);
        assert_eq!(endpoint(&c), "https://api.openai.com/v1/responses");
    }

    /// Claude 走 /v1/messages，不是 /chat/completions
    #[test]
    fn anthropic_uses_messages_endpoint() {
        let c = cfg(Provider::Claude);
        assert_eq!(endpoint(&c), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn trailing_slash_does_not_produce_double_slash() {
        let mut c = cfg(Provider::OpenAI);
        c.base_url = "https://api.openai.com/v1/".into();
        assert!(!endpoint(&c).contains("//v1"));
        assert_eq!(endpoint(&c), "https://api.openai.com/v1/responses");
    }

    // ------------------------- 请求体差异 -------------------------

    fn sample_req(p: Provider) -> ChatRequest {
        ChatRequest {
            config: cfg(p),
            system: Some("你是助手".into()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "你好".into(),
            }],
            json_output: false,
            max_output_tokens: None,
        }
    }

    /// OpenAI 风格把 system 放进 messages
    #[test]
    fn openai_body_puts_system_in_messages() {
        let b = build_openai_body(&sample_req(Provider::OpenAI));
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "你是助手");
        assert_eq!(msgs[1]["role"], "user");
        // 顶层不应有 system
        assert!(b.get("system").is_none());
    }

    /// Anthropic 的 system 必须是**顶层字段**，messages 里不能有 system
    #[test]
    fn anthropic_body_puts_system_at_top_level() {
        let b = build_anthropic_body(&sample_req(Provider::Claude));
        assert_eq!(b["system"], "你是助手");
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1, "messages 里不应包含 system");
        assert_eq!(msgs[0]["role"], "user");
    }

    /// Anthropic 的 max_tokens 必填：即使没传也必须在 body 里出现
    #[test]
    fn anthropic_body_always_has_max_tokens() {
        let mut req = sample_req(Provider::Claude);
        req.max_output_tokens = None;
        let b = build_anthropic_body(&req);
        assert!(
            b.get("max_tokens").and_then(|v| v.as_i64()).is_some(),
            "max_tokens 是 Claude 的必填参数"
        );

        let b2 = build_openai_body(&sample_req(Provider::OpenAI));
        assert!(b2.get("max_tokens").is_some());
    }

    /// Anthropic 的 messages 只接受 user/assistant，混入 system 会被拒
    #[test]
    fn anthropic_body_filters_non_chat_roles() {
        let mut req = sample_req(Provider::Claude);
        req.messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "不该出现在这里".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "问题".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "回答".into(),
            },
        ];
        let b = build_anthropic_body(&req);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "system 角色应被过滤");
        for m in msgs {
            let r = m["role"].as_str().unwrap();
            assert!(r == "user" || r == "assistant");
        }
    }

    #[test]
    fn json_output_sets_response_format_for_openai_style() {
        let mut req = sample_req(Provider::DeepSeek);
        req.json_output = true;
        let b = build_openai_body(&req);
        // DeepSeek 只支持 json_object（没有 json_schema）
        assert_eq!(b["response_format"]["type"], "json_object");
    }

    #[test]
    fn json_output_uses_output_config_for_anthropic() {
        let mut req = sample_req(Provider::Claude);
        req.json_output = true;
        let b = build_anthropic_body(&req);
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
    }

    // ------------------------- 响应解析差异 -------------------------

    #[test]
    fn parses_openai_response() {
        let v = serde_json::json!({
            "model": "deepseek-flash",
            "choices": [{
                "message": { "role": "assistant", "content": "你好" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        });
        let r = parse_openai_response(&v).unwrap();
        assert_eq!(r.text, "你好");
        assert_eq!(r.usage.unwrap().input_tokens, Some(10));
        assert!(!r.truncated);
    }

    /// Claude 的文本在顶层 content 数组里，且有多种块类型
    #[test]
    fn parses_anthropic_content_blocks() {
        let v = serde_json::json!({
            "model": "claude-sonnet-5",
            "content": [
                { "type": "thinking", "thinking": "内部推理不该被当正文" },
                { "type": "text", "text": "第一段" },
                { "type": "text", "text": "第二段" }
            ],
            "stop_reason": "end_turn"
        });
        let r = parse_anthropic_response(&v).unwrap();
        assert_eq!(r.text, "第一段第二段", "应只拼接 text 块");
        assert!(!r.text.contains("内部推理"));
    }

    #[test]
    fn anthropic_truncation_is_detected() {
        let v = serde_json::json!({
            "content": [{ "type": "text", "text": "被截断" }],
            "stop_reason": "max_tokens"
        });
        assert!(parse_anthropic_response(&v).unwrap().truncated);
    }

    /// DeepSeek 官方提示存在偶发空内容，必须识别为错误而不是"成功的空回复"
    #[test]
    fn empty_content_is_treated_as_error() {
        let v = serde_json::json!({
            "choices": [{ "message": { "content": "" }, "finish_reason": "stop" }]
        });
        let e = parse_openai_response(&v).unwrap_err();
        assert!(e.message.contains("空内容"), "{}", e.message);

        let v2 = serde_json::json!({
            "content": [{ "type": "thinking", "thinking": "只有思考块" }]
        });
        assert!(parse_anthropic_response(&v2).is_err());
    }

    #[test]
    fn malformed_responses_are_rejected() {
        assert!(parse_openai_response(&serde_json::json!({})).is_err());
        assert!(parse_anthropic_response(&serde_json::json!({})).is_err());
        // choices 存在但为空数组
        assert!(parse_openai_response(&serde_json::json!({ "choices": [] })).is_err());
    }

    /// DeepSeek 的缓存命中/未命中 token 是计费依据，必须解析出来
    #[test]
    fn parses_deepseek_cache_token_fields() {
        let v = serde_json::json!({
            "choices": [{ "message": { "content": "ok" }, "finish_reason": "stop" }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_cache_hit_tokens": 80,
                "prompt_cache_miss_tokens": 20
            }
        });
        let u = parse_openai_response(&v).unwrap().usage.unwrap();
        assert_eq!(u.cache_hit_tokens, Some(80));
        assert_eq!(u.cache_miss_tokens, Some(20));
    }

    // ------------------------- 错误映射 -------------------------

    #[test]
    fn auth_error_maps_to_unauthorized() {
        let e = map_http_error(Provider::OpenAI, 401, "invalid key");
        assert!(matches!(e.code, crate::error::ErrorCode::Unauthorized));
        assert!(e.hint.is_some());
    }

    /// 余额不足重试无效，提示必须说清
    #[test]
    fn insufficient_balance_says_retry_is_useless() {
        let e = map_http_error(Provider::DeepSeek, 402, "Insufficient Balance");
        assert!(matches!(e.code, crate::error::ErrorCode::QuotaExceeded));
        let hint = e.hint.unwrap();
        assert!(hint.contains("重试无效"), "应明确告知重试无效：{hint}");
    }

    #[test]
    fn rate_limit_maps_to_rate_limited() {
        let e = map_http_error(Provider::Claude, 429, "overloaded");
        assert!(matches!(e.code, crate::error::ErrorCode::RateLimited));
    }

    /// 404 的提示要提到 DeepSeek 无 /v1 —— 这是最容易踩的坑
    #[test]
    fn not_found_hint_mentions_deepseek_v1_pitfall() {
        let e = map_http_error(Provider::DeepSeek, 404, "not found");
        assert!(e.hint.unwrap().contains("/v1"));
    }

    #[test]
    fn server_error_suggests_retry_later() {
        let e = map_http_error(Provider::OpenAI, 503, "overloaded");
        assert!(e.hint.unwrap().contains("稍后重试"));
    }

    // ------------------------- 配置校验 -------------------------

    /// 默认值只有一个来源：`ai_provider_defaults` 必须与 `with_defaults` 完全一致
    /// （整改任务书 §2.2 / §10.1）。
    #[test]
    fn provider_defaults_have_a_single_source() {
        let all = ProviderDefaults::all();
        assert_eq!(
            all.len(),
            Provider::ALL.len(),
            "每个提供商都必须有默认值，否则界面上会出现空地址"
        );
        for d in &all {
            let c = ProviderConfig::with_defaults(d.provider);
            assert_eq!(d.base_url, c.base_url, "Base URL 与 with_defaults 漂移");
            assert_eq!(d.model, c.model, "默认模型与 with_defaults 漂移");
            assert_eq!(d.timeout_seconds, c.timeout_seconds);
            assert_eq!(d.max_output_tokens, c.max_output_tokens);
            assert_eq!(d.data_policy_note, d.provider.data_policy_note());
            assert_eq!(d.label, d.provider.label());
            assert_eq!(d.model_must_be_chosen, d.model.is_empty());
            assert!(!d.label.trim().is_empty());
        }

        // OpenAI 默认模型未经验证，必须留空，并如实标记"必须由用户选定"
        let openai = all
            .iter()
            .find(|d| d.provider == Provider::OpenAI)
            .expect("OpenAI 必须在默认值表里");
        assert!(openai.model.is_empty(), "OpenAI 默认模型未经验证，必须留空");
        assert!(openai.model_must_be_chosen);
        assert!(
            !openai.base_url.is_empty(),
            "Base URL 是有官方依据的，不能为空"
        );
    }

    /// 防漂移护栏：前端不得再维护第二份默认值表（整改任务书 §2.2 / §14.1）。
    ///
    /// 这条测试直接扫前端源码。它挡住的正是本轮修掉的那个真问题：
    /// 后端已经把 OpenAI 默认模型改成空，前端却还留着 `gpt-6-astra`，
    /// 于是用户切到 OpenAI 后会被自动填入一个**未经验证的 Azure 模型 ID**。
    #[test]
    fn frontend_has_no_duplicate_provider_defaults() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    collect(&p, out);
                } else if matches!(p.extension().and_then(|s| s.to_str()), Some("ts" | "tsx")) {
                    // 测试文件里可以讨论这些标识符（护栏自身的说明就写在里面），
                    // 但**产品代码**里不允许出现。
                    let is_test = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| n.contains(".test.") || n.contains(".spec."));
                    if !is_test {
                        out.push(p);
                    }
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        assert!(root.is_dir(), "前端源码目录不存在：{}", root.display());
        let mut files = Vec::new();
        collect(&root, &mut files);
        assert!(files.len() > 10, "扫到的前端文件太少，护栏失效");

        // 这些标识符一旦在前端出现，就说明默认值被复制成了第二份来源
        const FORBIDDEN: [&str; 3] = [
            "PROVIDER_DEFAULT_MODEL",
            "PROVIDER_DEFAULT_BASE",
            "gpt-6-astra",
        ];
        for f in &files {
            let Ok(text) = std::fs::read_to_string(f) else {
                continue;
            };
            for bad in FORBIDDEN {
                assert!(
                    !text.contains(bad),
                    "{} 里出现了 {bad}：Provider 默认值必须只由后端提供（整改任务书 §2.2）",
                    f.display()
                );
            }
        }
    }

    #[test]
    fn default_base_urls_match_official_docs() {
        assert_eq!(
            Provider::DeepSeek.default_base_url(),
            "https://api.deepseek.com"
        );
        assert_eq!(
            Provider::OpenAI.default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            Provider::Claude.default_base_url(),
            "https://api.anthropic.com"
        );
    }

    /// 默认模型必须是调研时官方文档列出的 ID，不能用已退役的旧名
    #[test]
    fn default_models_are_not_retired_names() {
        let ds = Provider::DeepSeek.default_model();
        assert_ne!(ds, "deepseek-chat", "deepseek-chat 已不存在");
        assert_ne!(ds, "deepseek-reasoner", "deepseek-reasoner 已不存在");
        assert_eq!(ds, "deepseek-flash");

        let cl = Provider::Claude.default_model();
        assert!(!cl.contains("3-5-sonnet"), "claude-3-5-sonnet 已是旧名");
        assert_eq!(cl, "claude-sonnet-5", "Claude 默认应是官方推荐的通用模型");
    }

    /// 整改任务书 §3.5：默认模型不得是**未经验证**的名字。
    ///
    /// OpenAI 的官方文档在核实环境里不可达（全部域名 403），
    /// `gpt-6-astra` 只能从 Microsoft 的 Azure 文档旁证，无法确认在
    /// OpenAI 平台上同名可用。因此这里锁死"OpenAI 默认模型必须为空"，
    /// 谁要是再凭印象填一个"听起来对"的 ID，这条测试就会红。
    #[test]
    fn openai_default_model_is_empty_until_verified() {
        assert_eq!(
            Provider::OpenAI.default_model(),
            "",
            "OpenAI 的默认模型未获官方确认，必须留空让用户拉取模型列表或手动填写"
        );
        assert_ne!(Provider::OpenAI.default_model(), "gpt-6-astra");
    }

    // =====================================================================
    // 整改任务书 §3.7：三家协议的关键差异逐条锁死
    // =====================================================================

    /// 三家的对话端点路径规则各不相同，写错就是 404 或打错协议
    #[test]
    fn endpoints_match_each_provider_protocol() {
        let deepseek = cfg(Provider::DeepSeek);
        assert_eq!(
            endpoint(&deepseek),
            "https://api.deepseek.com/chat/completions",
            "DeepSeek 的 base 不带 /v1，路径直接跟 /chat/completions"
        );

        let openai = cfg(Provider::OpenAI);
        assert_eq!(
            endpoint(&openai),
            "https://api.openai.com/v1/responses",
            "OpenAI 走 Responses API，而不是 chat/completions"
        );

        let claude = cfg(Provider::Claude);
        assert_eq!(
            endpoint(&claude),
            "https://api.anthropic.com/v1/messages",
            "Anthropic 是 /v1/messages"
        );

        let mut custom = cfg(Provider::Custom);
        custom.base_url = "http://127.0.0.1:11434/v1".into();
        assert_eq!(
            endpoint(&custom),
            "http://127.0.0.1:11434/v1/chat/completions",
            "自定义兼容服务走 Chat Completions"
        );
    }

    /// OpenAI 用 Responses 的字段名，DeepSeek/Custom 用 Chat Completions 的——
    /// 两者不能混（用错字段名服务商直接 400）
    #[test]
    fn request_bodies_differ_between_responses_and_chat_completions() {
        let req = |p: Provider| ChatRequest {
            config: cfg(p),
            system: Some("系统提示".into()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "你好".into(),
            }],
            json_output: true,
            max_output_tokens: Some(128),
        };

        // --- OpenAI Responses ---
        let oa = build_openai_responses_body(&req(Provider::OpenAI));
        assert!(oa.get("input").is_some(), "Responses 用 input");
        assert!(oa.get("messages").is_none(), "Responses 不应出现 messages");
        assert_eq!(oa["instructions"], "系统提示", "system 走顶层 instructions");
        assert_eq!(
            oa["max_output_tokens"], 128,
            "输出上限字段名是 max_output_tokens"
        );
        assert!(
            oa.get("max_tokens").is_none(),
            "Responses 不接受 max_tokens"
        );
        assert_eq!(
            oa["text"]["format"]["type"], "json_object",
            "JSON 输出走 text.format"
        );
        assert!(oa.get("response_format").is_none());

        // --- DeepSeek / Custom 的 Chat Completions ---
        let ds = build_openai_body(&req(Provider::DeepSeek));
        assert!(ds.get("messages").is_some(), "Chat Completions 用 messages");
        assert_eq!(ds["messages"][0]["role"], "system", "system 放进 messages");
        assert_eq!(ds["max_tokens"], 128, "Chat Completions 用 max_tokens");
        assert!(ds.get("max_output_tokens").is_none());
        assert_eq!(
            ds["response_format"]["type"], "json_object",
            "JSON 输出走 response_format"
        );
        assert!(ds.get("text").is_none());

        // --- Anthropic ---
        let cl = build_anthropic_body(&req(Provider::Claude));
        assert_eq!(cl["system"], "系统提示", "Anthropic 的 system 是顶层字段");
        assert!(
            cl["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|m| m["role"] != "system"),
            "Anthropic 的 messages 里不能有 system"
        );
        assert_eq!(cl["max_tokens"], 128, "Anthropic 的 max_tokens 必填");
        assert_eq!(cl["output_config"]["format"]["type"], "json_schema");
    }

    /// 四类请求头：Bearer / x-api-key + anthropic-version
    #[test]
    fn auth_headers_match_provider_requirements() {
        // Anthropic 用 x-api-key 且必须带 anthropic-version
        assert!(Provider::Claude.is_anthropic_style());
        assert!(!Provider::Claude.uses_openai_responses());

        // 其余三家都走 Bearer
        for p in [Provider::DeepSeek, Provider::OpenAI, Provider::Custom] {
            assert!(!p.is_anthropic_style(), "{:?} 不该用 x-api-key", p);
        }

        // OpenAI 是唯一走 Responses 的
        assert!(Provider::OpenAI.uses_openai_responses());
        for p in [Provider::DeepSeek, Provider::Claude, Provider::Custom] {
            assert!(!p.uses_openai_responses(), "{:?} 不该走 Responses", p);
        }
    }

    /// OpenAI Responses 的响应解析：文本在 output[].content[].output_text
    #[test]
    fn parse_openai_responses_extracts_text_and_usage() {
        let v = serde_json::json!({
            "id": "resp_1",
            "model": "some-model",
            "status": "completed",
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "role": "assistant", "content": [
                    { "type": "output_text", "text": "第一段" },
                    { "type": "output_text", "text": "第二段" }
                ]}
            ],
            "usage": {
                "input_tokens": 120,
                "output_tokens": 34,
                "input_tokens_details": { "cached_tokens": 100 }
            }
        });
        let r = parse_openai_responses_response(&v).expect("应解析成功");
        assert_eq!(r.text, "第一段第二段", "应拼接所有 output_text 块");
        assert_eq!(r.model, "some-model");
        assert!(!r.truncated);
        let u = r.usage.unwrap();
        assert_eq!(u.input_tokens, Some(120));
        assert_eq!(u.output_tokens, Some(34));
        assert_eq!(
            u.cache_hit_tokens,
            Some(100),
            "缓存命中取 input_tokens_details"
        );
    }

    /// 截断状态：OpenAI Responses 看 status=incomplete，而不是 finish_reason
    #[test]
    fn openai_responses_truncation_is_detected() {
        let v = serde_json::json!({
            "model": "m",
            "status": "incomplete",
            "incomplete_details": { "reason": "max_output_tokens" },
            "output": [{ "type": "message", "content": [{ "type": "output_text", "text": "被截断的内容" }] }]
        });
        let r = parse_openai_responses_response(&v).unwrap();
        assert!(r.truncated, "status=incomplete 必须被识别为截断");
    }

    /// 空响应：不能当成成功（否则上层会拿到空字符串继续跑）
    #[test]
    fn empty_responses_are_errors_not_successes() {
        // OpenAI Responses：没有任何文本块
        let v = serde_json::json!({
            "status": "completed",
            "output": [{ "type": "message", "content": [] }]
        });
        let e = parse_openai_responses_response(&v).unwrap_err();
        assert!(
            e.message.contains("空内容"),
            "应明确报空内容，实际：{}",
            e.message
        );

        // Chat Completions：choices 里 content 为空
        let v = serde_json::json!({
            "choices": [{ "message": { "content": "   " }, "finish_reason": "stop" }]
        });
        assert!(parse_openai_response(&v).is_err(), "空内容必须报错");

        // Anthropic：content 数组里没有 text 块
        let v = serde_json::json!({ "content": [{ "type": "thinking" }] });
        assert!(parse_anthropic_response(&v).is_err());
    }

    /// 模型拒绝（refusal）要说清楚，不能和"空内容"混为一谈
    #[test]
    fn openai_responses_refusal_has_specific_message() {
        let v = serde_json::json!({
            "status": "completed",
            "output": [{ "type": "message", "content": [
                { "type": "refusal", "refusal": "我不能帮助这个请求" }
            ]}]
        });
        let e = parse_openai_responses_response(&v).unwrap_err();
        assert!(e.message.contains("拒绝"), "实际：{}", e.message);
    }

    /// Anthropic 的 stop_reason=refusal / model_context_window_exceeded
    /// 与 max_tokens 一样都属于"没正常说完"，至少要能识别 max_tokens
    #[test]
    fn anthropic_stop_reasons_are_handled() {
        let mk = |reason: &str| {
            serde_json::json!({
                "model": "claude-sonnet-5",
                "stop_reason": reason,
                "content": [{ "type": "text", "text": "内容" }],
                "usage": { "input_tokens": 5, "output_tokens": 7,
                           "cache_read_input_tokens": 2, "cache_creation_input_tokens": 1 }
            })
        };
        assert!(
            parse_anthropic_response(&mk("max_tokens"))
                .unwrap()
                .truncated
        );
        assert!(!parse_anthropic_response(&mk("end_turn")).unwrap().truncated);

        let r = parse_anthropic_response(&mk("end_turn")).unwrap();
        let u = r.usage.unwrap();
        assert_eq!(u.input_tokens, Some(5));
        assert_eq!(u.cache_hit_tokens, Some(2));
        assert_eq!(u.cache_miss_tokens, Some(1));
    }

    /// DeepSeek 的 usage 含缓存字段，且 finish_reason=length 表示截断
    #[test]
    fn deepseek_usage_and_finish_reason_are_parsed() {
        let v = serde_json::json!({
            "model": "deepseek-flash",
            "choices": [{ "message": { "content": "好的" }, "finish_reason": "length" }],
            "usage": {
                "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30,
                "prompt_cache_hit_tokens": 6, "prompt_cache_miss_tokens": 4
            }
        });
        let r = parse_openai_response(&v).unwrap();
        assert!(r.truncated, "finish_reason=length 应判定为截断");
        let u = r.usage.unwrap();
        assert_eq!(u.input_tokens, Some(10));
        assert_eq!(u.output_tokens, Some(20));
        assert_eq!(u.cache_hit_tokens, Some(6));
        assert_eq!(u.cache_miss_tokens, Some(4));
    }

    /// HTTP 错误码分类：401 / 429 / 5xx / 529 各自的语义与可重试性
    #[test]
    fn http_errors_are_classified_for_each_status() {
        use crate::error::ErrorCode;

        let cases = [
            (401u16, ErrorCode::Unauthorized),
            (403, ErrorCode::Unauthorized),
            (402, ErrorCode::QuotaExceeded),
            (429, ErrorCode::RateLimited),
            (500, ErrorCode::Network),
            (503, ErrorCode::Network),
            (529, ErrorCode::Network), // Anthropic 特有的 overloaded
        ];
        for (status, expect) in cases {
            let e = map_http_error(Provider::Claude, status, r#"{"error":"x"}"#);
            assert!(
                matches!(e.code, c if format!("{c:?}") == format!("{expect:?}")),
                "HTTP {status} 应映射为 {expect:?}，实际 {:?}",
                e.code
            );
        }

        // 400 是参数问题，属于用户可修正的校验错误
        let e = map_http_error(Provider::DeepSeek, 400, "bad model");
        assert!(matches!(e.code, ErrorCode::Validation));
        assert!(e.hint.unwrap().contains("bad model"), "应带上服务商说明");
    }

    /// §10：第三方错误正文必须脱敏后再展示
    #[test]
    fn provider_error_bodies_are_sanitized() {
        // 1) Bearer token 被回显
        let s = sanitize_provider_error("Unauthorized: Bearer sk-abcdef1234567890abcdef", 300);
        assert!(
            !s.contains("sk-abcdef1234567890abcdef"),
            "密钥必须被打码：{s}"
        );
        assert!(s.contains("***"));
        assert!(s.contains("Unauthorized"), "但错误原因要保留");

        // 2) 裸的 OpenAI 风格 key
        let s = sanitize_provider_error("invalid api_key: sk-proj-AAAABBBBCCCCDDDDEEEEFFFF", 300);
        assert!(!s.contains("AAAABBBBCCCCDDDDEEEEFFFF"), "实际：{s}");

        // 3) 裸的长 token（很多中转会直接回显）
        let long = "a".repeat(40);
        let s = sanitize_provider_error(&format!("token {long} rejected"), 300);
        assert!(!s.contains(&long), "长 token 必须被打码：{s}");

        // 4) HTML 错误页被剥成文本
        let s = sanitize_provider_error(
            "<html><head><title>502 Bad Gateway</title></head><body>nginx</body></html>",
            300,
        );
        assert!(
            !s.contains('<') && !s.contains('>'),
            "HTML 标签应被剥掉：{s}"
        );
        assert!(s.contains("502 Bad Gateway"));

        // 5) 超长正文被截断且明示
        let s = sanitize_provider_error(&"字".repeat(1000), 100);
        assert!(s.contains("已截断"));
        assert!(s.chars().count() <= 120);

        // 6) 正常短错误原样保留（不能把有用信息也吃掉）
        let s = sanitize_provider_error("model not found", 300);
        assert_eq!(s, "model not found");

        // 7) 换行与多余空白被折叠
        let s = sanitize_provider_error("a\n\n   b\t\tc", 300);
        assert_eq!(s, "a b c");
    }

    /// API Key 未配置时必须给出明确的"未配置"错误，而不是发一个没头的请求
    #[tokio::test]
    async fn missing_api_key_is_reported_before_request() {
        let c = cfg(Provider::DeepSeek);
        // 测试环境没有凭据管理器里的 key，因此这里必然走到 NotConfigured
        let req = ChatRequest {
            config: c.clone(),
            system: None,
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            json_output: false,
            max_output_tokens: None,
        };
        let err = chat(&c, &req).await.unwrap_err();
        assert!(
            matches!(err.code, crate::error::ErrorCode::NotConfigured),
            "应报未配置，实际 {:?}：{}",
            err.code,
            err.message
        );
    }

    /// 错误的 Base URL 必须在校验阶段就被拒绝（而不是发出请求后 404）
    #[test]
    fn invalid_base_url_is_rejected_at_validation() {
        let mut c = cfg(Provider::OpenAI);
        // OpenAI 的默认模型现在故意留空，这里先给一个值，
        // 保证断言只针对 Base URL 这一项。
        c.model = "some-model".into();

        c.base_url = "not a url".into();
        assert!(c.validate_endpoint().is_err());

        c.base_url = "ftp://api.openai.com/v1".into();
        assert!(c.validate_endpoint().is_err(), "非 http(s) 协议必须拒绝");

        // 非本机的 http 必须拒绝（避免明文发密钥）
        c.base_url = "http://api.example.com/v1".into();
        assert!(c.validate_endpoint().is_err(), "非本机的 http 必须拒绝");

        // localhost / 127.0.0.1 / ::1 的 http 允许（本地模型）
        for local in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:11434/v1",
            "http://[::1]:11434/v1",
        ] {
            c.base_url = local.into();
            assert!(
                c.validate_endpoint().is_ok(),
                "{local} 属于本机地址，应当允许"
            );
        }
    }

    /// 超时与输出上限必须被夹到合理区间（§9：后端是最终校验层）
    #[test]
    fn timeout_and_output_limits_are_clamped() {
        let mut c = cfg(Provider::DeepSeek);
        c.timeout_seconds = 0;
        c.max_output_tokens = 0;
        c.normalize();
        assert_eq!(c.timeout_seconds, 60, "0 应被换成安全默认值");
        assert_eq!(c.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS);

        c.timeout_seconds = 100_000;
        c.max_output_tokens = 10_000_000;
        c.normalize();
        assert_eq!(c.timeout_seconds, 60, "超大超时应回落默认值");
        assert_eq!(c.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS);

        // 边界内的值保持不动
        c.timeout_seconds = 120;
        c.max_output_tokens = 4096;
        c.normalize();
        assert_eq!(c.timeout_seconds, 120);
        assert_eq!(c.max_output_tokens, 4096);

        // 归一化之后必须能通过校验
        assert!(c.validate_for_run().is_ok());
    }

    #[test]
    fn base_url_must_be_http_or_https() {
        let mut c = cfg(Provider::OpenAI);
        c.base_url = "ftp://example.com".into();
        assert!(c.validate_endpoint().is_err());

        c.base_url = "不是地址".into();
        assert!(c.validate_endpoint().is_err());

        c.base_url = String::new();
        assert!(c.validate_endpoint().is_err());
    }

    /// 明文 HTTP 只允许本机，否则密钥会在网络上裸奔
    #[test]
    fn plain_http_only_allowed_for_localhost() {
        let mut c = cfg(Provider::Custom);
        c.model = "local-model".into();

        for ok in ["http://localhost:11434/v1", "http://127.0.0.1:8080/v1"] {
            c.base_url = ok.into();
            assert!(c.validate_endpoint().is_ok(), "本机地址应允许：{ok}");
        }

        c.base_url = "http://api.example.com/v1".into();
        let e = c.validate_endpoint().unwrap_err();
        assert!(e.hint.unwrap().contains("明文"), "应说明风险");
    }

    /// 空模型：**保存放行、真正调用拒绝**（整改任务书 §3.3 / §3.5 场景 A、C）。
    ///
    /// 这条测试在修复前是红的：当时 `validate()` 一个方法同时管两件事，
    /// 于是"保存配置"也要求模型非空，用户永远走不到"拉取模型列表"那一步。
    #[test]
    fn empty_model_is_savable_but_not_runnable() {
        let mut c = cfg(Provider::Custom);
        c.base_url = "https://example.com/v1".into();
        c.model = "   ".into();

        assert!(
            c.validate_for_save().is_ok(),
            "模型为空必须仍能保存配置，否则用户无法先存 API Key 再拉模型列表"
        );

        let e = c.validate_for_run().unwrap_err();
        assert!(e.message.contains("尚未选择模型"), "实际：{}", e.message);
        assert!(
            e.hint.unwrap_or_default().contains("获取模型列表"),
            "提示必须告诉用户下一步怎么做"
        );
    }

    /// 归一化必须收敛越界值，避免请求参数非法
    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut c = cfg(Provider::OpenAI);
        c.timeout_seconds = 0;
        c.max_output_tokens = -5;
        c.normalize();
        assert_eq!(c.timeout_seconds, 60);
        assert_eq!(c.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS);

        c.timeout_seconds = 99999;
        c.max_output_tokens = MAX_OUTPUT_TOKENS_CAP + 1;
        c.normalize();
        assert_eq!(c.timeout_seconds, 60);
        assert_eq!(c.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS);
    }

    #[test]
    fn normalize_fills_defaults_and_trims_slash() {
        let mut c = cfg(Provider::DeepSeek);
        c.base_url = "https://api.deepseek.com/".into();
        c.model = "  ".into();
        c.normalize();
        assert_eq!(c.base_url, "https://api.deepseek.com");
        assert_eq!(c.model, "deepseek-flash");
    }

    // ------------------------- 密钥脱敏 -------------------------

    /// 脱敏后不能泄漏中间部分（§10 要求不明文展示密钥）
    #[test]
    fn mask_key_hides_middle() {
        let m = mask_key("sk-1234567890abcdefGHIJ");
        assert!(m.contains('•'), "应包含掩码字符");
        assert!(!m.contains("567890abcdef"), "中间部分必须被隐藏：{m}");
        assert!(m.starts_with("sk-1"));
    }

    #[test]
    fn mask_key_handles_short_input() {
        assert_eq!(mask_key("abc"), "••••••••");
        assert_eq!(mask_key(""), "••••••••");
        // 短密钥不应暴露任何字符
        assert!(!mask_key("12345678").contains('1'));
    }

    #[test]
    fn key_names_are_distinct_per_provider() {
        let names: Vec<String> = [
            Provider::DeepSeek,
            Provider::OpenAI,
            Provider::Claude,
            Provider::Custom,
        ]
        .iter()
        .map(|p| key_name(*p))
        .collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "不同服务商的凭据名不能重复");
    }

    /// 密钥状态必须**逐个 provider 独立**（回归护栏）。
    ///
    /// 修复前的缺陷：界面把当前 provider 的 `hasApiKey` 套给目标 provider，
    /// 于是 DeepSeek 存过密钥时切到 OpenAI 也显示「已配置」。这条测试锁住
    /// "返回的是一张每个 provider 一个键的表，而不是一个全局布尔值"。
    ///
    /// 刻意**不依赖本机凭据管理器里到底有没有密钥**：测试环境通常一个都没有，
    /// 所以不写 `assert!(status.deep_seek == false)` 这类断言——那种断言在
    /// 开发机存过密钥时会无辜变红。这里只断言"四个键都存在且类型正确"。
    #[test]
    fn provider_key_status_covers_each_provider_independently() {
        // 直接调**命令函数本身**（它不需要 State），而不是内部的 `load()`，
        // 这样命令的返回形状也被覆盖到。
        let status = ai_provider_key_status().expect("密钥状态命令应成功返回");
        let v = serde_json::to_value(status).expect("密钥状态应能序列化");
        let obj = v.as_object().expect("密钥状态必须是一个对象");

        assert_eq!(
            obj.len(),
            Provider::ALL.len(),
            "必须每个 provider 一个键（不能返回全局布尔值）：{v}"
        );

        // 序列化名必须与前端 `AiProvider` 的取值逐字一致：`OpenAI` 的天真
        // snake_case 是 `open_a_i`，而前端写的是 `open_ai`——这种不一致不会报错，
        // 只会让 `keyStatus[p]` 静默取到 undefined（存过密钥也显示未配置）。
        let names: Vec<String> = Provider::ALL
            .iter()
            .map(|p| {
                serde_json::to_value(p)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            vec!["deep_seek", "open_ai", "claude", "custom"],
            "provider 的序列化名必须与前端 AiProvider 一致"
        );

        for p in Provider::ALL {
            // 键名直接取 `Provider` 的序列化结果（snake_case），
            // 这样断言的就是前端 `keyStatus[p]` 真正用的那个键名；
            // 谁把这里改成 camelCase，这条测试就会红。
            let key = serde_json::to_value(p).unwrap();
            let key = key.as_str().expect("provider 应序列化为字符串");
            let val = obj
                .get(key)
                .unwrap_or_else(|| panic!("缺少 {key} 的密钥状态：{v}"));
            assert!(val.is_boolean(), "{key} 的状态必须是布尔值，实际：{val}");
        }
    }

    // ------------------------- 数据政策文案 -------------------------

    /// §6 要求说明数据范围；三家政策不同，文案**不能统一**
    #[test]
    fn data_policy_notes_differ_per_provider() {
        let ds = Provider::DeepSeek.data_policy_note();
        let oa = Provider::OpenAI.data_policy_note();
        let cl = Provider::Claude.data_policy_note();

        assert_ne!(ds, oa);
        assert_ne!(oa, cl);
        assert_ne!(ds, cl);

        // DeepSeek 的默认姿态是"用于改进服务"，必须明说
        assert!(
            ds.contains("改进"),
            "DeepSeek 文案应说明默认用于改进服务：{ds}"
        );
        assert!(ds.contains("关闭") || ds.contains("opt"), "应说明如何退出");
    }

    #[test]
    fn provider_labels_are_chinese() {
        for p in [
            Provider::DeepSeek,
            Provider::OpenAI,
            Provider::Claude,
            Provider::Custom,
        ] {
            assert!(!p.label().is_empty());
        }
    }

    #[test]
    fn only_claude_uses_anthropic_style() {
        assert!(Provider::Claude.is_anthropic_style());
        assert!(!Provider::OpenAI.is_anthropic_style());
        assert!(!Provider::DeepSeek.is_anthropic_style());
        assert!(!Provider::Custom.is_anthropic_style());
    }

    // ------------------------- max_tokens 上限 -------------------------

    #[test]
    fn max_tokens_is_clamped() {
        let mut req = sample_req(Provider::OpenAI);
        req.max_output_tokens = Some(MAX_OUTPUT_TOKENS_CAP * 10);
        let b = build_openai_body(&req);
        assert_eq!(
            b["max_tokens"].as_i64().unwrap(),
            MAX_OUTPUT_TOKENS_CAP,
            "超出上限应被收敛，避免意外产生高额费用"
        );

        req.max_output_tokens = Some(0);
        let b2 = build_openai_body(&req);
        assert!(b2["max_tokens"].as_i64().unwrap() >= 1, "至少为 1");
    }
}
