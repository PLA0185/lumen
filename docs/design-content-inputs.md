# 图片、文件与 AI 内容输入（2026-09-30）

## 已落地的使用方式

任务新建 / 编辑的描述、Markdown 备注、备忘正文、流程步骤说明以及 AI 助手输入，都使用
同一个内容编辑器。支持 Ctrl+V / Ctrl+Shift+V 从当前 Windows 剪贴板读取图片或文本，
拖入文件与选择文件。文件保存后插入 Markdown 引用，点击“预览内容”可查看图片和附件。
任务展开、备忘查看与流程步骤查看使用相同的安全渲染组件。

- 图片识别 PNG、JPEG、WebP、GIF 的文件头；不执行 SVG / HTML 文件。
- 一般文件作为可另存为的资源保存，不冒充文档预览。原文件不会被移动或删除。
- 单文件最多 **20 MiB**。每次 AI 请求最多 10 个文件，总计最多 20 MiB；文本文件合计
  最多 100000 字，必须使用 UTF-8。文件导入失败会显示错误，保留已有输入；一批导入部分
  成功时保留成功文件的引用并展示失败原因。
- 导入期间如果文字、光标或编辑位置变化，不用旧文本覆盖新输入，显示重新粘贴的提示。
- 内容资源导入即写入本地 SQLite；正文仍按各页面的保存操作写入，不会自动发送到 AI。

## 存储和恢复

正式迁移 `0010_content_assets.sql` 创建不可变资源表，保存名称、实际 MIME、Base64
内容、长度、SHA-256 和创建时间。正文引用 `lumen-asset:<UUID>`，不依赖原路径。
迁移沿用数据库已有的一致性快照机制，失败即中止升级。

资源读取、AI 发送和备份恢复校验长度、SHA-256 与 MIME。备份格式 **5** 保存资源本体，
旧格式 1–4 仍可恢复；空资源集合不改变旧格式数据校验和。传统任务“仅记录 / 复制到附件
目录”的附件仍只备份清单，界面的未包含附件提醒继续有效。

删除正文中的引用、取消编辑或者删除任务不自动清理资源。这样不会误删撤销、历史重复
任务或者其他备忘仍引用的文件；**本轮未实现未引用资源清理，会占用磁盘空间**。
JSON / SQLite 备份包含业务文件内容，应当按业务数据保管；不会包含 AI 密钥。

## AI 实际传输

只有点击“生成”后，当前输入引用的资源 ID 才由后端读取并发送。任务整理、排程补充说明、
日 / 周 / 月 / 年总结均接入文件内容；不会扫描和上传其他备忘或任意本机路径。

| 当前适配器 | 图片 | PDF | Word / Excel / PowerPoint 等 | UTF-8 文本 |
| --- | --- | --- | --- | --- |
| OpenAI Responses | `input_image` + Base64 data URL | `input_file` | `input_file` | 读取真实文本并加入 `input_text` |
| Anthropic Messages | `image` + Base64 source | `document` | 明确拒绝，提示转 PDF / 文本 | 读取真实文本并加入 `text` |
| DeepSeek Chat Completions | `image_url` + Base64 data URL | 明确拒绝 | 明确拒绝 | 读取真实文本并加入 `text` |
| 自定义 Chat Completions | `image_url` | `file` | 明确拒绝 | 读取真实文本并加入 `text` |

服务商接口支持不等于用户选的每个模型都支持。保留用户模型，模型或自定义部署拒绝格式时，
把真实错误显示出来。Claude 图片的 Base64 编码另限制 10000000 字节；动图的分析能力由
服务商决定，Claude 只使用首帧。压缩包、安装包等仍可本地保存，但不能作为 AI 分析文件。

本轮核对的官方说明：

- [OpenAI 文件输入](https://developers.openai.com/api/docs/guides/file-inputs)：Responses
  接受常见文档 / 表格类型，PDF 可使用 Base64 `file_data`；Chat Completions 文件输入仅 PDF。
- [OpenAI 图片输入](https://developers.openai.com/api/docs/guides/images-vision)：PNG、JPEG、
  WebP、非动画 GIF，支持 data URL。
- [Claude 图片](https://platform.claude.com/docs/en/build-with-claude/vision) 与
  [PDF](https://platform.claude.com/docs/en/build-with-claude/pdf-support)：原生图片 / PDF
  内容块，Word / Excel 需转换，不把文件名当正文。
- [DeepSeek 图片输入](https://api-docs.deepseek.com/guides/vision/)：最新文档明确
  `deepseek-flash` 接受图片，使用标准 Chat Completions `image_url` 内容块。

## 与云同步的关系

内容引用和资源本体必须作为同一次同步提交的依赖处理，不能先宣布备忘同步成功却没有
上传图片。`docs/design-cloud-sync.md` 描述的同步引擎 / 外部服务接入仍是设计，**尚未实现**；
本地资源和完整备份不能被称为跨电脑云同步。真实服务商的图片识别效果与网盘端到端同步
都需要实际账号验证，不能用本地请求体测试替代。
