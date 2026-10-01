import { useEffect, useRef, useState } from 'react'
import * as ai from '../lib/ai-ipc'
import type { SaveMemoInput } from '../lib/memos-ipc'
import { contentError } from '../lib/content-assets'
import { ContentEditor } from './ContentEditor'

export function AiFlowDialog({ onClose, onGenerated }: { onClose: () => void; onGenerated: (draft: SaveMemoInput) => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const running = useRef(false)
  const mounted = useRef(true)
  const [config, setConfig] = useState<ai.ProviderConfig | null>(null)
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [importing, setImporting] = useState(false)
  const [error, setError] = useState('')
  const reload = async () => {
    try { const value = await ai.aiGetConfig(); if (mounted.current) { setConfig(value); setError('') } }
    catch (e) { if (mounted.current) setError(contentError(e)) }
  }
  useEffect(() => {
    mounted.current = true
    dialog.current?.showModal()
    dialog.current?.querySelector<HTMLTextAreaElement>('textarea')?.focus()
    void reload()
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
      <h2>AI 生成流程</h2>
      <p>把业务说明、微信聊天记录或截图放在下面，AI 会整理成可修改的流程草稿。</p>
    </header>
    <section className="ai-flow-dialog__material" aria-label="原始材料输入区">
      <h3>原始材料</h3>
      <p className="setgroup__hint">可以直接粘贴文字，也可以粘贴或拖入截图。不明确的内容会标为“待确认”。</p>
      <p className="setgroup__hint">文字和图片可以混用：按聊天顺序粘贴，把相关原图放在对应文字旁；折叠的图片先展开，模糊缩略图请换原图。</p>
      <ContentEditor className="input selectable" aria-label="流程原始材料" value={text} onChange={e => setText(e.target.value)} onBusyChange={setImporting} disabled={busy} maxLength={20000} rows={10} placeholder={'例如：\n张三：收到订单，先核对型号和数量。\n李四：确认后发给仓库，缺货时先联系客户。\n\n也可以粘贴微信聊天记录或截图。'} />
    </section>
    <footer className="ai-flow-dialog__footer">
    <p className="setgroup__hint">点击生成才会发送所选材料到当前 AI 服务。生成的草稿需确认后才会保存。</p>
    {config?.hasApiKey && config.model.trim() ? <p className="setgroup__hint">当前模型：{config.model}。截图识别取决于模型的图片能力，看不清的文字需要人工核对。</p> : <p className="setgroup__hint">请先在「设置 → AI」保存模型和 API 密钥，再返回此处刷新配置。</p>}
    {error && <p role="alert" className="alert alert--error selectable">{error}</p>}
    <div className="ai-flow-dialog__actions">
      <button className="btn btn--primary" disabled={busy || importing || !text.trim() || !config?.hasApiKey || !config.model.trim()} onClick={() => void generate()}>{busy ? '生成中…' : '生成流程草稿'}</button>
      <button className="btn btn--ghost" disabled={busy} onClick={() => void reload()}>刷新 AI 配置</button>
      <button className="btn btn--ghost" disabled={busy} onClick={onClose}>取消生成</button>
    </div>
    </footer>
  </dialog>
}
