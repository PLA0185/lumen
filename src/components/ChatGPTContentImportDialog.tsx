import { useEffect, useRef, useState } from 'react'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as memo from '../lib/memos-ipc'
import { CHATGPT_FLOW_IMPORT_PROMPT, CHATGPT_IMPORT_PROMPT, parseChatGPTContentImport, type ChatGPTImportPreview } from '../lib/chatgpt-content-import'
import { contentImages } from '../lib/content-assets'
import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { FlowImagePicker } from './FlowImagePicker'

type Target = `new:${'memo' | 'flow'}` | `existing:${string}`

export function ChatGPTContentImportDialog({
  initialDocumentId,
  kindFilter,
  onClose,
  onSaved,
}: {
  initialDocumentId?: string
  kindFilter: 'memo' | 'flow' | 'all'
  onClose: () => void
  onSaved: (document: memo.MemoDocument) => void | Promise<void>
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  const saving = useRef(false)
  const [rows, setRows] = useState<memo.MemoSummary[]>([])
  const [target, setTarget] = useState<Target>(initialDocumentId ? `existing:${initialDocumentId}` : kindFilter === 'flow' ? 'new:flow' : 'new:memo')
  const [content, setContent] = useState('')
  const [proposal, setProposal] = useState<ChatGPTImportPreview | null>(null)
  const [original, setOriginal] = useState<memo.SaveMemoInput | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')

  useEffect(() => {
    const element = dialog.current
    if (!element) return
    element.showModal()
    return () => { if (element.open) element.close() }
  }, [])
  useEffect(() => {
    let active = true
    void memo.memoList('', false).then(result => {
      if (!active) return
      setRows(result.filter(row => !row.deletedAt && (kindFilter === 'all' || row.kind === kindFilter)))
      if (initialDocumentId && !result.some(row => row.id === initialDocumentId && !row.deletedAt)) setTarget(kindFilter === 'flow' ? 'new:flow' : 'new:memo')
    }).catch(e => { if (active) setError(`读取备忘与流程列表失败：${String(e)}`) })
    return () => { active = false }
  }, [initialDocumentId, kindFilter])

  const makePreview = async () => {
    if (!content.trim() || busy) return
    setBusy(true); setError(''); setNotice('')
    try {
      let base: memo.SaveMemoInput
      let isNew: boolean
      if (target.startsWith('existing:')) {
        const document = await memo.memoGet(target.slice('existing:'.length))
        if (document.deletedAt) throw new Error('这条记录已在回收站，请先恢复后再导入')
        base = { id: document.id, expectedRevision: document.revision, title: document.title, category: document.category, kind: document.kind, bodyMd: document.bodyMd, steps: document.steps }
        isNew = false
      } else {
        const kind = target.slice('new:'.length) as 'memo' | 'flow'
        base = { id: null, expectedRevision: null, title: '', category: '', kind, bodyMd: '', steps: [] }
        isNew = true
      }
      const next = parseChatGPTContentImport(content, base, isNew)
      if (next.kind !== base.kind) throw new Error('导入内容的类型与所选目标不一致，请调整 JSON 中的 kind，或先选择对应类型的目标')
      setOriginal(base)
      setProposal(next)
    } catch (e) {
      setProposal(null); setOriginal(null); setError(String(e))
    } finally { setBusy(false) }
  }

  const confirm = async () => {
    if (!proposal || saving.current) return
    saving.current = true; setBusy(true); setError('')
    try {
      const { source: _source, ...input } = proposal
      const saved = await memo.memoSave(input)
      await onSaved(saved)
      onClose()
    } catch (e) {
      setError(`保存失败，内容仍保留在预览中：${String(e)}`)
    } finally { saving.current = false; setBusy(false) }
  }

  const close = () => { if (!busy) onClose() }
  const selectedLabel = target.startsWith('existing:')
    ? rows.find(row => row.id === target.slice('existing:'.length))?.title ?? '现有记录'
    : target === 'new:flow' ? '新建流程' : '新建备忘'
  const flowImageSource = proposal?.kind === 'flow' ? [proposal.bodyMd, ...proposal.steps.map(step => step.detail)].join('\n') : ''
  const flowImages = contentImages(flowImageSource)
  const assignedImageIds = new Set(proposal?.kind === 'flow' ? proposal.steps.flatMap(step => contentImages(step.detail).map(image => image.id)) : [])
  const unassignedImages = flowImages.filter(image => !assignedImageIds.has(image.id))
  const setStepTitle = (index: number, title: string) => setProposal(current => current ? { ...current, steps: current.steps.map((step, stepIndex) => stepIndex === index ? { ...step, title } : step) } : current)
  const setStepDetail = (index: number, detail: string) => setProposal(current => {
    if (!current || current.kind !== 'flow') return current
    const beforeIds = new Set(contentImages(current.steps[index]?.detail ?? '').map(image => image.id))
    const movedIds = new Set(contentImages(detail).map(image => image.id).filter(id => !beforeIds.has(id)))
    const removeMovedImages = (text: string) => contentImages(text).filter(image => movedIds.has(image.id)).reduce((value, image) => value.replaceAll(image.markdown, '').trim(), text)
    return {
      ...current,
      bodyMd: removeMovedImages(current.bodyMd),
      steps: current.steps.map((step, stepIndex) => stepIndex === index
        ? { ...step, detail }
        : movedIds.size ? { ...step, detail: removeMovedImages(step.detail) } : step),
    }
  })
  const addStep = () => setProposal(current => current?.kind === 'flow' ? { ...current, steps: [...current.steps, { id: crypto.randomUUID(), title: '', owner: '', detail: '' }] } : current)
  const isFlowTarget = target === 'new:flow' || (target.startsWith('existing:') && rows.find(row => row.id === target.slice('existing:'.length))?.kind === 'flow')
  const prompt = isFlowTarget ? CHATGPT_FLOW_IMPORT_PROMPT : CHATGPT_IMPORT_PROMPT

  return <dialog ref={dialog} className="chatgpt-import-dialog" aria-label="从 ChatGPT 导入内容" onCancel={event => { event.preventDefault(); close() }}>
    <div className="chatgpt-import-dialog__inner">
      <header className="chatgpt-import-dialog__header">
        <div>
          <h2>从 ChatGPT 导入内容</h2>
          <p>ChatGPT 网页版和桌面版都可以复制回答并粘贴到这里。记录先进入预览，点“确认并保存”之前不会修改备忘或流程。</p>
        </div>
        <button className="btn btn--ghost btn--sm" disabled={busy} aria-label="关闭导入" onClick={close}>关闭</button>
      </header>

      <div className="alert alert--info">
        <strong>当前使用复制粘贴导入。</strong> OpenAI 目前将完整 MCP 写入开放给 Business、Enterprise 和 Edu 计划；Plus 不能直接写入 Lumen。直连还需要 Platform 隧道 ID 和运行时 API Key。<a href="https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt" target="_blank" rel="noreferrer">查看官方说明</a>。此入口不需要 API Key，也不会把文本发送给 AI 服务。
      </div>
      <label className="chatgpt-import-dialog__target">
        保存到
        <select className="input" aria-label="ChatGPT 导入目标" value={target} disabled={busy || !!proposal} onChange={event => { setTarget(event.target.value as Target); setProposal(null); setOriginal(null) }}>
          {kindFilter !== 'flow' && <option value="new:memo">新建备忘</option>}
          {kindFilter !== 'memo' && <option value="new:flow">新建流程</option>}
          {rows.map(row => <option key={row.id} value={`existing:${row.id}`}>{row.kind === 'flow' ? '流程' : '备忘'} · {row.title}</option>)}
        </select>
      </label>

      {!proposal ? <>
        <label className="chatgpt-import-dialog__content">
          粘贴 ChatGPT 的回答
          <ContentEditor className="input selectable" aria-label="ChatGPT 导入内容" value={content} disabled={busy} maxLength={100000} rows={10} placeholder={'支持 Markdown、纯文本，或 JSON。\n已有记录会以这段正文替换原正文；JSON 可同时修改标题、分类和流程步骤。'} onChange={event => { setContent(event.target.value); setError('') }} />
        </label>
        <div className="chatgpt-import-dialog__actions">
          <button className="btn btn--ghost" disabled={busy} onClick={() => { void writeText(prompt).then(() => setNotice('导入格式提示词已复制，可粘贴到 ChatGPT 网页版或桌面版。')).catch(e => setError(`复制失败：${String(e)}`)) }}>复制 ChatGPT 提示词</button>
          <span className="setgroup__hint">可粘贴图片或拖入文件。整理流程时，请把图片放在对应步骤标题下；也可在预览里把图片分配到步骤。</span>
          <button className="btn btn--primary" disabled={busy || !content.trim()} onClick={() => void makePreview()}>{busy ? '正在生成预览…' : '生成修改预览'}</button>
        </div>
      </> : <>
        <div className="chatgpt-import-dialog__preview">
          <div className="chatgpt-import-dialog__change">
            <span className="chip chip--muted">{original?.id ? `修改：${original.title}` : '新建内容'}</span>
            <span className="chip chip--muted">{proposal.kind === 'flow' ? '业务流程' : '备忘录'}</span>
            <span className="setgroup__hint">将保存到：{selectedLabel}</span>
          </div>
          <h3>{proposal.title}</h3>
          {proposal.category && <p className="setgroup__hint">分类：{proposal.category}</p>}
          {original?.id && <p className="alert alert--warn">确认后会替换这条记录中预览所列出的标题、分类、正文和流程步骤。</p>}
          <section>
            <h4>正文预览</h4>
            <div className="chatgpt-import-dialog__markdown"><ContentMarkdown>{proposal.bodyMd || '（无正文）'}</ContentMarkdown></div>
          </section>
          {proposal.kind === 'flow' && <section>
            <h4>流程步骤 · {proposal.steps.length} 步</h4>
            {original?.kind === 'flow' && <p className="setgroup__hint">原步骤图片会按步骤标题保留；改名或移除步骤后，无法匹配的图片会放入待分配区，需重新指定步骤才能保存。</p>}
            {unassignedImages.length > 0 && <p className="alert alert--warn">还有 {unassignedImages.length} 张图片未关联到流程步骤。请在对应步骤下选择图片；完成分配后才能保存。</p>}
            {proposal.steps.length ? <ol>{proposal.steps.map((step, index) => <li key={step.id}>
              <input className="input" aria-label={`导入预览第 ${index + 1} 步标题`} maxLength={300} value={step.title} onChange={event => setStepTitle(index, event.target.value)} />
              {step.owner && <span> · {step.owner}</span>}
              {step.detail && <div className="mdpreview"><ContentMarkdown>{step.detail}</ContentMarkdown></div>}
              <FlowImagePicker source={flowImageSource} detail={step.detail} step={index + 1} onChange={detail => setStepDetail(index, detail)} />
            </li>)}</ol> : <p className="setgroup__hint">尚无步骤。可以添加步骤，再把原图分配到对应步骤。</p>}
            <button className="btn btn--ghost" disabled={proposal.steps.length >= 100} onClick={addStep}>添加流程步骤</button>
          </section>}</div>
        <div className="chatgpt-import-dialog__actions">
          <button className="btn btn--ghost" disabled={busy} onClick={() => { setProposal(null); setOriginal(null); setError('') }}>返回修改输入</button>
          <button className="btn btn--primary" disabled={busy || (proposal.kind === 'flow' && (unassignedImages.length > 0 || proposal.steps.some(step => !step.title.trim())))} onClick={() => void confirm()}>{busy ? '正在保存…' : '确认并保存'}</button>
        </div>
      </>}
      {notice && <p className="alert alert--ok" role="status">{notice}</p>}
      {error && <p className="alert alert--error" role="alert">{error}</p>}
    </div>
  </dialog>
}
