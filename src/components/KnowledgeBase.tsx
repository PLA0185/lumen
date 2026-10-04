import { useCallback, useEffect, useRef, useState } from 'react'
import { Icon } from './Icons'
import { assetExport, contentError } from '../lib/content-assets'
import {
  knowledgeAsk,
  knowledgeDelete,
  knowledgeImportFile,
  knowledgeList,
  knowledgeSearch,
  type KnowledgeAskResult,
  type KnowledgeCitation,
  type KnowledgeHistoryEntry,
  type KnowledgeSourceSummary,
} from '../lib/knowledge-base-ipc'
import { onDataChanged } from '../lib/data-change'

function formatBytes(size: number) {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(0)} KB`
  return `${(size / 1024 / 1024).toFixed(1)} MB`
}

function Evidence({ citation, onOpenFlow }: {
  citation: KnowledgeCitation
  onOpenFlow: (flowId: string, stepId: string) => void
}) {
  const [exporting, setExporting] = useState(false)
  const [exportError, setExportError] = useState('')
  const exportOriginal = async () => {
    if (!citation.assetId) return
    setExporting(true); setExportError('')
    try {
      const { save } = await import('@tauri-apps/plugin-dialog')
      const path = await save({ title: '导出引用原件', defaultPath: citation.title })
      if (typeof path === 'string') await assetExport(citation.assetId, path)
    } catch (error) { setExportError(contentError(error)) }
    finally { setExporting(false) }
  }
  return <article className="knowledge-evidence">
    <div className="knowledge-evidence__meta">
      <strong>{citation.title}</strong>
      {citation.category && <span>{citation.category}</span>}
      <span>{citation.locator}</span>
    </div>
    <p>{citation.excerpt}</p>
    {citation.sourceKind === 'document' && citation.assetId && <button type="button" className="btn btn--ghost btn--sm" disabled={exporting} onClick={() => void exportOriginal()}>{exporting ? '导出中…' : '导出引用原件'}</button>}
    {exportError && <p className="knowledge-sources__warning" role="alert">原件导出失败：{exportError}</p>}
    {citation.sourceKind === 'flow' && citation.flowId && citation.stepId &&
      <button type="button" className="btn btn--ghost btn--sm" onClick={() => onOpenFlow(citation.flowId!, citation.stepId!)}>
        打开并定位到流程节点
      </button>}
  </article>
}

export function KnowledgeBase({ onOpenFlow }: { onOpenFlow: (flowId: string, stepId: string) => void }) {
  const [sources, setSources] = useState<KnowledgeSourceSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [listError, setListError] = useState('')
  const [importing, setImporting] = useState(false)
  const [importMessage, setImportMessage] = useState('')
  const [busyDelete, setBusyDelete] = useState('')
  const [exportingId, setExportingId] = useState('')
  const [searchText, setSearchText] = useState('')
  const [searchResults, setSearchResults] = useState<KnowledgeCitation[]>([])
  const [searching, setSearching] = useState(false)
  const [searchError, setSearchError] = useState('')
  const [searchRevision, setSearchRevision] = useState(0)
  const [question, setQuestion] = useState('')
  const [answer, setAnswer] = useState<KnowledgeAskResult | null>(null)
  const [history, setHistory] = useState<KnowledgeHistoryEntry[]>([])
  const [asking, setAsking] = useState(false)
  const [askError, setAskError] = useState('')
  const fileInput = useRef<HTMLInputElement>(null)
  const searchRequest = useRef(0)
  const askRequest = useRef(0)
  const listRequest = useRef(0)

  const reload = useCallback(async () => {
    const request = ++listRequest.current
    setLoading(true)
    try {
      const rows = await knowledgeList()
      if (request === listRequest.current) { setSources(rows); setListError('') }
    } catch (error) {
      if (request === listRequest.current) setListError(contentError(error))
    } finally { if (request === listRequest.current) setLoading(false) }
  }, [])
  useEffect(() => { void reload() }, [reload])
  useEffect(() => onDataChanged(['knowledge', 'memos'], () => {
    void reload()
    askRequest.current += 1
    setAnswer(null)
    setAskError('')
    setSearchResults([])
    setAsking(false)
    setSearchRevision(value => value + 1)
  }), [reload])

  useEffect(() => {
    const q = searchText.trim()
    const request = ++searchRequest.current
    if (!q) { setSearchResults([]); setSearchError(''); setSearching(false); return }
    setSearchResults([])
    setSearchError('')
    setSearching(true)
    const timer = window.setTimeout(async () => {
      setSearching(true); setSearchError('')
      try {
        const result = await knowledgeSearch(q)
        if (request === searchRequest.current) setSearchResults(result.citations)
      } catch (error) {
        if (request === searchRequest.current) setSearchError(contentError(error))
      } finally { if (request === searchRequest.current) setSearching(false) }
    }, 250)
    return () => window.clearTimeout(timer)
  }, [searchText, searchRevision])

  const importSelected = async (files: FileList | null) => {
    if (!files?.length) return
    const picked = Array.from(files)
    setImporting(true); setImportMessage('')
    const messages: string[] = []
    for (const file of picked) {
      try {
        const result = await knowledgeImportFile(file)
        messages.push(result.duplicate
          ? `「${file.name}」内容已存在，沿用已有索引。`
          : result.source.status === 'ready'
            ? `已解析「${file.name}」并加入检索。${result.source.warnings.length ? ` 提示：${result.source.warnings.join('；')}` : ''}`
            : `已保留「${file.name}」原件，但未能解析：${result.source.error ?? '解析器未返回原因'}`)
      } catch (error) { messages.push(`「${file.name}」导入失败：${contentError(error)}`) }
    }
    setImportMessage(messages.join('\n'))
    await reload()
    setImporting(false)
    if (fileInput.current) fileInput.current.value = ''
  }

  const removeSource = async (source: KnowledgeSourceSummary) => {
    if (!window.confirm(`从知识库移除「${source.title}」？将删除解析文本和检索索引；如果原件仍被备忘或流程引用，会保留原件，否则一并删除。`)) return
    setBusyDelete(source.id)
    try {
      const retained = await knowledgeDelete(source.id)
      setImportMessage(retained
        ? `已移除「${source.title}」的知识库索引；原文件仍被备忘或流程引用，因此保留在本机。`
        : `已移除「${source.title}」及其知识库原件。`)
      await reload()
    } catch (error) { setImportMessage(`移除失败：${contentError(error)}`) }
    finally { setBusyDelete('') }
  }

  const exportSource = async (source: KnowledgeSourceSummary) => {
    setExportingId(source.id)
    try {
      const { save } = await import('@tauri-apps/plugin-dialog')
      const path = await save({
        title: '导出知识库原件',
        defaultPath: source.title,
      })
      if (typeof path !== 'string') return
      await assetExport(source.assetId, path)
      setImportMessage(`已导出原件「${source.title}」。`)
    } catch (error) { setImportMessage(`原件导出失败：${contentError(error)}`) }
    finally { setExportingId('') }
  }

  const ask = async (flowId: string | null = null) => {
    const q = question.trim()
    if (!q || asking) return
    const request = ++askRequest.current
    setAsking(true); setAskError(''); setAnswer(null)
    try {
      const recentHistory = history.slice(-6)
      const contextHistory = flowId && recentHistory.at(-1)?.role === 'user' && recentHistory.at(-1)?.text === q
        ? recentHistory.slice(0, -1)
        : recentHistory
      const result = await knowledgeAsk({ question: q, history: contextHistory, selectedFlowId: flowId })
      if (request !== askRequest.current) return
      setAnswer(result)
      setHistory(current => {
        const turns = current.at(-1)?.role === 'user' && current.at(-1)?.text === q
          ? current
          : [...current, { role: 'user' as const, text: q }]
        return [...turns, ...(result.answer ? [{ role: 'assistant' as const, text: result.answer }] : [])].slice(-12)
      })
      if (result.status === 'answered') setQuestion('')
    } catch (error) { if (request === askRequest.current) setAskError(contentError(error)) }
    finally { if (request === askRequest.current) setAsking(false) }
  }

  return <section className="knowledge-base" aria-label="知识库">
    <div className="knowledge-base__intro">
      <div><h2>资料与流程问答</h2><p>答案只根据已导入资料和已保存流程生成，并附上可打开核对的来源。</p></div>
      <button type="button" className="btn btn--primary" disabled={importing} onClick={() => fileInput.current?.click()}><Icon name="plus" size={15} /> 添加资料</button>
      <input ref={fileInput} type="file" multiple disabled={importing} onChange={event => void importSelected(event.currentTarget.files)} aria-label="选择知识库资料" />
    </div>
    <p className="knowledge-base__privacy">知识库资料保存在本机，不参与云同步。提问时，问题和少量相关摘录会发送给设置中的 AI 服务商；导入时不会上传整份文件。</p>
    <div className="knowledge-base__grid">
      <section className="knowledge-panel" aria-label="资料库">
        <header><h3>已加入的资料</h3><span>{sources.length} 项</span></header>
        {loading && <p role="status">正在读取资料列表…</p>}
        {listError && <div className="alert alert--error" role="alert">{listError}<button type="button" className="btn btn--ghost btn--sm" onClick={() => void reload()}>重试</button></div>}
        {!loading && !listError && sources.length === 0 && <p className="knowledge-empty">还没有资料。可添加 PDF、DOCX、XLSX/XLS、PNG/JPEG/WEBP/GIF、Markdown、CSV、JSON、HTML、XML、log 或文本文件，也可直接检索已保存流程。</p>}
        <ul className="knowledge-sources">
          {sources.map(source => <li key={source.id}>
            <div className="knowledge-sources__title"><strong title={source.title}>{source.title}</strong><span>{formatBytes(source.byteSize)}</span></div>
            <div className="knowledge-sources__state">
              <span className={source.status === 'ready' ? 'knowledge-status' : 'knowledge-status knowledge-status--warning'}>{source.status === 'ready' ? '可检索' : '未解析'}</span>
              <span>{source.mime || '未知类型'}</span>
              <button type="button" className="btn btn--ghost btn--sm" disabled={!!exportingId} onClick={() => void exportSource(source)}>{exportingId === source.id ? '导出中…' : '导出原件'}</button>
              <button type="button" className="btn btn--ghost btn--sm" aria-label={`移除 ${source.title}`} disabled={busyDelete === source.id} onClick={() => void removeSource(source)}>移除</button>
            </div>
            {(source.error || source.warnings.length > 0) && <p className="knowledge-sources__warning">{source.error ?? source.warnings.join('；')}</p>}
          </li>)}
        </ul>
        {importing && <p role="status">正在解析并建立本机检索索引…</p>}
        {importMessage && <pre className="knowledge-message" role="status">{importMessage}</pre>}
        <details className="knowledge-base__scope"><summary>解析范围与限制</summary><p>使用应用现有解析器保留原件并抽取文本。PDF 扫描件和图片依赖本机 OCR；暂不支持的格式会保留原件并标明失败，不会假装已入索引。来源会显示解析片段和页码/章节位置，可导出原件核对；知识库为本机数据，目前不随云同步。</p></details>
      </section>

      <section className="knowledge-panel knowledge-panel--ask" aria-label="知识库问答">
        <header><h3>问下一步怎么做</h3><span>依据来源回答</span></header>
        <form onSubmit={event => { event.preventDefault(); void ask() }}>
          <textarea value={question} maxLength={2000} onChange={event => setQuestion(event.target.value)} placeholder="例如：领导让我安排美国发票，接下来按哪个流程、做哪一步？" aria-label="提问" />
          <button className="btn btn--primary" type="submit" disabled={asking || !question.trim()}>{asking ? '正在检索并核对…' : '检索并回答'}</button>
        </form>
        <div className="knowledge-direct-search">
          <label htmlFor="knowledge-search">只检索来源</label>
          <input id="knowledge-search" type="search" value={searchText} onChange={event => setSearchText(event.target.value)} placeholder="关键词、单据名或流程步骤" />
          {searching && <span role="status">检索中…</span>}
          {searchError && <p className="alert alert--error" role="alert">{searchError}</p>}
          {searchText.trim() && !searching && !searchError && searchResults.length === 0 && <p className="knowledge-empty">没有找到匹配的来源。</p>}
          {searchResults.map(citation => <Evidence key={citation.id} citation={citation} onOpenFlow={onOpenFlow} />)}
        </div>
        {askError && <p className="alert alert--error" role="alert">{askError}</p>}
        {answer && <div className="knowledge-answer" aria-live="polite">
          <p className="knowledge-answer__notice">{answer.providerNotice}</p>
          {answer.message && <p role="status">{answer.message}</p>}
          {answer.status === 'clarify' && <div className="knowledge-candidates"><strong>请选择要查询的流程</strong>{answer.flowCandidates.map(candidate => <button type="button" className="knowledge-candidate" key={candidate.flowId} disabled={asking} onClick={() => void ask(candidate.flowId)}><strong>{candidate.title}</strong><span>{candidate.category} · {candidate.evidence}</span></button>)}</div>}
          {answer.answer && <div className="knowledge-answer__text">{answer.answer}</div>}
          {answer.citations.length > 0 && <div className="knowledge-answer__evidence"><h4>回答依据</h4>{answer.citations.map(citation => <Evidence key={citation.id} citation={citation} onOpenFlow={onOpenFlow} />)}</div>}
          {answer.searchTerms.length > 0 && <details><summary>本次检索词</summary><p>{answer.searchTerms.join('、')}</p></details>}
        </div>}
      </section>
    </div>
  </section>
}
