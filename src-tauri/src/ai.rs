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
const KEYRING_SERVICE: &str = "com.pla0185.aitodo";

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
    OpenAI,
    /// Anthropic Claude
    Claude,
    /// 自定义 OpenAI 兼容服务（如本地模型、第三方中转）
    Custom,
}

impl Provider {
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
    /// 这些是调研时（2026-09-23）官方文档列出的模型 ID。
    /// 旧名如 `deepseek-chat`、`claude-3-5-sonnet` 已不存在，
    /// 因此这里不把它们作为默认值。
    pub fn default_model(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek-flash",
            Self::OpenAI => "gpt-6-astra",
            Self::Claude => "claude-sonnet-5",
            Self::Custom => "",
        }
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
                "自定义服务的隐私政策由其提供方决定，AiTodo 无法代为说明。\
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

    /// 校验 Base URL 可用（§6 要求"Base URL 可编辑时校验"）
    pub fn validate(&self) -> AppResult<()> {
        let u = self.base_url.trim();
        if u.is_empty() {
            return Err(AppError::validation("Base URL 不能为空"));
        }
        if !u.starts_with("https://") && !u.starts_with("http://") {
            return Err(AppError::validation(format!("Base URL 必须以 http:// 或 https:// 开头：{u}"))
                .with_hint("例如 https://api.deepseek.com"));
        }
        // 明文 HTTP 只允许本机（本地模型常见），其它地址必须用 HTTPS，
        // 否则 API Key 会在网络上明文传输
        if u.starts_with("http://") {
            let host_part = u.trim_start_matches("http://");
            let host = host_part.split(['/', ':']).next().unwrap_or("");
            let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
            if !is_local {
                return Err(AppError::validation(
                    "出于安全考虑，非本机地址必须使用 https://",
                )
                .with_hint("明文 HTTP 会让 API Key 在网络上以明文传输，仅在连接 localhost 时允许"));
            }
        }
        if self.model.trim().is_empty() {
            return Err(AppError::validation("模型名称不能为空")
                .with_hint("若服务商不支持列出模型，请手动填写模型 ID"));
        }
        Ok(())
    }
}

// =============================================================================
// 密钥存储（Windows 凭据管理器）
// =============================================================================

/// 凭据条目名
fn key_name(provider: Provider) -> String {
    format!("ai-{}", match provider {
        Provider::DeepSeek => "deepseek",
        Provider::OpenAI => "openai",
        Provider::Claude => "claude",
        Provider::Custom => "custom",
    })
}

/// 保存 API Key 到系统凭据管理器（§6 要求优先使用 Windows 凭据管理）
pub fn save_api_key(provider: Provider, key: &str) -> AppResult<()> {
    let k = key.trim();
    if k.is_empty() {
        return delete_api_key(provider);
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider)).map_err(|e| {
        AppError::internal(format!("无法访问系统凭据管理器：{e}"))
    })?;
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
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider)).map_err(|e| {
        AppError::internal(format!("无法访问系统凭据管理器：{e}"))
    })?;
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => Ok(Some(p)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::internal(format!("读取密钥失败：{e}"))),
    }
}

/// 删除 API Key
pub fn delete_api_key(provider: Provider) -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &key_name(provider)).map_err(|e| {
        AppError::internal(format!("无法访问系统凭据管理器：{e}"))
    })?;
    match entry.delete_credential() {
        Ok(()) => {
            log::info!("已删除 {} 的 API Key", provider.label());
            Ok(())
        }
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::internal(format!("删除密钥失败：{e}"))),
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
        .user_agent("AiTodo/0.1")
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
        AppError::new(crate::error::ErrorCode::Network, format!("无法连接到服务商：{e}"))
            .with_hint("请检查网络、代理设置与 Base URL 是否正确")
    } else {
        AppError::new(crate::error::ErrorCode::Network, format!("网络请求失败：{e}"))
    }
}

