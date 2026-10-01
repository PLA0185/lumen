# DeepSeek Harness 接入调研

核对日期：2026-10-02。目的：判断 Harness 是否能帮助 Lumen 实现流程知识库聊天、文件理解和多轮追问。用户选择先做隔离验证，再作为可选问答引擎接入。

## 结论

可以接入。官方提供 TypeScript、Python SDK 和独立运行程序，不必更换 Lumen 的 Tauri 窗口。Harness 能复用 Agent 循环、工具调用、会话事件及历史管理；流程检索、访问范围、来源引用、步骤定位和 24 小时缓存仍需要 Lumen 实现。此结论是文档与接口评估，不等于已完成正式集成。

来源：[官方 SDK](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/client/README.md)、[架构](https://deepseek-harness.github.io/deepseek-harness/reference/)。

## 功能与接入边界

| 用户需求 | Harness 能提供的基础 | Lumen 要完成的工作 |
| --- | --- | --- |
| 像聊天一样问流程 | 多轮会话、事件、模型与工具调度 | 悬浮入口、聊天面板、错误重试、短期缓存 |
| 当前流程或全部流程 | 自定义工具、MCP 客户端 | SQLite 检索，强制限定所选范围，排除回收站 |
| ERP 发货单在哪、怎么生成 | 模型可按需调用检索工具 | 返回流程和步骤编号、证据、版本；答案可点击定位 |
| 多个相近答案先追问 | 模型能继续对话 | 候选选择及下一轮反馈，不能假定 SDK 已接好官方询问 UI |
| Word、Excel 等附件 | Office 技能和转换工具 | 文件读取及来源位置、OCR、格式错误传播、原件保留 |
| 卡片画布、拖动和缩放 | 不负责这些界面行为 | 沿用现有步骤顺序、编辑及保存规则 |

[工具开发](https://deepseek-harness.github.io/deepseek-harness/develop/basic/tool)、[MCP](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/mcp/mcp-client/README.md)、[Office 技能](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/skill/skill-office/README.md)。

## 实际接口

- SDK 用子进程 stdio 的逐行 JSON-RPC 驱动 `dsh`，不是浏览器内的普通 React 依赖。Windows 可以用独立运行程序，Python SDK 的原生 wheel 路线无需系统 Node；完整资源布局仍需验证和打包。
- 官方 Web Host 有 HTTP RPC 与 WebSocket，但不是已经替 Lumen 设计好的“上传任意文件并检索回答”的 REST 接口。首选 SDK 路线，而不是套一个官方 Web 页面。
- 当前源码文档说明 SDK 没有逐轮取消接口，放弃运行需要关闭运行进程；服务端向 SDK 客户端的请求尚未实现。消歧可以由 Lumen 展示候选，再将用户选择作为下一轮输入。
- Office 的技能提供指令和脚本，部署方还需提供解释器和相应库。普通文件 `read` 仅支持 UTF-8；不能由 Office 技能存在推出所有 PDF、图片、加密或旧版 Office 文件均已准确识别。

[Python SDK](https://deepseek-harness.github.io/deepseek-harness/guide/python-sdk)、[SDK 协议](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/protocol/README.md)、[运行时发行](https://github.com/deepseek-ai/deepseek-harness/blob/master/python/sdk-runtime/README.md)、[文件读取](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/fs/tool-fs/README.md)。

## 建议的产品接入方式

Lumen 保留原有 DeepSeek 配置，新增可选 Harness 引擎；通过独立运行进程调用只读流程检索工具。界面仍使用 Lumen 的悬浮聊天窗与顺序画布。当前流程范围严格限定当前记录；全部流程范围先检索，再读取有限证据，不把整个库每次全发出去。

默认 SDK 示例包含 Shell 和较大的访问权限，工作目录不是访问隔离。问答配置应移除 Shell、写文件、插件安装等无关工具，并关闭默认会话日志上传；工具访问由 Lumen 后端校验，不靠提示词约束。当前属于 developer preview，官方明确可能发生破坏兼容的变化，正式包应固定版本及完整性摘要。

[默认配置](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/bundle/sdk-minimal/cordis.patch.yml)、[会话日志](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/session/session-log-deepseek/README.md)、[官方安全说明](https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md)、[预览声明](https://github.com/deepseek-ai/deepseek-harness)。

## 本轮验证状态

已进行官方文档、源码与发行元数据核对。npm 的 SDK `latest` 与运行程序 `latest` 指向不同版本，不能无条件组合安装；PyPI 提供 Windows 原生运行时，但与 npm 发行线版本不同。

隔离 SDK 实机验证实际通过：官方 PyPI Windows runtime `0.1.5rc1` 与同版 npm SDK/protocol `0.1.5-rc.1`，均校验官方 SHA256/SHA512。真实原生运行程序、官方 SDK 及自定义插件成功启动；模型实际调用只读 `search_flows` / `get_flow`；同一会话两轮保留历史；HTTP 503 产生明确错误事件；重复关闭成功；关闭挂起请求返回 `TransportClosedError` 并确认原生进程退出。六次模型请求均到本机模拟服务，工具列表仅有两个只读工具，没有会话日志上传字段。

这是接入链路验证，不是模型准确率验收。npm `0.2.0-rc.2` 包仅完成接口与哈希核对，完整最新运行时未验证；重启后的会话恢复、正式 Tauri 打包、用户业务资料准确率、真实 DeepSeek 模型、多设备知识库问答均未验证。未访问生产密钥或向模型发送用户附件。该 Windows wheel 压缩约 68.7 MiB，主程序解压约 229 MiB，正式接入还要考虑额外运行资源和安装包体积。

本机 npm TLS 校验失败；临时 registry 转发服务被自动审批拒绝，返回仅为 `blocked by policy`，没有重试或关闭 TLS 校验。改用独立的官方 HTTPS 下载并校验发行件，完成上述真实验证。

本机完整证据位于仓库外 `D:/Codex/lumen-verification-20261002/harness-spike-062253/`，包括可复跑脚本、模拟请求、真实错误事件、进程退出、发行哈希和验证结果；不将运行程序或会话缓存提交到 Git。
