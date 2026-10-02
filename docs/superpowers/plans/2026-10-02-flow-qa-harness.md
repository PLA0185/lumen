# 流程知识库问答与可选 Harness 引擎 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在全屏流程画布上提供悬浮聊天、可靠的流程检索与来源跳转，并接入经隔离验证的可选 Harness 引擎。

**Architecture:** Rust 持有授权范围、检索证据和会话，React 只展示聊天及操作来源。直接模型接口与 Harness 共享只读检索契约；Harness 经独立 Node 工作进程调用官方 SDK，通过受控消息回调 Rust 工具。聊天缓存与业务数据库分离，运行资源随安装包提供。

**Tech Stack:** 现有 React/TypeScript、Tauri 2、Rust/sqlx、Windows 凭据管理器；官方 SDK/protocol `0.1.5-rc.1`、Windows runtime `0.1.5rc1`；固定版本独立 Node 24。

**Spec:** [用户已确认的设计](../../design-flow-qa-harness-2026-10-02.md)。依据：[实际 SDK 隔离验证](../../deepseek-harness-research-2026-10-02.md)。

用户补充（2026-10-02）：保留左侧菜单和全部顶部工具栏，画布只铺满剩余区域；添加步骤居中；流程图导出支持格式、尺寸、清晰度及常规设置；全部应用快捷键在设置中自定义，直接滚轮缩放可选并默认开启。任务 4 集成画布行为；独立任务 7 完成导出、任务 8 完成快捷键。执行顺序调整为 **1 → 9 → 10 → 8 → 7 → 2 → 3 → 4 → 5 → 6**，所有步骤仍逐项独立复核；期间真实验收发现的缺陷先修复。

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

**Interfaces:** async `KnowledgeContext::load(db: &Db, scope: QaScope) -> AppResult<Self>`；`search(&self, query: &str, exact: bool, limit: usize) -> AppResult<Vec<QaCandidate>>`；`get_flow(&self, flow_id: Option<&str>, step_id: Option<&str>) -> AppResult<MemoDocument>`；async `read_attachment(&mut self, db: &Db, asset_id: &str, offset: usize) -> AppResult<AttachmentEvidence>`；async `validate_citation(&self, db: &Db, citation: &QaCitation) -> AppResult<()>`。证据分段最多 8,000 字；偏移须在已授权内容范围内，返回是否还有下一段。复用 `memos::list_impl/get_impl`、`content_assets::get_asset`，知识读取使用 `document_import::extract_asset_read_only`，不写内嵌资源到业务库。

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

### Task 7: 流程图导出与真实设置

**Files:** Create `src/lib/flow-map-export.ts`、`flow-map-export.test.ts`、`src/components/FlowExportDialog.tsx`、`FlowExportDialog.test.tsx`、`src-tauri/src/flow_export.rs`、`tools/verify_flow_export_ui.py`; Modify `FlowCanvas.tsx`、`MemosView.tsx`、`src/styles.css`、`src/lib/ipc.ts`、`src-tauri/src/pdf.rs`、`src-tauri/src/lib.rs`。共享卡片位置/摘要逻辑可提取到 `src/lib/flow-canvas-layout.ts`，画布与导出调用同一实现；不能复制两套布局。

**Interfaces:** `FlowExportFormat = 'png'|'jpeg'|'webp'|'svg'|'pdf'`；`FlowExportOptions = { format; scope: 'all'|'viewport'; width: number; height: number; lockAspect: boolean; quality: number; background: 'current'|'light'|'dark'|'transparent'|'custom'; backgroundColor: string; paperWidthMm: number; paperHeightMm: number; landscape: boolean; marginMm: number; pdfLayout: 'single'|'pages' }`。`renderFlowMap({title,steps,view,viewport,options}) -> Promise<{blob:Blob,width:number,height:number}>`；`saveFlowMap(...) -> Promise<string|null>`。FlowCanvas 新增 `title?:string`，导出可编辑草稿但不确认保存。Rust `flow_export_write(path:String, format:FlowExportFormat, data_base64:String) -> AppResult<()>` 仅写用户选定路径；校验扩展名、真实文件头和有界字节数。`export_pdf` 新增可选 `options: Option<PdfOptions>`，既有任务导出不传参数仍使用原默认设置；PdfOptions 字段为纸张宽高、landscape、marginMm。