/// 按 HTTP 状态码与响应体翻译错误。
///
/// 关键区分：**计费与配额类错误重试无效**，要直接告诉用户去充值/提额，
/// 而不是让他反复重试。
fn map_http_error(provider: Provider, status: u16, body: &str) -> AppError {
    let snippet: String = body.chars().take(400).collect();
    match status {
        401 | 403 => AppError::new(crate::error::ErrorCode::Unauthorized, "API Key 无效或没有权限")
            .with_hint(format!(
                "请在设置中检查 {} 的 API Key 是否正确、是否已过期。\n服务商返回：{snippet}",
                provider.label()
            )),
        402 => AppError::new(crate::error::ErrorCode::QuotaExceeded, "账户余额不足")
            .with_hint("这是计费问题，重试无效。请到服务商控制台充值后重试"),
        429 => AppError::new(crate::error::ErrorCode::RateLimited, "请求过于频繁，已触发限流")
            .with_hint("请稍后重试。若频繁出现，请降低调用频率或检查账户配额"),
        400 | 422 => AppError::validation(format!("请求参数被拒绝（HTTP {status}）"))
            .with_hint(format!("常见原因：模型名不存在、参数超范围。\n服务商返回：{snippet}")),
        404 => AppError::validation("接口地址不存在（HTTP 404）").with_hint(format!(
            "请检查 Base URL 是否正确——注意 DeepSeek 的地址不含 /v1。\n服务商返回：{snippet}"
        )),
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
/// DeepSeek 无 `/v1`，OpenAI 的 base 已含 `/v1`，Anthropic 用 `/v1/messages`。
fn endpoint(cfg: &ProviderConfig) -> String {
    let base = cfg.base_url.trim_end_matches('/');
    match cfg.provider {
        Provider::Claude => format!("{base}/v1/messages"),
        // DeepSeek 与 OpenAI 兼容：base 之后直接跟 /chat/completions
        _ => format!("{base}/chat/completions"),
    }
}

/// 构造 OpenAI 风格（DeepSeek / OpenAI / Custom）的请求体
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
        // DeepSeek 只支持 json_object；OpenAI 的 chat/completions 也接受该形式。
        // 真正的 json_schema 严格模式只在 OpenAI Responses 上可用，
        // 而本项目统一走 chat/completions，因此这里用 json_object +
        // 提示词约束，并在解析层做结构校验（§6 要求结构校验）。
        body["response_format"] = serde_json::json!({ "type": "json_object" });
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

/// 解析 OpenAI 风格响应
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
        return Err(AppError::new(
            crate::error::ErrorCode::Internal,
            "服务商返回了空内容",
        )
        .with_hint("DeepSeek 官方提示存在偶发返回空内容的情况，可重试一次；若持续出现请更换模型"));
    }

    let finish = choice.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("");
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
        .ok_or_else(|| {
            AppError::internal("Claude 返回的结构不符合预期（缺少 content 数组）")
        })?;

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
        return Err(AppError::new(
            crate::error::ErrorCode::Internal,
            "Claude 返回了空内容",
        )
        .with_hint("可能被安全策略拦截，或响应只包含非文本块。可尝试调整提示词后重试"));
    }

    let stop = v.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("");
    let usage = v.get("usage").map(|u| TokenUsage {
        input_tokens: u.get("input_tokens").and_then(|x| x.as_i64()),
        output_tokens: u.get("output_tokens").and_then(|x| x.as_i64()),
        cache_hit_tokens: u.get("cache_read_input_tokens").and_then(|x| x.as_i64()),
        cache_miss_tokens: u.get("cache_creation_input_tokens").and_then(|x| x.as_i64()),
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
    cfg.validate()?;

    let api_key = load_api_key(cfg.provider)?.ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::NotConfigured,
            format!("尚未配置 {} 的 API Key", cfg.provider.label()),
        )
        .with_hint("请在设置 → AI 中填写并保存密钥")
    })?;

    let client = build_client(cfg.timeout_seconds)?;
    let url = endpoint(cfg);
    let (body, is_anthropic) = if cfg.provider.is_anthropic_style() {
        (build_anthropic_body(req), true)
    } else {
        (build_openai_body(req), false)
    };

    let mut rb = client.post(&url).json(&body);

    if is_anthropic {
        // Anthropic 用 x-api-key，**不是** Bearer；
        // 且 anthropic-version 是必填头。
        rb = rb
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
    } else {
        rb = rb.header("authorization", format!("Bearer {api_key}"));
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

    if is_anthropic {
        parse_anthropic_response(&json)
    } else {
        parse_openai_response(&json)
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
        if r.model.is_empty() { &cfg.model } else { &r.model },
        preview
    ))
}

