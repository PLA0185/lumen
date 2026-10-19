# ChatGPT 内容导入（0.4.55）

## 当前可用方式

Lumen 在“备忘与流程”提供“ChatGPT 导入”。新建内容使用同一份通用提示词，由 GPT 根据内容判断保存为备忘还是流程；若用户从另一个类型的列表发起导入，保存后会自动切到识别出的类型。更新已有记录时提示词会要求 GPT 保持原类型。ChatGPT 网页版和桌面版均可复制回答并粘贴到 Lumen。导入解析只生成本地预览；只有用户点击“确认并保存”后才调用备忘 / 流程保存接口。更新已有记录时带原版本号，若内容已在别处修改，保存会返回冲突，预览会留在窗口中。

普通 Markdown 和文本可直接导入。新建备忘会把首个一级标题作为标题；流程 Markdown 使用一级标题作为流程名、二级及以下标题作为步骤标题，标题后的内容（包括 `lumen-asset:` 图片引用）放在对应步骤。更新已有流程时，旧图片按完全匹配的步骤标题留在原步骤；标题已改或步骤被移除的图片会移入预览的待分配内容，不会被静默丢弃。也接受 JSON 格式的 `title`、`category`、`kind`、`bodyMd` 和 `steps`。流程预览允许把原始材料中的图片重新分配到某个步骤；仍有未分配图片或步骤标题为空时，不允许确认保存。

“复制通用提示词”同时说明备忘与流程两种 JSON 结构及判定规则。备忘输出 `kind: "memo"`、正文和空的 `steps`；有明确先后操作的内容输出 `kind: "flow"`，并按顺序生成步骤。每次导入一条记录；整理流程时，可让 ChatGPT 按每一步标题组织文字，再把图片粘贴到对应步骤附近；或先粘贴图片，在预览中使用“从原始材料添加图片”指定归属。

粘贴或拖入的图片、文件会先导入本机资源库，预览确认后才关联到备忘 / 流程记录。取消导入不会修改现有记录；未使用的本机资源不会被自动关联。

## 直连状态

此版本没有实现 ChatGPT 原生 MCP 连接。OpenAI 官方当前说明：包含写入 / 修改操作的完整 MCP 支持处于 beta，面向 Business、Enterprise 和 Edu；其文档没有将 Plus 列为完整写入可用计划。ChatGPT 不直接连接本机 MCP 服务，私有网络部署需要 Secure MCP Tunnel；开始前要有 Platform `tunnel_id`、`tunnel-client` 运行时 API Key、可访问的 MCP 服务，并且 ChatGPT 工作区要允许自定义 MCP。

用户目前是 ChatGPT Plus，也没有 OpenAI Platform 隧道凭据，所以不能在该账号上实际启用 ChatGPT → Lumen 直连写入。复制粘贴是当前可用的跨桌面端 / 网页端路径；以后具备官方写入权限与隧道后，仍需实现和验收 MCP 服务端及权限配置，不能把本导入窗口当作已完成的原生连接。

官方依据：

- [OpenAI：ChatGPT 中的开发者模式和 MCP 应用](https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt)
- [OpenAI：Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)
