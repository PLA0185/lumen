# 流程知识库问答与可选 Harness 引擎 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在全屏流程画布上提供悬浮聊天、可靠的流程检索与来源跳转，并接入经隔离验证的可选 Harness 引擎。

**Architecture:** Rust 持有授权范围、检索证据和会话，React 只展示聊天及操作来源。直接模型接口与 Harness 共享只读检索契约；Harness 经独立 Node 工作进程调用官方 SDK，通过受控消息回调 Rust 工具。聊天缓存与业务数据库分离，运行资源随安装包提供。

**Tech Stack:** 现有 React/TypeScript、Tauri 2、Rust/sqlx、Windows 凭据管理器；官方 SDK/protocol `0.1.5-rc.1`、Windows runtime `0.1.5rc1`；固定版本独立 Node 24。

**Spec:** [用户已确认的设计](../../design-flow-qa-harness-2026-10-02.md)。依据：[实际 SDK 隔离验证](../../deepseek-harness-research-2026-10-02.md)。

## Global Constraints

- 当前范围包含未保存草稿；全部范围只读已保存、未删除的流程，排除普通备忘、任务、回收站。
- 缓存从最后一条消息起 **24 小时**有效；收起不停止回复；清空不删除业务记录或附件。
- `search_flows` 每次最多 **10 个结果**；多个相近候选先确认对象；下一步按真实步骤索引回答。
- 引用包含 `flowId、stepId、revision、attachmentId（可空）、locator、excerpt`；草稿另含内容摘要。
- 负责人可以为空；拖入文件不解析，明确识别或问答需要该附件时才提取。
- 保留现有服务商、模型、凭据；问答引擎单独选择；禁止静默切引擎、自动重复付费调用。
- Harness 不开放 Shell、任意文件读写、网页、插件安装或会话上传；不依赖开发环境 Node。
- 不改变业务表结构，不清空 Home 配置；密钥、运行程序、聊天缓存、日志绝不提交 Git。
- 每类独立中文 commit；工作记录四部分随代码推送；全门禁和 CI 真实成功后才宣告完成。

## Review Focus

1. 问答期间流程被编辑/删除，旧来源必须报过期，不能跳到另一条记录（任务 1、4）。
2. 输入带恶意附件路径、跨流程编号或提示指令时，工具范围仍由 Rust 控制（任务 1、3）。
3. 停止后立即重发、收起时回复到达，不能串会话或丢问题（任务 2、3、4）。
4. 时钟回拨、损坏缓存、写盘失败时不能伪成功，也不能递归删除业务目录（任务 2）。
5. 新电脑无 Node、运行程序旁文件缺失或非 ASCII 安装路径时，要么正常启动要么明确失败（任务 5）。

## 文件职责与共享类型

新增 `src-tauri/src/flow_qa/`：`mod.rs` IPC/状态，`knowledge.rs` 范围检索与证据，`session.rs` 缓存/生命周期，`direct.rs` 现有模型适配，`harness.rs` 受控进程。新增 `tools/harness-worker/worker.mjs` 与锁定依赖清单：仅官方 SDK 和工具消息桥；新增 `tools/prepare-harness.ps1`：校验固定发行资源，产物位于已忽略的 `src-tauri/resources/harness/`。新增前端 `src/lib/flow-qa-ipc.ts`、`src/components/FlowQaChat.tsx` 与对应测试；修改 `MemosView.tsx` 连接画布、草稿及来源定位。

共享 DTO（Rust 使用 serde camelCase，TypeScript 同名字段）：

```typescript
type QaEngine = 'direct' | 'harness'
type QaScope = { kind: 'current'; current: SaveMemoInput } | { kind: 'all' }
type QaCitation = { id: string; flowId: string | null; stepId: string | null; revision: number | null; draftHash: string | null; attachmentId: string | null; locator: string; excerpt: string }
type QaCandidate = { flowId: string | null; title: string; category: string; stepId: string | null; evidence: string }
type QaMessage = { id: string; role: 'user' | 'assistant'; text: string; citations: QaCitation[]; candidates: QaCandidate[]; error: string | null }
type QaSession = { id: string; engine: QaEngine; scope: QaScope; messages: QaMessage[]; lastMessageAt: string; status: 'idle' | 'running' | 'stopped' | 'error' }
```

### Task 1: 授权检索与可验证证据

**Files:** Create `src-tauri/src/flow_qa/{mod,knowledge}.rs`; Modify `src-tauri/src/lib.rs`; Tests 放 `knowledge.rs` 模块内。