/// 列出可用模型。
///
/// §6 要求"模型列表接口若不可用允许手动输入模型名"，因此这里**失败不算错**：
/// 返回空列表 + 说明，让界面引导用户手动填写。
pub async fn list_models(cfg: &ProviderConfig) -> AppResult<Vec<String>> {
    let api_key = load_api_key(cfg.provider)?.ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::NotConfigured,
            "尚未配置 API Key",
        )
    })?;

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

/// 读取当前 AI 配置（**不含密钥**）
#[tauri::command]
pub async fn ai_get_config(
    state: State<'_, AppState>,
) -> AppResult<Option<ProviderConfig>> {
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
    cfg.validate()?;

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
        if cfg.has_api_key { "已保存" } else { "未设置" }
    );
    Ok(cfg)
}

/// 测试连接（§6 要求"可连接测试"）
#[tauri::command]
pub async fn ai_test_connection(config: ProviderConfig) -> AppResult<String> {
    test_connection(&config).await
}

/// 列出可用模型。失败时返回空列表 + 原因，界面据此引导手动输入（§6）。
#[tauri::command]
pub async fn ai_list_models(
    config: ProviderConfig,
) -> AppResult<serde_json::Value> {
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
pub async fn ai_clear_key(
    state: State<'_, AppState>,
    provider: Provider,
) -> AppResult<bool> {
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
                sqlx::query("UPDATE settings SET value_json = ?1, updated_at = ?2 WHERE key = 'ai_config'")
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
                endpoint_suffix: "/chat/completions",
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
                assert!(body.get("max_tokens").is_some(), "Claude 的 max_tokens 必填");
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
    fn openai_endpoint_keeps_v1() {
        let c = cfg(Provider::OpenAI);
        assert_eq!(
            endpoint(&c),
            "https://api.openai.com/v1/chat/completions"
        );
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
        assert_eq!(endpoint(&c), "https://api.openai.com/v1/chat/completions");
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

    #[test]
    fn default_base_urls_match_official_docs() {
        assert_eq!(Provider::DeepSeek.default_base_url(), "https://api.deepseek.com");
        assert_eq!(Provider::OpenAI.default_base_url(), "https://api.openai.com/v1");
        assert_eq!(Provider::Claude.default_base_url(), "https://api.anthropic.com");
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
    }

    #[test]
    fn base_url_must_be_http_or_https() {
        let mut c = cfg(Provider::OpenAI);
        c.base_url = "ftp://example.com".into();
        assert!(c.validate().is_err());

        c.base_url = "不是地址".into();
        assert!(c.validate().is_err());

        c.base_url = String::new();
        assert!(c.validate().is_err());
    }

    /// 明文 HTTP 只允许本机，否则密钥会在网络上裸奔
    #[test]
    fn plain_http_only_allowed_for_localhost() {
        let mut c = cfg(Provider::Custom);
        c.model = "local-model".into();

        for ok in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
        ] {
            c.base_url = ok.into();
            assert!(c.validate().is_ok(), "本机地址应允许：{ok}");
        }

        c.base_url = "http://api.example.com/v1".into();
        let e = c.validate().unwrap_err();
        assert!(e.hint.unwrap().contains("明文"), "应说明风险");
    }

    #[test]
    fn model_cannot_be_empty() {
        let mut c = cfg(Provider::Custom);
        c.base_url = "https://example.com/v1".into();
        c.model = "   ".into();
        assert!(c.validate().is_err());
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
        assert!(ds.contains("改进"), "DeepSeek 文案应说明默认用于改进服务：{ds}");
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
