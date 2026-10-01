import { useEffect, useLayoutEffect, useId, useRef, useState, type ReactNode } from 'react'
import type { FlowStep } from '../lib/memos-ipc'
import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { FlowImagePicker } from './FlowImagePicker'
import { setCanvasInputActive } from '../lib/canvas-input'

const WIDTH = 252, HEIGHT = 148, GAP = 64, ROW = 212
function position(index: number) {
  const row = Math.floor(index / 3), column = index % 3
  return { x: (row % 2 ? 2 - column : column) * (WIDTH + GAP), y: row * ROW }
}
function plain(text: string) { return text.replace(/!?\[([^\]]*)\]\(lumen-asset:[^)]+\)/g, '$1').replace(/[#*_`]/g, '') }
function match(step: FlowStep, query: string, exact: boolean) {
  const text = `${step.title}\n${step.owner}\n${plain(step.detail)}`.toLocaleLowerCase()
  const needle = query.trim().toLocaleLowerCase()
  if (exact) return text.includes(needle)
  return needle.split(/\s+/).every(token => {
    // Ordered characters permit missing words while preserving the query's meaning.
    let from = 0
    for (const char of token) { const found = text.indexOf(char, from); if (found < 0) return false; from = found + 1 }
    return true
  })
}
function isInput(target: EventTarget | null) {
  return target instanceof Element && !!target.closest('input, textarea, select, [contenteditable="true"], .flow-canvas__inspector')
}

export function FlowCanvas({ steps, source, onChange, disabled = false, readOnly = false, initialEdit = false, metadata }: {
  steps: FlowStep[]; source: string; onChange: (steps: FlowStep[]) => void; disabled?: boolean; readOnly?: boolean; initialEdit?: boolean; metadata?: ReactNode
}) {
  const [selectedId, setSelectedId] = useState<string | null>(() => initialEdit ? steps[0]?.id ?? null : null)
  const [query, setQuery] = useState('')
  const [infoOpen, setInfoOpen] = useState(initialEdit)
  const [mode, setMode] = useState('fuzzy')
  const [view, setView] = useState({ x: 32, y: 32, scale: 1 })
  const [space, setSpace] = useState(false)
  const [inputError, setInputError] = useState('')
  const viewport = useRef<HTMLDivElement>(null)
  const pointerInside = useRef(false)
  const spaceHeld = useRef(false)
  const nativeActive = useRef(false)
  const mounted = useRef(true)
  const drag = useRef<{ id: number; x: number; y: number; originX: number; originY: number } | null>(null)
  const marker = useId().replace(/:/g, '')
  const selected = steps.findIndex(step => step.id === selectedId)
  const step = steps[selected]
  const matches = query.trim() ? steps.flatMap((step, i) => match(step, query, mode === 'exact') ? [i] : []) : []
  function syncNative(active = pointerInside.current && !isInput(document.activeElement), force = false) {
    if (!force && nativeActive.current === active) return
    nativeActive.current = active
    void setCanvasInputActive(active).catch(() => {
      if (mounted.current) setInputError('画布键盘控制未能同步，请重新打开流程。')
    })
  }
  function availableRect(inspectorOpen = infoOpen) {
    const element = viewport.current, rect = element?.getBoundingClientRect()
    const width = rect?.width || 800, height = rect?.height || 520
    let top = 32, bottom = height - 32, right = width - 32
    const full = element?.closest('.memos--canvas')
    if (full && rect) {
      const actions = full.querySelector('.memos__document-actions')?.getBoundingClientRect()
      if (actions?.height) top = Math.max(top, actions.bottom - rect.top + 16)
      for (const selector of ['.flow-canvas__toolbar', '.flow-canvas__hint']) {
        const overlay = full.querySelector(selector)?.getBoundingClientRect()
        if (overlay?.height) bottom = Math.min(bottom, overlay.top - rect.top - 16)
      }
    }
    if (inspectorOpen) {
      const inspector = element?.querySelector('.flow-canvas__inspector')?.getBoundingClientRect()
      right = inspector?.width && rect ? inspector.left - rect.left - 24 : width - (full ? 380 : 356)
    }
    return { left: 32, top, width: Math.max(1, right - 32), height: Math.max(1, bottom - top) }
  }
  function center(index: number) {
    const p = position(index), bounds = availableRect(true)
    setView(v => ({ ...v, x: bounds.left + bounds.width / 2 - (p.x + WIDTH / 2) * v.scale, y: bounds.top + bounds.height / 2 - (p.y + HEIGHT / 2) * v.scale }))
  }
  function focus(index: number) {
    const target = steps[index]; if (!target) return
    setSelectedId(target.id); setInfoOpen(true)
    center(index)
  }
  function zoom(factor: number, x?: number, y?: number) {
    const rect = viewport.current?.getBoundingClientRect()
    const px = x ?? (rect?.width ?? 800) / 2, py = y ?? (rect?.height ?? 520) / 2
    setView(v => {
      const scale = Math.max(.02, Math.min(2, v.scale * factor)), ratio = scale / v.scale
      return { scale, x: px - (px - v.x) * ratio, y: py - (py - v.y) * ratio }
    })
  }
  function fit() {
    const bounds = availableRect(), lastRow = Math.max(0, Math.floor((steps.length - 1) / 3))
    const width = Math.min(3, Math.max(1, steps.length)) * (WIDTH + GAP) - GAP, height = lastRow * ROW + HEIGHT
    const scale = Math.max(.02, Math.min(1, bounds.width / width, bounds.height / height))
    setView({ scale, x: bounds.left + (bounds.width - width * scale) / 2, y: bounds.top + (bounds.height - height * scale) / 2 })
  }
  const initialFit = useRef(fit)
  useLayoutEffect(() => { initialFit.current() }, [])
  useEffect(() => {
    mounted.current = true
    const element = viewport.current
    if (!element) return
    const wheel = (event: WheelEvent) => {
      if (!event.altKey || isInput(event.target)) return
      event.preventDefault()
      const rect = element.getBoundingClientRect()
      zoom(Math.exp(-Math.max(-300, Math.min(300, event.deltaY)) * .002), event.clientX - rect.left, event.clientY - rect.top)
    }
    const keydown = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.isComposing || isInput(event.target) || (!pointerInside.current && !element.contains(event.target as Node))) return
      event.preventDefault(); event.stopPropagation(); spaceHeld.current = true; setSpace(true)
    }
    const clear = () => { spaceHeld.current = false; setSpace(false); drag.current = null }
    const blur = () => { clear(); syncNative(false) }
    const focus = () => syncNative()
    // Windows clears its scope on native deactivation even if WebView DOM focus is retained.
    const refocus = () => syncNative(pointerInside.current && !isInput(document.activeElement), true)
    const keyup = (event: KeyboardEvent) => { if (event.code === 'Space') { if (spaceHeld.current) { event.preventDefault(); event.stopPropagation() }; clear() } }
    element.addEventListener('wheel', wheel, { passive: false })
    window.addEventListener('keydown', keydown, true); window.addEventListener('keyup', keyup, true); window.addEventListener('blur', blur); window.addEventListener('focusin', focus); window.addEventListener('focus', refocus)
    return () => { mounted.current = false; syncNative(false); element.removeEventListener('wheel', wheel); window.removeEventListener('keydown', keydown, true); window.removeEventListener('keyup', keyup, true); window.removeEventListener('blur', blur); window.removeEventListener('focusin', focus); window.removeEventListener('focus', refocus) }
  }, [])
  function patch(changes: Partial<FlowStep>) { if (!disabled && !readOnly) onChange(steps.map((s, i) => i === selected ? { ...s, ...changes } : s)) }
  function move(direction: number) {
    const target = selected + direction
    if (disabled || readOnly || selected < 0 || target < 0 || target >= steps.length) return
    const next = [...steps]; [next[selected], next[target]] = [next[target]!, next[selected]!]; onChange(next)
  }
  return <section className="flow-canvas">
    {inputError && <p className="alert alert--err" role="alert">{inputError}</p>}
    <div className="flow-canvas__toolbar">
      <input className="input" aria-label="搜索流程步骤" placeholder="搜索标题、负责人、操作…" value={query} onChange={e => setQuery(e.target.value)} />
      <select className="input input--compact" aria-label="搜索方式" value={mode} onChange={e => setMode(e.target.value)}><option value="fuzzy">模糊搜索</option><option value="exact">精准搜索</option></select>
      {query.trim() && <><span role="status">{matches.length} 个匹配</span><button className="btn btn--ghost btn--sm" disabled={!matches.length} aria-label="下一个搜索结果" onClick={() => focus(matches.find(i => i > selected) ?? matches[0]!)}>定位下一项</button></>}
      <div className="flow-canvas__zoom">
        <button className="btn btn--ghost btn--sm" onClick={() => setInfoOpen(true)}>流程信息</button>
        <button className="btn btn--ghost btn--sm" aria-label="缩小画布" onClick={() => zoom(1 / 1.2)}>−</button>
        <span aria-label="画布缩放比例">{Math.round(view.scale * 100)}%</span>
        <button className="btn btn--ghost btn--sm" aria-label="放大画布" onClick={() => zoom(1.2)}>＋</button>
        <button className="btn btn--ghost btn--sm" onClick={fit}>查看全图</button>
      </div>
      {!readOnly && <button className="btn btn--primary btn--sm" disabled={disabled || steps.length >= 100} onClick={() => {
        const id = crypto.randomUUID(); onChange([...steps, { id, title: '', owner: '', detail: '' }]); setSelectedId(id); setInfoOpen(true)
        center(steps.length)
      }}>添加步骤</button>}
    </div>
    <p className="setgroup__hint flow-canvas__hint">空格＋拖动平移 · Alt＋滚轮缩放 · 点击卡片查看和编辑；连线表示操作先后顺序</p>
    <div ref={viewport} className={`flow-canvas__viewport${space ? ' flow-canvas__viewport--pan' : ''}`} aria-label="流程画布" tabIndex={0}
      onPointerEnter={e => { pointerInside.current = !isInput(e.target); syncNative() }} onPointerLeave={() => { pointerInside.current = false; syncNative(false) }}
      onPointerDown={e => {
        if (!space || e.button !== 0 || isInput(e.target)) return
        e.preventDefault(); viewport.current?.setPointerCapture(e.pointerId)
        drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY, originX: view.x, originY: view.y }
      }} onPointerMove={e => {
        pointerInside.current = !isInput(e.target); syncNative()
        const start = drag.current; if (!start || start.id !== e.pointerId) return
        setView(v => ({ ...v, x: start.originX + e.clientX - start.x, y: start.originY + e.clientY - start.y }))
      }} onPointerUp={e => { if (drag.current?.id === e.pointerId) { drag.current = null; viewport.current?.releasePointerCapture(e.pointerId) } }} onPointerCancel={() => { drag.current = null; setSpace(false) }}>
      <div className="flow-canvas__scene" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}>
        <svg className="flow-canvas__edges" aria-hidden="true" width={3 * (WIDTH + GAP)} height={Math.max(1, Math.ceil(steps.length / 3)) * ROW}>
          <defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" /></marker></defs>
          {steps.slice(1).map((s, i) => {
            const a = position(i), b = position(i + 1), row = a.y !== b.y, right = a.x < b.x
            const ax = row ? a.x + WIDTH / 2 : a.x + (right ? WIDTH : 0), ay = row ? a.y + HEIGHT : a.y + HEIGHT / 2
            const bx = row ? b.x + WIDTH / 2 : b.x + (right ? 0 : WIDTH), by = row ? b.y : b.y + HEIGHT / 2
            return <path key={s.id} d={`M ${ax} ${ay} L ${bx} ${by}`} stroke="currentColor" strokeWidth="2" markerEnd={`url(#${marker})`} />
          })}
        </svg>
        {steps.map((s, i) => { const p = position(i); return <button type="button" key={s.id} style={{ left: p.x, top: p.y, width: WIDTH, height: HEIGHT }}
          className={`flow-canvas__node memos__node${matches.includes(i) ? ' flow-canvas__node--match' : ''}`} aria-label={`查看第 ${i + 1} 步：${s.title || '未命名步骤'}`} aria-pressed={selectedId === s.id}
          onClick={() => { if (!space) focus(i) }}><span className="memos__step-number">{i + 1}</span><div><h3>{s.title || '未命名步骤（待补充）'}</h3>{s.owner && <p className="memos__owner">负责人：{s.owner}</p>}<p className="flow-canvas__summary">{plain(s.detail) || '点击补充操作说明'}</p></div></button> })}
        {!steps.length && <p className="flow-canvas__empty">还没有步骤，点击「添加步骤」开始。</p>}
      </div>
      {infoOpen && <aside key={step?.id ?? 'metadata'} className="flow-canvas__inspector" aria-label={step ? `第 ${selected + 1} 步详情` : '流程信息'}>
        <div className="flow-canvas__inspector-head"><strong>{step ? `第 ${selected + 1} 步` : '流程信息'}</strong><button className="btn btn--ghost btn--sm" onClick={() => { setSelectedId(null); setInfoOpen(false) }}>收起详情</button></div>
        {metadata && <div className="flow-canvas__metadata">{metadata}</div>}
        {step && (readOnly ? <><h3>{step.title || '未命名步骤'}</h3>{step.owner && <p>负责人：{step.owner}</p>}<ContentMarkdown>{step.detail}</ContentMarkdown></> : <fieldset className="memos__step-fields" disabled={disabled}>
          <label>步骤标题<input className="input selectable" aria-label={`第 ${selected + 1} 步标题`} maxLength={300} placeholder="做什么" value={step.title} onChange={e => patch({ title: e.target.value })} /></label>
          <label>负责人（可留空）<input className="input selectable" aria-label={`第 ${selected + 1} 步负责人`} maxLength={100} value={step.owner} onChange={e => patch({ owner: e.target.value })} /></label>
          <label>操作说明<ContentEditor extractFiles className="input selectable" aria-label={`第 ${selected + 1} 步说明`} maxLength={5000} value={step.detail} disabled={disabled} onChange={e => patch({ detail: e.target.value })} /></label>
          <FlowImagePicker source={source} detail={step.detail} step={selected + 1} onChange={detail => patch({ detail })} />
          <div className="memos__step-actions"><button className="btn btn--ghost btn--sm" aria-label={`第 ${selected + 1} 步上移`} disabled={selected === 0} onClick={() => move(-1)}>上移</button><button className="btn btn--ghost btn--sm" aria-label={`第 ${selected + 1} 步下移`} disabled={selected === steps.length - 1} onClick={() => move(1)}>下移</button><button className="btn btn--ghost btn--sm" aria-label={`删除第 ${selected + 1} 步`} onClick={() => { onChange(steps.filter(s => s.id !== step.id)); setSelectedId(steps[selected - 1]?.id ?? steps[selected + 1]?.id ?? null) }}>删除步骤</button></div>
        </fieldset>)}
      </aside>}
    </div>
  </section>
}
