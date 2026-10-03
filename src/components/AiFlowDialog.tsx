import { useEffect, useRef, useState } from 'react'
import * as ai from '../lib/ai-ipc'
import type { SaveMemoInput } from '../lib/memos-ipc'
import { contentError } from '../lib/content-assets'
import { ContentEditor } from './ContentEditor'
import { useAiConfig } from '../lib/use-ai-config'
import { AiModelPicker } from './AiModelPicker'

export function AiFlowDialog({ onClose, onGenerated, initialText = '', restructuring = false }: { onClose: () => void; onGenerated: (draft: SaveMemoInput) => void; initialText?: string; restructuring?: boolean }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const running = useRef(false)
  const mounted = useRef(true)
  const { config, loading, configError, reload } = useAiConfig()
  const [text, setText] = useState(initialText)
  const [busy, setBusy] = useState(false)
  const [importing, setImporting] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    mounted.current = true
    dialog.current?.showModal()
    dialog.current?.querySelector<HTMLTextAreaElement>('textarea')?.focus()
    return () => { mounted.current = false }
  }, [])
  const generate = async () => {
    if (running.current || importing || !config) return
    running.current = true; setBusy(true); setError('')
    try { const draft = await ai.aiGenerateFlow(config, text); if (mounted.current) onGenerated(draft) }
    catch (e) { if (mounted.current) setError(contentError(e)) }
    finally { running.current = false; if (mounted.current) setBusy(false) }
  }
  return <dialog ref={dialog} className="ai-quick-dialog ai-flow-dialog" aria-label="AI 生成流程" onCancel={e => { if (running.current) e.preventDefault(); else onClose() }}>
    <header className="ai-flow-dialog__header">
      <h2>{restructuring ? '细分现有流程' : 'AI 生成流程'}</h2>
      <p>AI 分析目录、章节和实际操作，规范或概括标题；正文和图片保留原文，不擅自新增业务要求。</p>
    </header>
    <section className="ai-flow-dialog__material" aria-label="原始材料输入区">
      <h3>原始材料</h3>
      <p className="setgroup__hint">可以直接粘贴文字，也可以粘贴或拖入截图。按原文整理步骤及对应图片，保留已有说明。</p>
      <p className="setgroup__hint">文字和图片可以混用：按聊天顺序粘贴，把相关原图放在对应文字旁；折叠的图片先展开，模糊缩略图请换原图。</p>
      <p className="setgroup__hint">可添加 Word（DOCX）、Excel（XLSX / XLS）、PDF 和图片，添加时只保留原文件。需要正文时点击文件旁的“识别内容”。目录、简介留在流程信息；节点对应实际动作，说明、图片和图注一起保留。图片与扫描件文字需核对。</p>
      <p className="setgroup__hint">已引用 {ai.inputAssetIds(text).length} 个图片 / 文件。单次材料总大小最多 20 MiB，本机数量上限 600 个。正文最多 20000 字，含资源引用最多 80000 字；模型还可能有自己的限制。</p>
      <ContentEditor extractFiles className="input selectable" aria-label="流程原始材料" readOnly={restructuring} value={text} onChange={e => setText(e.target.value)} onBusyChange={setImporting} disabled={busy} maxLength={80000} rows={10} placeholder={'例如：\n张三：收到订单，先核对型号和数量。\n李四：确认后发给仓库，缺货时先联系客户。\n\n也可以粘贴微信聊天记录或截图。'} />
    </section>
    <footer className="ai-flow-dialog__footer">
    <p className="setgroup__hint">点击生成才会发送所选材料到当前 AI 服务，由 AI 分析操作与层级。可规范标题，不能凭空增加动作或要求；草稿确认后才会保存。</p>
    {config?.hasApiKey ? <AiModelPicker config={config} disabled={busy || loading} onSaved={reload} /> : !loading && !configError && <p className="setgroup__hint">请先在「设置 → AI」保存模型和 API 密钥。</p>}
    {configError && <p role="alert" className="alert alert--error">{configError}</p>}
    {error && <p role="alert" className="alert alert--error selectable">{error}</p>}
    <div className="ai-flow-dialog__actions">
      <button className="btn btn--primary" disabled={busy || loading || !!configError || importing || !text.trim() || !config?.hasApiKey || !config.model.trim()} onClick={() => void generate()}>{busy ? '生成中…' : restructuring ? '生成细分预览' : '生成流程草稿'}</button>
      <button className="btn btn--ghost" disabled={busy} onClick={() => void reload()}>刷新 AI 配置</button>
      <button className="btn btn--ghost" disabled={busy} onClick={onClose}>取消生成</button>
    </div>
    </footer>
  </dialog>
}