**Interfaces:** `KnowledgeContext::load(db: &Db, scope: QaScope) -> AppResult<Self>`；`search(&self, query: &str, exact: bool, limit: usize) -> AppResult<Vec<QaCandidate>>`；`get_flow(&self, flow_id: Option<&str>, step_id: Option<&str>) -> AppResult<MemoDocument>`；`read_attachment(&mut self, db: &Db, asset_id: &str, offset: usize) -> AppResult<AttachmentEvidence>`；`validate_citation(&self, citation: &QaCitation) -> AppResult<()>`。证据分段最多 8,000 字；偏移须在已授权内容范围内，返回是否还有下一段。复用 `memos::list_impl/get_impl`、`content_assets::get_asset`、`document_import::extract_asset`。

- [ ] 写行为测试：current 不能取其它 ID；all 不包含 memo/trash；伪造 assetId/路径拒绝；版本/草稿摘要不一致拒绝；模糊与精准不同；按索引返回下一步；limit > 10 拒绝；扫描件提取警告保留。
- [ ] 运行 `cargo test --lib flow_qa::knowledge`，确认新增用例实际为红（先有可编译的最小接口，不把编译失败当行为测试）。
- [ ] 实现上述接口，空库返回空候选，不伪造答案；仅扫描授权正文中真实引用的附件编号。
- [ ] 同一命令重跑全绿，核对原件字节与业务库未被修改。
- [ ] 独立提交 `feat(flow-qa): 添加授权流程检索与来源校验`。

### Task 2: 会话、直接模型接口与 24 小时缓存

**Files:** Create `src-tauri/src/flow_qa/{session,direct}.rs`; Modify `mod.rs` 和 `src-tauri/src/lib.rs`; Tests 模块内。

**Interfaces:** IPC `flow_qa_restore() -> AppResult<Option<QaSession>>`；`flow_qa_send(input: QaSendInput) -> AppResult<QaSession>`（input：`sessionId: string|null, scope: QaScope, engine: QaEngine, text: string, candidateFlowId: string|null`）；`flow_qa_stop(session_id: String) -> AppResult<QaSession>`；`flow_qa_clear(session_id: String) -> AppResult<()>`。事件 `flow-qa-update` 负载 QaSession。`direct::answer(cfg: &ProviderConfig, context: &mut KnowledgeContext, history: &[QaMessage], text: &str) -> AppResult<QaMessage>` 复用 `ai::chat`，结构化结果经证据校验。

- [ ] 写测试：空库明确未找到；两个同等候选先输出选择卡；选中后只答该流程；两轮历史有效；拒绝未知证据；24 小时边界到期；时钟回拨拒绝恢复未来缓存；损坏缓存报告错误；写盘失败不能返回成功；清空只删会话专用目录。
- [ ] 运行 `cargo test --lib flow_qa::session` 和 `cargo test --lib flow_qa::direct`，确认行为红。
- [ ] 实现单会话运行标识与停止令牌；缓存采用临时文件写入后原子替换，恢复时将意外中断的 running 标为 stopped。凭据只由既有配置读取。
- [ ] 重跑上述测试全绿；本机模型夹具验证错误保留问题、停止后旧回复不能覆盖新请求。
- [ ] 独立提交 `feat(flow-qa): 添加对话服务与短期缓存`。

### Task 3: 官方 Harness 工作进程与只读工具桥

**Files:** Create `tools/harness-worker/{worker.mjs,package.json,pnpm-lock.yaml,worker.test.mjs}`、`src-tauri/src/flow_qa/harness.rs`; Modify `mod.rs`。

**Interfaces:** Rust `HarnessSession::start(resource_dir: &Path, session_dir: &Path, config: &ProviderConfig) -> AppResult<Self>`；`prompt(&mut self, text: &str, history: &[QaMessage], context: &mut KnowledgeContext) -> AppResult<QaMessage>`；`close(&mut self) -> AppResult<()>`。父子消息以 JSON 行传输：`start/prompt/toolResult/close` 请求、`ready/toolCall/message/error/closed` 响应，均携带 sessionId/requestId；工具只有 `search_flows/get_flow/read_attachment`，Rust 复用任务 1 校验。

- [ ] 在真实 SDK 与 loopback 模型下写并运行红测试：两轮调用、越界工具拒绝、503 非成功、停止关闭 PID、重启后带历史续聊；消息错误 ID 不得串线；缺少资源明确报错。
- [ ] 实现固定插件白名单与专用 home，保留隔离 spike 已验证的禁用项；标准输出只作协议，诊断信息脱敏。运行进程使用绝对资源路径和结构化参数。
- [ ] 对真实 SDK/runtime 重跑上述测试全绿，不替换成假 peer；停止只关闭当前问答进程；收起 UI 不触发 close。
- [ ] 独立提交 `feat(harness): 接入官方只读问答运行进程`。

