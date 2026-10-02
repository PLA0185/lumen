import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { FlowStep } from '../lib/memos-ipc'

export function FlowStepMenu({ steps, id, anchor, disabled, onClose, onMove, onSwap, onAdd, onDelete }: {
  steps: FlowStep[]; id: string; anchor: HTMLButtonElement; disabled: boolean
  onClose(): void; onMove(index: number): void; onSwap(id: string): void; onAdd(index: number): void; onDelete(): void
}) {
  const index = steps.findIndex(s => s.id === id)
  const [destination, setDestination] = useState(index)
  const [swapId, setSwapId] = useState(steps.find(s => s.id !== id)?.id ?? '')
  const [deleting, setDeleting] = useState(false)
  const panel = useRef<HTMLDivElement>(null)
  const rect = anchor.getBoundingClientRect()
  useEffect(() => {
    const previous = document.activeElement
    panel.current?.focus()
    const outside = (e: PointerEvent) => { if (!panel.current?.contains(e.target as Node) && !anchor.contains(e.target as Node)) onClose() }
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose() }
      if (e.key === 'Tab') {
        const fields = panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), select:not(:disabled)')
        if (!fields?.length) return
        if (e.shiftKey && (document.activeElement === fields[0] || document.activeElement === panel.current)) { e.preventDefault(); fields[fields.length - 1]!.focus() }
        else if (!e.shiftKey && document.activeElement === fields[fields.length - 1]) { e.preventDefault(); fields[0]!.focus() }
      }
    }
    document.addEventListener('pointerdown', outside, true)
    document.addEventListener('keydown', key, true)
    return () => {
      document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', key, true)
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus()
    }
  }, [anchor, onClose])
  if (index < 0) return null
  return createPortal(<div ref={panel} tabIndex={-1} role="dialog" aria-label={`第 ${index + 1} 步操作菜单`} className="flow-step-menu" style={{ left: Math.max(8, Math.min(rect.left, window.innerWidth - 328)), top: Math.max(8, Math.min(rect.bottom + 8, window.innerHeight - 450)) }}>
    <div className="flow-canvas__inspector-head"><strong>第 {index + 1} 步</strong><button type="button" className="btn btn--quiet btn--sm" aria-label="关闭步骤菜单" onClick={onClose}>关闭</button></div>
    <fieldset disabled={disabled}>
      <div className="memos__step-actions"><button className="btn btn--ghost btn--sm" disabled={index === 0} onClick={() => onMove(index - 1)}>上移</button><button className="btn btn--ghost btn--sm" disabled={index === steps.length - 1} onClick={() => onMove(index + 1)}>下移</button></div>
      <label>移动到<select className="input" aria-label="移动目标位置" value={destination} onChange={e => setDestination(Number(e.target.value))}>{steps.map((s, i) => <option key={s.id} value={i}>第 {i + 1} 步 · {s.title || '未命名步骤'}</option>)}</select></label>
      <button className="btn btn--ghost btn--sm" disabled={destination === index || destination >= steps.length} onClick={() => onMove(destination)}>移动到此位置</button>
      <label>与另一节点交换<select className="input" aria-label="交换目标步骤" value={swapId} onChange={e => setSwapId(e.target.value)}>{steps.filter(s => s.id !== id).map(s => <option key={s.id} value={s.id}>第 {steps.indexOf(s) + 1} 步 · {s.title || '未命名步骤'}</option>)}</select></label>
      <button className="btn btn--ghost btn--sm" disabled={!swapId || !steps.some(s => s.id === swapId && s.id !== id)} onClick={() => onSwap(swapId)}>交换位置</button>
      <div className="memos__step-actions"><button className="btn btn--ghost btn--sm" disabled={steps.length >= 100} onClick={() => onAdd(index)}>在前面添加</button><button className="btn btn--ghost btn--sm" disabled={steps.length >= 100} onClick={() => onAdd(index + 1)}>在后面添加</button></div>
      {deleting ? <div className="flow-step-menu__delete"><p>删除此步骤？原图片文件会保留。</p><button className="btn btn--ghost btn--sm" onClick={onDelete}>确认删除此步骤</button><button className="btn btn--quiet btn--sm" onClick={() => setDeleting(false)}>取消删除</button></div> : <button className="btn btn--ghost btn--sm" onClick={() => setDeleting(true)}>删除步骤</button>}
    </fieldset>
  </div>, document.body)
}
