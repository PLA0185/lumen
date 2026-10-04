# 知识库检索调研与本地融合方案

调研日期：2026-10-04。本文对照官方检索与分块指南、SQLite 全文检索文档，以及社区在解析丢失和引用幻觉方面的实际反馈，确定 Lumen 知识库的实现边界。

## 调研结论

- 检索增强生成的可靠性首先取决于检索到正确证据。微软的 RAG 指南建议按文档结构切块，并组合关键词、向量与元数据检索；对已有应用，先保留原有应用流程并把检索结果作为上下文，是低风险的起步方式。[Azure AI Search RAG 概述](https://learn.microsoft.com/en-us/azure/search/retrieval-augmented-generation-overview?tabs=docs)
- 分块应保留标题、段落及来源位置，过大的整篇文档会让内容被截断或使检索定位困难。微软专门建议按文档布局保留章节结构。[结构化文档切块指南](https://learn.microsoft.com/en-us/azure/search/vector-search-how-to-chunk-documents) [语义分块指南](https://learn.microsoft.com/en-us/azure/search/search-how-to-semantic-chunking)
- SQLite FTS5 提供本机全文索引、BM25 排序、摘要片段与高亮；trigram tokenizer 可做子串搜索，适合中文和混合语言文件。但短于三个 Unicode 字符的检索词不能单靠 trigram，因此需要短词回退，并用真实中文 fixture 验证。[SQLite FTS5 文档](https://www.sqlite.org/fts5.html)
- 增量索引应记录来源哈希，只在源内容变化时重新处理。LlamaIndex 的摄取流水线把文档元数据、哈希与缓存作为正式的一等机制，而不是每次问答都重新解析附件。[LlamaIndex Ingestion Pipeline](https://github.com/run-llama/llama_index/blob/main/docs/src/content/docs/framework/module_guides/loading/ingestion_pipeline/index.md)
- 解析器返回整篇文件、模型或接口却只消费有限长度，是常见的数据丢失来源；社区 issue 也记录了将模型生成的编号或 URL 当作可信引用会产生幻觉。引用 ID 应由本机应用生成并与具体原文片段绑定，不能由模型编造。[解析长度与分块讨论](https://github.com/run-llama/llama_index/discussions/12095) [引用索引幻觉问题](https://github.com/langchain-ai/langchain/issues/7239)

## 与 Lumen 现状融合

Lumen 的 `document_import` 覆盖 UTF-8 文本/Markdown/CSV/JSON/XML/HTML/log、DOCX、XLSX/XLS、PDF 及常见图片。图片、扫描 PDF 页和 Office 内嵌图片的 OCR 优先使用当前已配置的多模态 AI，未配置、失败或空结果时回退到随 Windows 应用打包的 PaddleOCR PP-OCRv4；Windows OCR 已删除。AI 只收到逐张待识别图片，不上传原始整份资料。识别文字按原文锚点、工作簿图片标记或 PDF 页位置回填。两引擎都失败的内容保留原件并显示警告，不会伪装成可检索文本。超过解析器边界、加密或不支持的内容会返回错误或警告。旧 DOC、PPTX、RTF、ODT 当前没有本机提取实现，不能把识别 MIME 类型误说成已支持解析。

多模态能力按当前选用的服务商和模型判断，不把“支持聊天”当作“支持图像”。DeepSeek 官方 Vision 文档明确列出 `deepseek-flash` 可通过 Chat Completions 接收图片；这也是 Lumen 当前 DeepSeek 默认模型名。没有配置可用 AI、模型拒绝图片或请求失败时仍会落到本机 PaddleOCR。[DeepSeek Vision 指南](https://api-docs.deepseek.com/guides/vision/)

流程图以 `memo_documents` 为权威来源，查询时读当前未删除的流程及步骤；不复制一份可过期的流程索引。独立资料通过 `content_assets` 保留原件，以独立知识来源记录提取文本、解析告警和可定位分块。迁移前仍走现有 WAL 一致性快照。

SQLite FTS5 trigram 对关键词与中文子串执行本机检索，以标题和章节字段加权、正文 BM25 排序；短词与标题字段另有显式回退。没有新增向量数据库、在线嵌入接口或后台常驻服务。AI 先分析问题及短对话历史，产出受限的检索词，再由本机索引找到片段；回答调用现有配置的 AI 提供商。供应商只收到问题、少量检索词和上限内的命中片段，不会在导入时收到文件，也不会自动上传整个知识库。

本版不宣称向量语义召回。AI 查询改写帮助自然语言问题命中本机全文索引，但不能替代向量相似度；最终是否作答仍由本机实际命中证据决定。无命中不请求模型生成答案。流程候选来自多个不同流程时，必须先让用户选定流程，避免拼接相近但不相干的业务步骤。

备份要包含知识来源、分块和原始 `content_assets`；FTS 虚拟索引是派生数据，不直接备份，恢复通过分块插入触发器重建。现有云同步范围是备忘与流程，独立知识库文件不随云同步；这项边界必须在界面说明，知识库记录保留在本机完整备份中。

## 设计取舍与验证要求

1. 先验证当前 bundled SQLite 能在正式迁移中创建 trigram FTS5 表，验证两字符回退、中文子串、标题/章节加权、文件名定位及删除后索引清除；若 FTS5 在目标构建不可用，迁移应明确失败，不能静默落到扫描全库并声称索引成功。
2. 分块以段落/标题为边界，较长内容再定长拆分并保留字符偏移；原始解析全文仍随来源记录保存，不以摘要或重写文本替代证据。
3. 保留文件原件及解析失败原因。明确标出 OCR/解析告警；不支持格式仍可在来源列表查看并导出原件，但不进入可搜索状态。
4. 每条模型引用只能映射到本轮本机检索产生的 ID；展示前验证文件哈希/启用状态或流程版本，冲突时返回错误，不用过期内容冒充当前内容。
5. 检索与问答组件必须分别可测：解析、数据库事务、FTS 查询、候选澄清、结构化 AI 输出、引用验证、备份恢复和页面交互都需要实际测试。外部 AI 的联网回答无法在无可用密钥时宣称已实测。