- [ ] 写可编译接口与行为红测试：整图包含首尾 100 步/连线；当前视野正确裁切；透明 PNG/不透明 JPG；比例锁定尺寸；1×/2×/4× 真实像素；质量改变 JPG/WebP 编码；超 16,384px 或 64,000,000 像素拒绝；标题/内容中的 XML/脚本仅作为文字；保存取消不返回成功；后端拒绝格式/文件头不一致；PDF 设置无效或 Set* 失败向上报错；打印后主界面恢复；纸张尺寸、方向、边距与分页真实生效。
- [ ] 运行 `pnpm exec vitest run src/lib/flow-map-export.test.ts src/components/FlowExportDialog.test.tsx` 与 `cargo test --lib flow_export`、`cargo test --lib pdf`，记录行为红，不将编译失败当完成 TDD。
- [ ] 实现原生 SVG 文字和连线，共享画布位置/摘要；同一 SVG 用浏览器 Canvas 编码真实 PNG/JPG/WebP。栅格默认 2×、质量 90；不用没有实际写入元数据的 DPI 数字。PDF DOM 挂在 `.app` 外，分页按整行卡片切片并加继续标识，实际设置传到 WebView2，每个设置错误都返回。
- [ ] 实现预览与设置：所有设计中的格式/尺寸/质量/背景/纸张/方向/边距/分页可真实操作，格式无效的设置隐藏或禁用并解释；文件大小来自已生成 Blob。使用系统保存对话框选择路径，选择前不写文件；导出不修改业务状态，取消和错误保留草稿。
- [ ] 重跑针对测试全绿；真实隔离窗口实际鼠标逐类保存文件，检查魔数、宽高、透明通道、JPEG/WebP 不同质量体积、SVG 可重新打开、PDF 页数/MediaBox 与中文可提取、100 步完整图；观察设置和图一致，记录耗时与文件体积。测试原件和业务行数不变。
- [ ] 更新四部分工作记录，独立提交 `feat(flow-export): 支持流程图多格式导出与可调设置`；提交前跑全部仓库门禁，交给独立任务复核再进行任务 2。

### Task 8: 统一可自定义快捷键与画布手势

**Files:** Create `src/lib/shortcuts.ts`、`shortcuts.test.ts`、`src/components/ShortcutSettings.tsx` 与测试；Modify `src/lib/window-ipc.ts`、`src-tauri/src/window_mgr.rs`、`src-tauri/src/shortcuts.rs`、`SettingsView.tsx`、`WindowSettings.tsx`、`App.tsx`、`FlowCanvas.tsx` 及现有上下文快捷键组件与测试。清单依据 `docs/shortcut-inventory-2026-10-02.md`，实际操作完整纳入，不只修改三个主窗口组合键。

**Interfaces:** 现有 WindowConfig 四个全局字段保留，新增默认化 `localShortcuts: Record<string,string[]>`、`canvasInput: {panHold:string;wheelZoom:'direct'|'alt'|'ctrl'|'shift'|'off'}`；老 JSON 缺字段时使用默认值，无需业务库迁移。前端统一 `matchesShortcut(event, bindings)` 严格匹配修饰键并排除 composition/defaultPrevented，局部 Enter/Escape/F2 沿用组件作用域。画布默认直接滚轮缩放，可选择修饰键或关闭；编辑框滚轮正常滚动。所有配置经现有 windowGetConfig/windowSetConfig 和 window-config-changed 更新、持久化，不建另一套保存接口。

- [ ] 写行为红测试：旧配置完整保留；所有清单命令可修改并立即生效；默认/恢复默认；等价全局组合键冲突；OS 注册失败不保存成功并恢复旧注册；编辑输入/IME/严格修饰键不误触；嵌套对话框仅处理自身取消；按住可配置平移键和鼠标拖动；直接滚轮默认、Alt 模式、关闭模式、重开后记忆；画布输入内不缩放。
- [ ] 设置中分组显示全局、应用、上下文、画布，使用按键录入和可移除的多绑定，说明作用范围；允许取消绑定。相同作用域冲突阻止保存，互斥局部上下文可复用 Enter/Escape。系统保留键和编辑器复制/粘贴/撤销、Tab/IME 保留其系统行为，不把它们冒充应用可配置命令。
- [ ] 后端完整预校验全局组合，使用解析后的规范键身份比较，注册失败回滚旧组合、返回错误；UI 不伪报已保存。恢复路径以真实可注册快捷键/托盘为依据。
- [ ] 替换清单中全部硬编码应用操作匹配，保留文本输入保护与无障碍焦点操作；配置变化读取最新值，不能空依赖闭包锁住旧手势。不同窗口均收到设置更新。
- [ ] 覆盖测试与真实隔离窗口鼠标/键盘验收：修改新建/搜索/发送/取消/画布平移，验证旧键失效新键生效、关闭重开仍生效；全局占用故障可见且配置未保存；原菜单栏保留、输入不触发 Windows 菜单。
- [ ] 全门禁、四部分工作记录、独立中文提交，独立任务复核；已有服务商、凭据和业务记录保持。

新增任务执行顺序：任务 1 完成复核后，先修复真实验收发现的 Word 内嵌图生成遗漏，再执行任务 8、7，随后恢复任务 2、3、4、5、6。任务范围来自用户继续补充的明确要求，无需重复设计确认。


### Task 9: 用户反馈的 AI 状态与模型选择（已完成源码及隔离验收）

成功保存 AI 配置后现有助手刷新，读取失败和未配置分开；助手/流程生成读取账号实际模型列表，选择后保存并保留密钥。连接测试成功不代替配置保存。共用成功写入通知，旧异步读取不能覆盖新状态。以 React 先红后绿和本机 HTTP 原生请求验证，真实模型/正式安装在集成阶段验证。

