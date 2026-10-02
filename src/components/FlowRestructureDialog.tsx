import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { SaveMemoInput } from '../lib/memos-ipc'
import { AiFlowDialog } from './AiFlowDialog'
import { FlowCanvas } from './FlowCanvas'

// Use current cards, including edits and their image anchors, exactly once.
function sourceOf(draft: SaveMemoInput) {
  let groupKey = ''
  return draft.steps.map(step => {
    const key = JSON.stringify(step.group ?? null)
    const headers = step.group && key !== groupKey ? [step.group.title, ...step.group.path].map((title, i) => `${'#'.repeat(i + 2)} ${title}\n\n`).join('') : ''
    groupKey = key
    return `${headers}${'#'.repeat(step.group ? 3 + step.group.path.length : 2)} ${step.title}\n\n${step.owner ? `负责人：${step.owner}\n\n` : ''}${step.detail}`
  }).join('\n\n')
}
export function FlowRestructureDialog({ original, onClose, onApply }: { original: SaveMemoInput; onClose: () => void; onApply: (steps: SaveMemoInput['steps']) => Promise<void> }) {
  const [preview, setPreview] = useState<SaveMemoInput | null>(null)
  const [busy, setBusy] = useState(false)
  const running = useRef(false)
  const [error, setError] = useState('')
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => { if (preview) dialog.current?.showModal() }, [preview])
  const apply = async () => {
    if (!preview || running.current) return
    running.current = true; setBusy(true); setError('')
    try { await onApply(preview.steps) }
    catch (e) { setError(e instanceof Error ? e.message : String(e)) }
    finally { running.current = false; setBusy(false) }
  }
  if (!preview) return <AiFlowDialog initialText={sourceOf(original)} restructuring onClose={onClose} onGenerated={setPreview} />
  return createPortal(<dialog ref={dialog} className="flow-restructure-dialog" aria-label="流程细分预览" onCancel={e => { if (running.current) e.preventDefault(); else onClose() }}>
    <header><h2>流程细分预览</h2><p>原文和图片按原顺序分组。确认后保存为当前流程的新版本。</p></header>
    <FlowCanvas steps={preview.steps} source={preview.bodyMd} readOnly onChange={() => {}} />
    {error && <p role="alert" className="alert alert--error">{error}</p>}
    <footer><button className="btn btn--primary" disabled={busy} onClick={() => void apply()}>{busy ? '保存中…' : '确认保存细分'}</button><button className="btn btn--ghost" disabled={busy} onClick={onClose}>取消细分</button></footer>
  </dialog>, document.body)
}
