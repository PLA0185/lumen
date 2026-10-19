import { useEffect, useRef, useState } from 'react'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as memo from '../lib/memos-ipc'
import { CHATGPT_IMPORT_PROMPT, parseChatGPTContentImport, type ChatGPTImportPreview } from '../lib/chatgpt-content-import'
import { contentImages } from '../lib/content-assets'
import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { FlowImagePicker } from './FlowImagePicker'

type Target = 'new:auto' | `existing:${string}`

export function ChatGPTContentImportDialog({
  initialDocumentId,
  kindFilter,
  onClose,
  onSaved,
}: {
  initialDocumentId?: string
  kindFilter: 'memo' | 'flow' | 'all'
  onClose: () => void
  onSaved: (document: memo.MemoDocument | memo.MemoDocument[]) => void | Promise<void>
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  const saving = useRef(false)
  const [rows, setRows] = useState<memo.MemoSummary[]>([])
  const [target, setTarget] = useState<Target>(initialDocumentId ? `existing:${initialDocumentId}` : 'new:auto')
  const [content, setContent] = useState('')
  const [proposal, setProposal] = useState<ChatGPTImportPreview | null>(null)
  const [original, setOriginal] = useState<memo.SaveMemoInput | null>(null)
  const [archivePath, setArchivePath] = useState('')
  const [archivePreview, setArchivePreview] = useState<memo.MemoArchivePreview | null>(null)
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
      setRows(result.filter(row => !row.deletedAt))
      if (initialDocumentId && !result.some(row => row.id === initialDocumentId && !row.deletedAt)) setTarget('new:auto')
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
        // Markdown without a JSON `kind` falls back to the section the user opened.
        // The universal prompt always supplies `kind`, so new JSON can choose either type.
        const kind = kindFilter === 'flow' ? 'flow' : 'memo'
        base = { id: null, expectedRevision: null, title: '', category: '', kind, bodyMd: '', steps: [] }
        isNew = true
      }
      const next = parseChatGPTContentImport(content, base, isNew)
      if (!isNew && next.kind !== base.kind) throw new Error('更新已有记录时不能更改记录类型，请保持 JSON 中的 kind 与原记录一致')
      setOriginal(base)
      setProposal(next)
    } catch (e) {
      setProposal(null); setOriginal(null); setError(String(e))
    } finally { setBusy(false) }
  }

  const previewArchive = async (path: string) => {
    if (busy) return
    setBusy(true); setError(''); setNotice(''); setProposal(null); setOriginal(null)
    try {
      const result = await memo.memoArchivePreview(path)
      setArchivePath(path); setArchivePreview(result); setTarget('new:auto')
    } catch (e) {
      setArchivePath(''); setArchivePreview(null); setError(`压缩包识别失败：${String(e)}`)
    } finally { setBusy(false) }
  }

  const chooseArchive = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const path = await open({ title: '选择 GPT 导出的压缩包', multiple: false, filters: [{ name: 'ZIP 压缩包', extensions: ['zip'] }] })
      if (typeof path === 'string') await previewArchive(path)
    } catch (e) { setError(`打开压缩包失败：${String(e)}`) }
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

  const confirmArchive = async () => {
    if (!archivePath || !archivePreview || saving.current) return
    saving.current = true; setBusy(true); setError('')
    try {
      const saved = await memo.memoArchiveImport(archivePath, archivePreview.sha256)
      setArchivePath('')
      try {
        await onSaved(saved)
        onClose()
      } catch (e) {
        setError(`已导入 ${saved.length} 条记录，但刷新界面失败。请关闭窗口后重新打开备忘与流程：${String(e)}`)
      }
    } catch (e) {
      setError(`批量导入失败，原有内容未更改：${String(e)}`)
    } finally { saving.current = false; setBusy(false) }
  }

  const close = () => { if (!busy) onClose() }
  const selectedLabel = target.startsWith('existing:')
    ? rows.find(row => row.id === target.slice('existing:'.length))?.title ?? '现有记录'
    : 'GPT 判断类型后新建'
  const selectedDocument = target.startsWith('existing:') ? rows.find(row => row.id === target.slice('existing:'.length)) : undefined
  const isExistingTarget = target.startsWith('existing:')
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
  const prompt = isExistingTarget && selectedDocument
    ? `${CHATGPT_IMPORT_PROMPT}\n\n本次是更新现有${selectedDocument.kind === 'flow' ? '流程' : '备忘'}，请保持 kind 为 "${selectedDocument.kind}"，只整理这条记录的内容。`
    : CHATGPT_IMPORT_PROMPT

  return <dialog ref={dialog} className="chatgpt-import-dialog" aria-label="从 ChatGPT 导入内容" onCancel={event => { event.preventDefault(); close() }}>
    <div className="chatgpt-import-dialog__inner">
      <header className="chatgpt-import-dialog__header">
        <div>
          <h2>从 ChatGPT 导入内容</h2>
          <p>ChatGPT 网页版和桌面版都可以复制回答并粘贴到这里。记录先进入预览，点“确认并保存”之前不会修改备忘或流程。</p>
        </div>
        <button className="btn btn--ghost btn--sm" disabled={busy} aria-label="关闭导入" onClick={close}>关闭</button>
      </header>

      <div className="alert alert--info chatgpt-import-dialog__access-note">
        <p><strong>当前通过复制粘贴导入。</strong> ChatGPT 的回答会先进入预览，只有你确认后才保存。此入口不需要 API Key，也不会把文本发送给 AI 服务。</p>
        <p>ChatGPT 直连写入还需要账号计划支持以及 Platform 隧道配置。<a className="chatgpt-import-dialog__official-link" href="https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt" target="_blank" rel="noreferrer">查看 ChatGPT 官方说明</a></p>
      </div>
      <fieldset className="chatgpt-import-dialog__target" disabled={busy || !!proposal || !!archivePreview}>
        <legend>导入目标</legend>
        <div className="chatgpt-import-dialog__target-modes" role="group" aria-label="导入目标类型">
          <button type="button" className={`btn ${!isExistingTarget ? 'btn--primary' : 'btn--ghost'}`} aria-pressed={!isExistingTarget} onClick={() => setTarget('new:auto')}>新建内容（GPT 判断类型）</button>
          <button type="button" className={`btn ${isExistingTarget ? 'btn--primary' : 'btn--ghost'}`} aria-pressed={isExistingTarget} disabled={!rows.length} onClick={() => {
            const current = selectedDocument
            const next = current ?? rows[0]
            if (next) setTarget(`existing:${next.id}`)
          }}>更新已有记录</button>
        </div>
        {isExistingTarget && <label className="chatgpt-import-dialog__target-select">
          选择备忘或流程
          <select className="input" aria-label="选择要更新的备忘或流程" value={target} onChange={event => setTarget(event.target.value as Target)}>
            {rows.map(row => <option key={row.id} value={`existing:${row.id}`}>{row.kind === 'flow' ? '流程' : '备忘'} · {row.title}</option>)}
          </select>
        </label>}
        <p className="chatgpt-import-dialog__target-hint">新建时由 GPT 根据内容判断保存为备忘还是流程；更新已有记录时保留原类型，并先显示修改预览。</p>
      </fieldset>

      {archivePreview ? <>
        <div className="chatgpt-import-dialog__preview" aria-label="ZIP 批量导入预览">
          <div className="chatgpt-import-dialog__change">
            <span className="chip chip--muted">ZIP 批量导入</span>
            <span className="chip chip--muted">{archivePreview.records.length} 条备忘与流程</span>
            <span className="chip chip--muted">{archivePreview.imageCount} 张关联图片</span>
          </div>
          <p className="setgroup__hint">确认后会一次性新建全部记录；流程步骤和图片引用会按压缩包中的内容保留。关闭或返回不会写入任何内容。</p>
          {archivePreview.records.map((record, index) => <article className="chatgpt-import-dialog__archive-record" key={`${record.kind}-${record.title}-${index}`}>
            <h3><span className="chip chip--muted">{record.kind === 'flow' ? '流程' : '备忘'}</span> {record.title}</h3>
            {record.category && <p className="setgroup__hint">分类：{record.category}</p>}
            <p className="setgroup__hint">{record.kind === 'flow' ? `${record.steps.length} 个步骤` : '独立备忘'} · {record.imageCount} 张图片</p>
            {record.bodyMd && <pre>{record.bodyMd}</pre>}
            {record.steps.length > 0 && <ol>{record.steps.map((step, stepIndex) => <li key={`${step.id}-${stepIndex}`}><strong>{step.title}</strong>{step.detail && <pre>{step.detail}</pre>}</li>)}</ol>}
          </article>)}
        </div>
        <div className="chatgpt-import-dialog__actions">
          <button className="btn btn--ghost" disabled={busy} onClick={() => { setArchivePreview(null); setArchivePath(''); setError('') }}>返回粘贴导入</button>
          <button className="btn btn--ghost" disabled={busy} onClick={() => void chooseArchive()}>换一个压缩包</button>
          <button className="btn btn--primary" disabled={busy || !archivePath} onClick={() => void confirmArchive()}>{busy ? '正在批量导入…' : `确认导入 ${archivePreview.records.length} 条`}</button>
        </div>
      </> : !proposal ? <>
        <section className="chatgpt-import-dialog__format" aria-label="ChatGPT 返回格式说明">
          <div className="chatgpt-import-dialog__format-heading">
            <strong>通用返回格式</strong>
            <span>先复制下方指令给 GPT，再把材料一起发给它</span>
          </div>
          <p>只需复制这一份提示词。GPT 会按内容判断 <code>kind</code>：按顺序执行的操作用 <code>flow</code>，资料、规则和独立说明用 <code>memo</code>。备忘的 <code>steps</code> 留空；流程按步骤填写。</p>
          <p>多条记录或带多张截图的 GPT 压缩包也可批量导入：ZIP 内放 JSON 数组或多份 JSON，并把图片放在 <code>images/</code>，正文和步骤用 Markdown 相对路径引用。</p>
          <pre><code>{'备忘：\n{\n  "kind": "memo",\n  "title": "备忘标题",\n  "category": "分类",\n  "bodyMd": "Markdown 正文",\n  "steps": []\n}\n\n流程：\n{\n  "kind": "flow",\n  "title": "流程标题",\n  "category": "分类",\n  "bodyMd": "背景说明",\n  "steps": [{ "title": "步骤", "owner": "负责人", "detail": "操作说明" }]\n}'}</code></pre>
        </section>
        <label className="chatgpt-import-dialog__content">
          粘贴 ChatGPT 的回答
          <ContentEditor className="input selectable" aria-label="ChatGPT 导入内容" value={content} disabled={busy} maxLength={100000} rows={10} placeholder={isExistingTarget ? '粘贴与所选记录类型一致的 GPT JSON，也可粘贴 Markdown / 纯文本。' : '粘贴 GPT 返回的单条 JSON；kind 会自动决定保存为备忘或流程。'} onArchiveDrop={path => { void previewArchive(path) }} onChange={event => { setContent(event.target.value); setError('') }} />
        </label>
        <div className="chatgpt-import-dialog__actions">
          <button className="btn btn--ghost" disabled={busy} onClick={() => { void writeText(prompt).then(() => setNotice('通用提示词已复制；把指令和材料一起发给 ChatGPT。')).catch(e => setError(`复制失败：${String(e)}`)) }}>复制通用提示词</button>
          <button type="button" className="btn btn--ghost" disabled={busy} title="ZIP 会批量新建记录，不会覆盖已有备忘或流程" onClick={() => void chooseArchive()}>导入 GPT 压缩包</button>
          <span className="setgroup__hint">可粘贴文本、拖入文件，或把 GPT 的 ZIP 拖到此窗口。ZIP 先预览，确认后批量导入并保留图片关联。</span>
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