### Task 4: 悬浮聊天与来源导航

**Files:** Create `src/lib/flow-qa-ipc.ts`、`src/components/FlowQaChat.tsx`、`FlowQaChat.test.tsx`; Modify `MemosView.tsx`、`FlowCanvas.tsx`、`MemosView.test.tsx`、`src/styles.css`。

**Interfaces:** `FlowQaChat({ scope, onScopeChange, onNavigate }: { scope: QaScope; onScopeChange(scope: QaScope): void; onNavigate(citation: QaCitation): Promise<void> })`；`FlowCanvas` 新增可选 `focusStepId: string|null`，定位 ID 而不是索引。前端 IPC 与任务 2 名称和参数完全一致。

- [ ] 写行为红测试：默认只有图标；展开/收起保留消息；回复在收起时继续缓存；切换范围不清历史；候选点击继续问答；未知/过期来源不导航；跨流程跳转沿用未保存保护；当前草稿来源不强制保存；清空只清聊天；空格在聊天输入中正常输入。
- [ ] 运行 `pnpm exec vitest run src/components/FlowQaChat.test.tsx src/components/MemosView.test.tsx`，确认行为红。
- [ ] 实现浮层、范围和引擎选择、发送/停止/重试、可点击来源及候选。未就绪 Harness 显示实际错误；聊天入口只在完整可用后加入，不添加假按钮。
- [ ] 重跑测试全绿；真实鼠标验收图标、来源定位、收起期间回复、精准/模糊搜索和画布手势。
- [ ] 独立提交 `feat(flow-qa): 添加悬浮聊天与步骤来源导航`。

### Task 5: 正式运行资源、安装与真实 DeepSeek 验收

**Files:** Create `tools/prepare-harness.ps1`、`tools/harness-resource-manifest.json`、`tools/verify_flow_qa_ui.py`; Modify `src-tauri/tauri.conf.json`、`.gitignore`、`.github/workflows/release.yml`（依实际现有发布流程）、`package.json`。

**Interfaces:** `prepare-harness.ps1 -OutputDirectory <absolute-path>` 下载固定 SDK/runtime/Node，逐件校验官方哈希；清单固定 URL、SHA256/SHA512、版本、必需旁文件与许可。构建失败必须中止，不以开发 Node 替代。`HarnessSession::start` 只读打包资源，写入只到会话专用目录。

- [ ] 先验证缺失文件、哈希错误、中文含空格安装路径、PATH 无 Node 时的红用例。
- [ ] 实现资源准备与 Tauri bundle 配置，资源目录忽略 Git；新机风格环境中 SDK/runtime 两轮实际调用、停止重启验证全绿。
- [ ] 使用现有 DeepSeek 配置，在专用隔离流程资料下验证 ERP 问题、两份相近流程追问、附件按需读取、来源忠实度；记录真实请求和 fixture 测试的区别，不把模拟回答算实际模型通过。
- [ ] 原始文件/截图/DOCX/XLSX/XLS/PDF/TXT 逐类验收；旧 DOC、PPT/PPTX 在真实转换通过前明确报告读取错误，不宣称支持。
- [ ] 独立提交 `build(harness): 打包经校验的独立问答运行资源`。

### Task 6: 门禁、文档、发布与数据保持

**Files:** Modify `docs/work-log.md`、`README.md`、`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`；必要验收报告放 `docs/`。

- [ ] 完整复核设计覆盖、实现与真实调用结果；记录所有未验证/未实现项，尤其格式转换、运行包体积与启动耗时。
- [ ] 跑 AGENTS 全门禁：`pnpm install --frozen-lockfile`、`pnpm typecheck`、`pnpm test`、`pnpm build`、`pnpm lint`、`cargo fmt --check`、`cargo test --lib`、`cargo clippy --all-targets --all-features -- -D warnings`。新问题先修复，不以旧一轮通过替代。
- [ ] 追加四部分工作记录和 README 索引；自查秘密/数据库/日志/构建产物未入库；统一版本到 0.4.10（若期间已有新版本，使用下一个补丁版本），提交并推送 main，等待最新 CI 全绿。
- [ ] 使用仓库签名脚本正式构建/发布；比较安装前后一致性快照和业务行数/配置状态，不打印凭据；安装后真实入口与运行资源复验。
- [ ] 文档收尾提交并推送，核对最终 CI，向用户报告可直接使用的结果与真实限制。

自审：检索/引用归任务 1；多轮、消歧与缓存归任务 2；可选实际引擎归任务 3；悬浮入口及导航归任务 4；独立部署与真实服务归任务 5；门禁、发布及记录归任务 6。Review Focus 的五项均分配了行为测试；各层共用 DTO，不以模型输出作为授权依据。
