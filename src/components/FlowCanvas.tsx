import { useCallback, useEffect, useLayoutEffect, useId, useRef, useState, type ReactNode } from 'react'
import type { FlowStep } from '../lib/memos-ipc'
import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { FlowImagePicker } from './FlowImagePicker'
import { setCanvasInputActive } from '../lib/canvas-input'

const WIDTH = 420, HEIGHT = 148, GAP = 96
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
  const [layout, setLayout] = useState('horizontal')
  const [view, setView] = useState({ x: 32, y: 32, scale: 1 })
  const [space, setSpace] = useState(false)
  const [panning, setPanning] = useState(false)
  const [nodeHeights, setNodeHeights] = useState<Record<string, number>>({})
  const [navTop, setNavTop] = useState(84)
  const [inputError, setInputError] = useState('')
  const viewport = useRef<HTMLDivElement>(null)
  const pointerInside = useRef(false)
  const spaceHeld = useRef(false)
  const nativeActive = useRef(false)
  const mounted = useRef(true)
  const drag = useRef<{ id: number; x: number; y: number; originX: number; originY: number } | null>(null)
  const pendingFocus = useRef<string | null>(null)
  const viewTouched = useRef(false)
  const marker = useId().replace(/:/g, '')
  const selected = steps.findIndex(step => step.id === selectedId)
  const step = steps[selected]
  const matches = query.trim() ? steps.flatMap((step, i) => match(step, query, mode === 'exact') ? [i] : []) : []
  function height(index: number) { return nodeHeights[steps[index]?.id ?? ''] || HEIGHT }
  function position(index: number) {
    return layout === 'horizontal' ? { x: index * (WIDTH + GAP), y: 0 } : { x: 0, y: steps.slice(0, index).reduce((sum, _, i) => sum + height(i) + GAP, 0) }
  }
  const sceneHeight = layout === 'horizontal' ? Math.max(HEIGHT, ...Object.values(nodeHeights)) : position(Math.max(0, steps.length - 1)).y + height(steps.length - 1)
  const ownsInput = useCallback(() => {
    return !isInput(document.activeElement) && (pointerInside.current || !!viewport.current?.contains(document.activeElement))
  }, [])
  const syncNative = useCallback((active = ownsInput(), force = false) => {
    if (!force && nativeActive.current === active) return
    nativeActive.current = active
    void setCanvasInputActive(active).catch(() => {
      if (mounted.current) setInputError('画布键盘控制未能同步，请重新打开流程。')
    })
  }, [ownsInput])
  function availableRect(inspectorOpen = infoOpen) {
    const element = viewport.current, rect = element?.getBoundingClientRect()
    const width = rect?.width || 800, height = rect?.height || 520
    let top = 32, bottom = height - 32, right = width - 32
    const full = element?.closest('.memos--canvas')
    if (full && rect) {
      const actions = full.querySelector('.memos__document-actions')?.getBoundingClientRect()
      if (actions?.height) top = Math.max(top, actions.bottom - rect.top + 16)
      const navigation = full.querySelector('.flow-canvas__nav')?.getBoundingClientRect()
      if (navigation?.height) top = Math.max(top, navigation.bottom - rect.top + 16)
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
    viewTouched.current = true
    const p = position(index), bounds = availableRect()
    const height = viewport.current?.querySelectorAll<HTMLElement>('.flow-canvas__node')[index]?.offsetHeight || HEIGHT
    setView(v => ({ ...v, x: bounds.left + bounds.width / 2 - (p.x + WIDTH / 2) * v.scale, y: bounds.top + Math.max(0, (bounds.height - height * v.scale) / 2) - p.y * v.scale }))
  }
  function focus(index: number) {
    const target = steps[index]; if (!target) return
    pendingFocus.current = target.id
    setSelectedId(target.id); setInfoOpen(true)
    // If already selected React need not render again.
    if (selectedId === target.id && infoOpen) center(index)
  }
  function zoom(factor: number, x?: number, y?: number) {
    viewTouched.current = true
    const rect = viewport.current?.getBoundingClientRect()
    const px = x ?? (rect?.width ?? 800) / 2, py = y ?? (rect?.height ?? 520) / 2
    setView(v => {
      const scale = Math.max(.001, Math.min(2, v.scale * factor)), ratio = scale / v.scale
      return { scale, x: px - (px - v.x) * ratio, y: py - (py - v.y) * ratio }
    })
  }
  function fit() {
    viewTouched.current = true
    const bounds = availableRect()
    const width = layout === 'horizontal' ? Math.max(1, steps.length) * (WIDTH + GAP) - GAP : WIDTH, height = sceneHeight
    const scale = Math.max(.001, Math.min(1, bounds.width / width, bounds.height / height))
    setView({ scale, x: bounds.left + (bounds.width - width * scale) / 2, y: bounds.top + (bounds.height - height * scale) / 2 })
  }
  const initialView = useRef(() => {
    const bounds = availableRect()
    setView({ x: bounds.left, y: bounds.top, scale: 1 })
  })
  useLayoutEffect(() => { if (!viewTouched.current) initialView.current() }, [navTop])
  useLayoutEffect(() => {
    const actions = viewport.current?.closest('.memos--canvas')?.querySelector<HTMLElement>('.memos__document-actions')
    if (!actions) return
    const measure = () => setNavTop(actions.offsetTop + actions.offsetHeight + 12)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(actions)
    return () => observer.disconnect()
  }, [])
  useLayoutEffect(() => {
    if (!pendingFocus.current) return
    const index = steps.findIndex(s => s.id === pendingFocus.current)
    pendingFocus.current = null
    if (index >= 0) center(index)
  })
  useEffect(() => {
    const nodes = viewport.current?.querySelectorAll<HTMLElement>('.flow-canvas__node')
    if (!nodes) return
    const measure = () => setNodeHeights(previous => {
      const next = Object.fromEntries(Array.from(nodes, node => [node.dataset.stepId!, Math.max(HEIGHT, node.offsetHeight)]))
      return Object.keys(previous).length === nodes.length && Object.entries(next).every(([id, height]) => previous[id] === height) ? previous : next
    })
    measure()
    const observer = new ResizeObserver(measure)
    nodes.forEach(node => observer.observe(node))
    return () => observer.disconnect()
  }, [steps])
  useEffect(() => {
    mounted.current = true
    const element = viewport.current
    if (!element) return
    let listening = true, windowFocused = true
    const wheel = (event: WheelEvent) => {
      if (isInput(event.target)) return
      event.preventDefault()
      const rect = element.getBoundingClientRect()
      zoom(Math.exp(-Math.max(-300, Math.min(300, event.deltaY)) * .002), event.clientX - rect.left, event.clientY - rect.top)
    }
    const keydown = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.isComposing || isInput(event.target) || !ownsInput()) return
      event.preventDefault(); event.stopPropagation(); spaceHeld.current = true; setSpace(true)
    }
    const clear = () => { spaceHeld.current = false; setSpace(false); setPanning(false); drag.current = null }
    const blur = () => { windowFocused = false; clear(); syncNative(false) }
    const focus = () => syncNative()
    const focusout = () => queueMicrotask(() => { if (listening && windowFocused) syncNative() })
    // Windows clears its scope on native deactivation even if WebView DOM focus is retained.
    const refocus = () => { windowFocused = true; syncNative(ownsInput(), true) }
    const keyup = (event: KeyboardEvent) => { if (event.code === 'Space') { if (spaceHeld.current) { event.preventDefault(); event.stopPropagation() }; clear() } }
    element.addEventListener('wheel', wheel, { passive: false })
    window.addEventListener('keydown', keydown, true); window.addEventListener('keyup', keyup, true); window.addEventListener('blur', blur); window.addEventListener('focusin', focus); window.addEventListener('focusout', focusout); window.addEventListener('focus', refocus)
    return () => { listening = false; mounted.current = false; syncNative(false); element.removeEventListener('wheel', wheel); window.removeEventListener('keydown', keydown, true); window.removeEventListener('keyup', keyup, true); window.removeEventListener('blur', blur); window.removeEventListener('focusin', focus); window.removeEventListener('focusout', focusout); window.removeEventListener('focus', refocus) }
  }, [ownsInput, syncNative])
  function patch(changes: Partial<FlowStep>) { if (!disabled && !readOnly) onChange(steps.map((s, i) => i === selected ? { ...s, ...changes } : s)) }
  function move(direction: number) {
    const target = selected + direction
    if (disabled || readOnly || selected < 0 || target < 0 || target >= steps.length) return
    const next = [...steps]; [next[selected], next[target]] = [next[target]!, next[selected]!]; onChange(next)
  }
  return <section className="flow-canvas" style={{ '--flow-nav-top': `${navTop}px` } as import('react').CSSProperties}>
    {inputError && <p className="alert alert--err" role="alert">{inputError}</p>}
    <nav className="flow-canvas__nav" aria-label="流程节点导航">
      {steps.map((s, i) => <button key={s.id} type="button" className={`flow-canvas__nav-step${matches.includes(i) ? ' flow-canvas__nav-step--match' : ''}`} aria-label={`定位第 ${i + 1} 步：${s.title || '未命名步骤'}`} aria-current={selectedId === s.id ? 'step' : undefined} onClick={() => {
        pendingFocus.current = s.id
        setSelectedId(s.id); setInfoOpen(false)
        if (selectedId === s.id && !infoOpen) center(i)
      }}><span className="flow-canvas__nav-mark" /><span className="flow-canvas__nav-label">{i + 1} · {s.title || '未命名步骤'}</span></button>)}
    </nav>
    <div className="flow-canvas__toolbar">
      <input className="input" aria-label="搜索流程步骤" placeholder="搜索标题、负责人、操作…" value={query} onChange={e => setQuery(e.target.value)} />
      <select className="input input--compact" aria-label="搜索方式" value={mode} onChange={e => setMode(e.target.value)}><option value="fuzzy">模糊搜索</option><option value="exact">精准搜索</option></select>
      <select className="input input--compact" aria-label="流程排列方向" value={layout} onChange={e => {
        setLayout(e.target.value)
        pendingFocus.current = steps[selected >= 0 ? selected : 0]?.id ?? null
      }}><option value="horizontal">横向顺排</option><option value="vertical">纵向顺排</option></select>
      {query.trim() && <><span role="status">{matches.length} 个匹配</span><button className="btn btn--ghost btn--sm" disabled={!matches.length} aria-label="下一个搜索结果" onClick={() => focus(matches.find(i => i > selected) ?? matches[0]!)}>定位下一项</button></>}
      <div className="flow-canvas__zoom">
        <button className="btn btn--ghost btn--sm" onClick={() => { setSelectedId(null); setInfoOpen(true) }}>流程信息</button>
        <button className="btn btn--ghost btn--sm" aria-label="缩小画布" onClick={() => zoom(1 / 1.2)}>−</button>
        <span aria-label="画布缩放比例">{Math.round(view.scale * 100)}%</span>
        <button className="btn btn--ghost btn--sm" aria-label="放大画布" onClick={() => zoom(1.2)}>＋</button>
        <button className="btn btn--ghost btn--sm" onClick={fit}>查看全图</button>
      </div>
      {!readOnly && <button className="btn btn--primary btn--sm" disabled={disabled || steps.length >= 100} onClick={() => {
        const id = crypto.randomUUID(); pendingFocus.current = id; onChange([...steps, { id, title: '', owner: '', detail: '' }]); setSelectedId(id); setInfoOpen(true)
      }}>添加步骤</button>}
    </div>
    <p className="setgroup__hint flow-canvas__hint">按住鼠标中键拖动平移 · 滚轮缩放 · 卡片直接显示内容，点击编辑；连线表示操作先后顺序</p>
    <div ref={viewport} className={`flow-canvas__viewport${space || panning ? ' flow-canvas__viewport--pan' : ''}`} aria-label="流程画布" tabIndex={0}
      onPointerEnter={e => { pointerInside.current = !isInput(e.target); syncNative() }} onPointerLeave={() => { pointerInside.current = false; syncNative() }}
      onPointerDown={e => {
        if ((e.button !== 1 && !(space && e.button === 0)) || isInput(e.target)) return
        e.preventDefault(); viewport.current?.setPointerCapture(e.pointerId)
        viewTouched.current = true
        setPanning(true)
        drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY, originX: view.x, originY: view.y }
      }} onPointerMove={e => {
        pointerInside.current = !isInput(e.target); syncNative()
        const start = drag.current; if (!start || start.id !== e.pointerId) return
        setView(v => ({ ...v, x: start.originX + e.clientX - start.x, y: start.originY + e.clientY - start.y }))
      }} onPointerUp={e => { if (drag.current?.id === e.pointerId) { drag.current = null; setPanning(false); viewport.current?.releasePointerCapture(e.pointerId) } }} onPointerCancel={() => { drag.current = null; setSpace(false); setPanning(false) }} onLostPointerCapture={() => { drag.current = null; setPanning(false) }} onAuxClick={e => { if (e.button === 1) e.preventDefault() }}>
      <div className="flow-canvas__scene" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}>
        <svg className="flow-canvas__edges" aria-hidden="true" width={layout === 'horizontal' ? Math.max(1, steps.length) * (WIDTH + GAP) : WIDTH} height={sceneHeight}>
          <defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" /></marker></defs>
          {steps.slice(1).map((s, i) => {
            const a = position(i), b = position(i + 1)
            const path = layout === 'horizontal' ? `M ${a.x + WIDTH} 40 L ${b.x} 40` : `M ${WIDTH / 2} ${a.y + height(i)} L ${WIDTH / 2} ${b.y}`
            return <path key={s.id} d={path} stroke="currentColor" strokeWidth="2" markerEnd={`url(#${marker})`} />
          })}
        </svg>
        {steps.map((s, i) => { const p = position(i); return <article tabIndex={0} key={s.id} data-step-id={s.id} style={{ left: p.x, top: p.y, width: WIDTH, minHeight: HEIGHT }}
          className={`flow-canvas__node memos__node${matches.includes(i) ? ' flow-canvas__node--match' : ''}`} aria-label={`查看第 ${i + 1} 步：${s.title || '未命名步骤'}`} aria-pressed={selectedId === s.id}
          onClick={e => { if (!space && !panning && !(e.target as Element).closest('a, button') && !window.getSelection()?.toString()) focus(i) }} onKeyDown={e => { if (e.key === 'Enter' && e.target === e.currentTarget) { e.preventDefault(); focus(i) } }}><span className="memos__step-number">{i + 1}</span><div><h3><button type="button" className="flow-canvas__title" onClick={() => focus(i)}>{s.title || '未命名步骤'}</button></h3>{s.owner && <p className="memos__owner">负责人：{s.owner}</p>}{s.detail ? <div className="flow-canvas__body"><ContentMarkdown>{s.detail}</ContentMarkdown></div> : <p className="flow-canvas__summary">点击补充操作说明</p>}</div></article> })}
        {!steps.length && <p className="flow-canvas__empty">还没有步骤，点击「添加步骤」开始。</p>}
      </div>
      {infoOpen && <aside key={step?.id ?? 'metadata'} className="flow-canvas__inspector" aria-label={step ? `第 ${selected + 1} 步详情` : '流程信息'}>
        <div className="flow-canvas__inspector-head"><strong>{step ? `第 ${selected + 1} 步` : '流程信息'}</strong><button className="btn btn--ghost btn--sm" onClick={() => { setSelectedId(null); setInfoOpen(false) }}>收起详情</button></div>
        {!step && metadata && <div className="flow-canvas__metadata">{metadata}</div>}
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
