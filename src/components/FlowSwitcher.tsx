import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { memoList, type MemoSummary } from '../lib/memos-ipc'
import { contentError } from '../lib/content-assets'
import { SafeText } from './SafeText'
export function FlowSwitcher({ id, title, disabled, onSwitch }: { id: string | null; title: string; disabled: boolean; onSwitch(id: string): Promise<boolean> }) {
  const [open, setOpen] = useState(false), [flows, setFlows] = useState<MemoSummary[]>([]), [query, setQuery] = useState(''), [error, setError] = useState(''), [loading, setLoading] = useState(false)
  const element = useRef<HTMLDivElement>(null)
  const panel = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({left:8,top:8})
  useLayoutEffect(() => {
    if (!open) return
    const measure = () => {
      const rect = element.current?.getBoundingClientRect()
      if (!rect) return
      const width = Math.min(340, window.innerWidth - 16)
      const left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8))
      const top = Math.max(8, Math.min(rect.bottom + 8, window.innerHeight - Math.min(400, window.innerHeight * .65) - 8))
      setPosition(current => current.left===left && current.top===top ? current : {left,top})
    }
    measure()
    window.addEventListener('resize',measure)
    window.addEventListener('scroll',measure,true)
    return () => {window.removeEventListener('resize',measure);window.removeEventListener('scroll',measure,true)}
  }, [open])
  useEffect(() => {
    if (!open) return
    let active = true
    setLoading(true); setError('')
    void memoList('', false).then(rows => { if (active) setFlows(rows.filter(row => row.kind === 'flow' && !row.deletedAt)) }).catch(e => { if (active) setError(contentError(e)) }).finally(() => { if (active) setLoading(false) })
    const outside = (e: PointerEvent) => { if (!element.current?.contains(e.target as Node) && !panel.current?.contains(e.target as Node)) setOpen(false) }
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setOpen(false); element.current?.querySelector('button')?.focus({ preventScroll: true }) } }
    document.addEventListener('pointerdown', outside, true); document.addEventListener('keydown', key, true)
    return () => { active = false; document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', key, true) }
  }, [open])
  return <div ref={element} className="flow-switcher">
    <button type="button" className="btn btn--primary flow-switcher__title" title="点击切换流程" aria-label={`切换流程：${title || '新流程'}`} aria-haspopup="dialog" aria-expanded={open} disabled={disabled} onClick={() => setOpen(!open)}>{title || '新流程'} <span aria-hidden="true">▾</span></button>
    {open && createPortal(<div ref={panel} role="dialog" aria-label="选择流程" className="flow-switcher__menu" style={position}><input autoFocus className="input" aria-label="查找流程名称" placeholder="查找流程名称或分类" value={query} onChange={e => setQuery(e.target.value)} />
      {loading && <p role="status">正在读取流程…</p>}{error && <p className="formerr" role="alert">{error}</p>}
      {!loading && !error && flows.filter(f => `${f.title}\n${f.category}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).map(flow => <button key={flow.id} type="button" className="flow-switcher__item" disabled={disabled || flow.id === id} aria-current={flow.id === id ? 'page' : undefined} onClick={() => void onSwitch(flow.id).then(changed => { if (changed) setOpen(false) }).catch(e => setError(contentError(e)))}><SafeText>{flow.title}</SafeText>{flow.category && <small><SafeText>{flow.category}</SafeText></small>}</button>)}
      {!loading && !error && !flows.some(f => `${f.title}\n${f.category}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) && <p>没有匹配的流程</p>}
    </div>, document.body)}
  </div>
}