### Task 10: 用户追加的全页面同步入口及默认类型（已完成源码及隔离验收）

在全局顶部栏增加手动选择入口；内容为全部、任务及关联业务、备忘/流程；方向为双向、仅上传、仅下载。默认保存在本机原连接 JSON，自动同步采用默认；手动选项仅本次有效，暂停自动同步时仍能执行一次。只下载绝不写远端或清除待上传，只上传不导入远端正文；未选范围保留。旧字段兼容、不清库、不重新索要密码。实际 HTTP 两库 9 种矩阵、真实 UI 保存和重载验证。

执行调整：9/10 是用户实时反馈优先处理；委派子代理因账户额度在开始实现前失败，已告知用户并继续内联执行；独立复核未完成须如实记录。


### Task 11: 实际使用反馈的流程阅读与导航、顶部布局、空态刷新

用户明确修订：横向或纵向均可，卡片保留足够间距且不得蛇形折返；完整显示文字与图片，打开保持可阅读比例；选步骤只显示该步骤编辑，流程整体信息单独打开。顶部提供类似位置标记的节点导航，悬停显示标题，点击定位并不强制编辑。默认中键拖动、直接滚轮缩放；可选快捷键工作仍属于 Task 8。整体顶部栏左侧标题/说明按截图上下排列，中央按钮均匀分布，搜索靠右、紧邻最右导出。空态后台刷新不卸载已显示内容。

AI 转换只忠实保留原文，不主动提出材料、前置条件、审阅问题或待确认要求；负责人/说明未知留空，原文明确的信息保留。现有用户正文不自动清除。新增行为必须验证真实鼠标、动态图片高度、排列切换、导航定位、刷新稳定性与窗口/字体组合。

### Task 12–15: 图片操作、可调卡片、共用断行与画布内编辑（用户实时修订）

执行在 Task 8 已保存恢复点之后，优先满足实际流程阅读与编辑，随后继续原计划。既有系统图像资源和版本条件写入复用，不破坏原图、不清库。

- 图片下不显示文件名/大小；单击打开独立查看器。可选备注保存在 Markdown 图片标题，空备注不显示；实际提供手绘、箭头、文字、选择删除标注、撤销/重做、放大缩小、适应窗口、另存为、从当前流程移除。保存标注生成新的图片资源替换当前引用，原图与其它流程的引用保留；关闭未保存标注保留当前查看器草稿直到本次会话结束。编辑只在可写流程内保存，纯查看仍可另存为。所有失败明确返回，取消保存不伪报成功。
- 单击卡片选中，双击在卡片内编辑标题/负责人/正文，去掉重复的步骤侧栏；添加步骤直接进入新卡片编辑并居中。整体流程信息仍从独立入口访问。图片操作不触发步骤编辑；输入与图片工具不误触画布平移/缩放。
- 单独拖动与缩放卡片；可读最小宽度，高度至少容纳完整内容，文本/图片随宽度排版。大小和手动位置在步骤 JSON 的可选 layout 保存，老步骤缺字段沿用默认；原 schema 不变，严格校验有限数值/边界。连接线使用实际卡片边缘。提供真实自动排版，清除手动位置、按横向/纵向顺序重新对齐，保留用户设置的大小并留适当间距。
- 共用中文防孤字断行规则用于 Markdown 与流程标题/备注，保持复制文本不变；单字来自自然换行的残行必须消除。卡片缩小、字号变化、混合中文/标点/数字、强调与列表均验证；不以隐藏/省略全文解决。
- 上方流程名称为可打开的下拉切换入口，实际加载其它未删除流程；切换前沿用现有自动保存/冲突处理，失败不吞掉草稿。
- AI 新生成结果去掉原材料没有的审阅/完成/例外要求，原文明示的对应段落保留。当前保存流程核对发现原始材料无这些标题、生成部分却有；对旧内容的修复需保留原文、图片及用户输入，并通过版本条件写入与历史记录验证。

验收：行为红绿、完整仓库门禁、真实 WebView2 点击/双击/拖动/缩放/标注/保存/切换与重开持久化；检查新旧图像像素/文件头和原资源不变；中文逐行测量，不以截图观感代替。正式安装、最新 CI 与公开签名发布另记真实结果。

### Task 16: Word 原文章节与图文位置、AI 重排补写保护（实时反馈优先）

核对原始 DOCX、XML 段落样式/关系、已生成步骤，先复现导入丢失位置和模型重复图片。恢复章节及图片原位置，以明确编号章节构造最终步骤；同步约束 AI 提示、正文单次发送、重复配图和无来源要求校验。原文有的照原文保留，未知负责人留空；普通识别不能再把定位图片堆到末尾。修正现有错误流程通过版本条件保存保留历史，不修改 Word 原文件。独立 XML 清单对照实际文档，完整门禁、原生生成/保存/重开与正式安装后再记交付结果。根因说明见 [Word 流程忠实性](../../word-flow-fidelity-2026-10-02.md)。
