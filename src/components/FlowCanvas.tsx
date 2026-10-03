import { useCallback, useEffect, useLayoutEffect, useId, useRef, useState, type ReactNode, type PointerEvent as ReactPointerEvent } from 'react'
import { flowStepDisplayTitle, type FlowStep } from '../lib/memos-ipc'
import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { FlowImagePicker } from './FlowImagePicker'
import { FlowStepMenu } from './FlowStepMenu'
import { SafeText } from './SafeText'
import { setCanvasInputActive } from '../lib/canvas-input'
import { replaceImageReference } from '../lib/content-assets'
import { flowLayout, autoArrange, edgePath, NODE_HEIGHT as HEIGHT, MIN_WIDTH, MAX_WIDTH, MAX_HEIGHT, MAX_POSITION } from '../lib/flow-canvas-layout'

function plain(text: string) { return text.replace(/!?\[([^\]]*)\]\(lumen-asset:[^)]+\)/g, '$1').replace(/[#*_`]/g, '') }
function match(step: FlowStep, query: string, exact: boolean) {
  const text = `${step.group?.title ?? ''}\n${step.group?.path.join('\n') ?? ''}\n${step.title}\n${step.owner}\n${plain(step.detail)}`.toLocaleLowerCase()
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
  return target instanceof Element && !!target.closest('input, textarea, select, [contenteditable="true"], [role="dialog"], .flow-canvas__inspector, .flow-canvas__inline-editor')
}

export function FlowCanvas({ steps, source, onChange, onFinishEditing, disabled = false, readOnly = false, initialEdit = false, metadata, switcher }: {
  steps: FlowStep[]; source: string; onChange: (steps: FlowStep[]) => void; onFinishEditing?: () => Promise<boolean>; disabled?: boolean; readOnly?: boolean; initialEdit?: boolean; metadata?: ReactNode; switcher?: ReactNode
}) {
  const [selectedId, setSelectedId] = useState<string | null>(() => initialEdit ? steps[0]?.id ?? null : null)
  const [query, setQuery] = useState('')
  const [infoOpen, setInfoOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(() => initialEdit ? steps[0]?.id ?? null : null)
  const [lastInitialEdit, setLastInitialEdit] = useState(initialEdit)
  if (lastInitialEdit !== initialEdit) {
    setLastInitialEdit(initialEdit)
    if (!initialEdit) setEditingId(null)
  }
  const [menu, setMenu] = useState<{ id: string; anchor: HTMLButtonElement; point?: {x:number; y:number} } | null>(null)
  const closeMenu = useCallback(() => setMenu(null), [])
  const [mode, setMode] = useState('fuzzy')
  const [layout, setLayout] = useState('horizontal')
  const [view, setView] = useState({ x: 32, y: 32, scale: 1 })
  const [space, setSpace] = useState(false)
  const [panning, setPanning] = useState(false)
  const [nodeHeights, setNodeHeights] = useState<Record<string, number>>({})
  const [transient, setTransient] = useState<{ id: string; layout: NonNullable<FlowStep['layout']> } | null>(null)
  const manipulation = useRef<{ id: string; pointerId: number; kind: 'move' | 'resize'; clientX: number; clientY: number; scale: number; original: NonNullable<FlowStep['layout']>; next: NonNullable<FlowStep['layout']> } | null>(null)
  const [navTop, setNavTop] = useState(84)
  const [inputError, setInputError] = useState('')
  const viewport = useRef<HTMLDivElement>(null)
  const pointerInside = useRef(false)
  const spaceHeld = useRef(false)
  const nativeActive = useRef(false)
  const mounted = useRef(true)
  const committing = useRef<Promise<boolean> | null>(null)
  const finishEditing = useCallback(() => {
    if (committing.current) return committing.current
    if (!editingId || disabled) return Promise.resolve(false)
    const task = (async () => {
      try {
        if (onFinishEditing && !await onFinishEditing()) return false
        setEditingId(current => current === editingId ? null : current)
        return true
      } catch (e) {
        setInputError(e instanceof Error ? e.message : String(e))
        return false
      }
    })()
    committing.current = task
    void task.finally(() => { committing.current = null })
    return task
  }, [editingId, disabled, onFinishEditing])
  useEffect(() => {
    const outside = (e: MouseEvent) => {
      const target = e.target
      if (!(target instanceof Element)) return
      const dialog = target.closest('dialog[open], [role="dialog"]')
      if (dialog && !dialog.contains(viewport.current)) return
      if (infoOpen && !target.closest('.flow-canvas__inspector, [data-flow-info-toggle]')) setInfoOpen(false)
      const editor = viewport.current?.querySelector('.flow-canvas__inline-editor')
      if (editingId && editor && !editor.contains(target) && !target.closest('button, a, input, textarea, select, .flow-canvas__node, .flow-canvas__inspector')) {
        e.preventDefault(); e.stopPropagation()
        void finishEditing()
      }
    }
    document.addEventListener('click', outside, true)
    document.addEventListener('dblclick', outside, true)
    return () => {
      document.removeEventListener('click', outside, true)
      document.removeEventListener('dblclick', outside, true)
    }
  }, [editingId, infoOpen, finishEditing])
  const drag = useRef<{ id: number; x: number; y: number; originX: number; originY: number } | null>(null)
  const pendingFocus = useRef<string | null>(null)
  const viewTouched = useRef(false)
  const marker = useId().replace(/:/g, '')
  const latest = useRef({ steps, disabled, readOnly, onChange }); latest.current = { steps, disabled, readOnly, onChange }
  const selected = steps.findIndex(step => step.id === selectedId)
  const matches = query.trim() ? steps.flatMap((step, i) => match(step, query, mode === 'exact') ? [i] : []) : []
  const scene = flowLayout(steps.map(s => transient?.id === s.id ? { ...s, layout: transient.layout } : s), nodeHeights, layout)
  const chapterTitles = [...new Set(steps.flatMap(step => step.group ? [step.group.title] : []))]
  function position(index: number) { return scene.nodes[index] ?? { id: '', x: 0, y: 0, width: 420, height: HEIGHT } }
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
    const actualHeight = viewport.current?.querySelectorAll<HTMLElement>('.flow-canvas__node')[index]?.offsetHeight || p.height
    setView(v => ({ ...v, x: bounds.left + bounds.width / 2 - (p.x + p.width / 2) * v.scale, y: bounds.top + Math.max(0, (bounds.height - actualHeight * v.scale) / 2) - p.y * v.scale }))
  }
  function focus(index: number, recenter = true) {
    const target = steps[index]; if (!target) return
    pendingFocus.current = recenter ? target.id : null
    setSelectedId(target.id); setInfoOpen(false)
    if (selectedId !== target.id) setEditingId(null)
    // If already selected React need not render again.
    if (recenter && selectedId === target.id && !infoOpen) center(index)
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
    const { width, height, left, top } = scene.bounds
    const scale = Math.max(.001, Math.min(1, bounds.width / width, bounds.height / height))
    setView({ scale, x: bounds.left + (bounds.width - width * scale) / 2 - left * scale, y: bounds.top + (bounds.height - height * scale) / 2 - top * scale })
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
  useLayoutEffect(() => {
    if (!editingId) return
    const node = Array.from(viewport.current?.querySelectorAll<HTMLElement>('.flow-canvas__node') ?? []).find(n => n.dataset.stepId === editingId)
    node?.querySelector<HTMLInputElement>('input')?.focus({ preventScroll: true })
  }, [editingId])
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
      if (manipulation.current) return
      const rect = element.getBoundingClientRect()
      zoom(Math.exp(-Math.max(-300, Math.min(300, event.deltaY)) * .002), event.clientX - rect.left, event.clientY - rect.top)
    }
    const keydown = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.isComposing || isInput(event.target) || !ownsInput() || (event.target instanceof Element && event.target.closest('.content-asset [role="button"], .flow-canvas__number'))) return
      event.preventDefault(); event.stopPropagation(); spaceHeld.current = true; setSpace(true)
    }
    const clear = () => { spaceHeld.current = false; setSpace(false); setPanning(false); drag.current = null; manipulation.current = null; setTransient(null) }
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
  function patch(id: string, changes: Partial<FlowStep>) { if (!disabled && !readOnly) onChange(steps.map(s => s.id === id ? { ...s, ...changes } : s)) }
  function updateImage(stepId: string, imageId: string, replacement: string | null) {
    const current = latest.current, step = current.steps.find(s => s.id === stepId)
    if (current.disabled || current.readOnly || !step) throw new Error('当前步骤不可编辑，请重新打开图片')
    const detail = replaceImageReference(step.detail, imageId, replacement)
    if (detail.length > 5000) throw new Error('步骤说明超过 5000 字，请缩短备注后重试')
    current.onChange(current.steps.map(s => s.id === stepId ? { ...s, detail } : s))
  }
  function beginEdit(index: number) {
    if (disabled || readOnly || !steps[index]) return
    focus(index, false); pendingFocus.current = steps[index]!.id; setEditingId(steps[index]!.id); closeMenu()
  }
  function add(index = steps.length) {
    if (disabled || readOnly || steps.length >= 100) return
    const id = crypto.randomUUID(), next = [...steps]
    next.splice(index, 0, { id, title: '', owner: '', detail: '', ...(steps[index - 1]?.group ?? steps[index]?.group ? { group: steps[index - 1]?.group ?? steps[index]?.group } : {}) })
    pendingFocus.current = id; onChange(autoArrange(next)); setSelectedId(id); setEditingId(id); setInfoOpen(false); closeMenu()
  }
  function reorder(target: number, swap = false) {
    const from = steps.findIndex(s => s.id === menu?.id)
    if (disabled || readOnly || from < 0 || target < 0 || target >= steps.length || from === target) return
    const next = [...steps], id = next[from]!.id
    if (swap) [next[from], next[target]] = [next[target]!, next[from]!]
    else next.splice(target, 0, next.splice(from, 1)[0]!)
    pendingFocus.current = id; setSelectedId(id); onChange(autoArrange(next)); closeMenu()
  }
  function startManipulation(e: ReactPointerEvent<HTMLButtonElement>, id: string, kind: 'move' | 'resize') {
    if (disabled || readOnly || space || e.button !== 0) return
    const node = scene.nodes.find(n => n.id === id)
    if (!node) return
    e.preventDefault(); e.stopPropagation(); e.currentTarget.setPointerCapture(e.pointerId); closeMenu(); setSelectedId(id); setInfoOpen(false)
    const original = { width: node.width, minHeight: kind === 'resize' ? node.height : steps.find(s => s.id === id)?.layout?.minHeight ?? HEIGHT, x: node.x, y: node.y }
    manipulation.current = { id, pointerId: e.pointerId, kind, clientX: e.clientX, clientY: e.clientY, scale: view.scale, original, next: original }
  }
  function changeManipulation(e: ReactPointerEvent<HTMLButtonElement>) {
    const current = manipulation.current
    if (!current || current.pointerId !== e.pointerId) return
    e.stopPropagation()
    const dx = (e.clientX - current.clientX) / current.scale, dy = (e.clientY - current.clientY) / current.scale
    current.next = current.kind === 'move' ? { ...current.original, x: Math.max(-MAX_POSITION, Math.min(MAX_POSITION, current.original.x! + dx)), y: Math.max(-MAX_POSITION, Math.min(MAX_POSITION, current.original.y! + dy)) } : { ...current.original, width: Math.round(Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, current.original.width + dx))), minHeight: Math.round(Math.max(HEIGHT, Math.min(MAX_HEIGHT, current.original.minHeight + dy))) }
    // Resizing automatically aligned cards keeps them aligned as width/height changes.
    if (current.kind === 'resize' && steps.find(s => s.id === current.id)?.layout?.x === undefined) { delete current.next.x; delete current.next.y }
    setTransient({ id: current.id, layout: current.next })
  }
  function finishManipulation(e: ReactPointerEvent<HTMLButtonElement>, cancel = false) {
    const current = manipulation.current
    if (!current || current.pointerId !== e.pointerId) return
    manipulation.current = null; setTransient(null)
    const value = latest.current
    if (!cancel && !value.disabled && !value.readOnly && value.steps.some(s => s.id === current.id)) value.onChange(value.steps.map(s => s.id === current.id ? { ...s, layout: current.next } : s))
    if (e.currentTarget.hasPointerCapture?.(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
  }
  return <section className="flow-canvas" style={{ '--flow-nav-top': `${navTop}px` } as import('react').CSSProperties}>
    {inputError && <p className="alert alert--err" role="alert">{inputError}</p>}
    <div className="flow-canvas__head">
    {switcher && <div className="flow-canvas__switcher">{switcher}</div>}
    <nav className="flow-canvas__nav" aria-label="流程节点导航">
      {steps.map((s, i) => {
        const previous = steps[i - 1]?.group, group = s.group
        const level = group && previous?.title !== group.title ? 'chapter' : group && JSON.stringify(previous?.path) !== JSON.stringify(group.path) ? 'stage' : 'step'
        return <button key={s.id} type="button" data-level={level} data-chapter={group ? chapterTitles.indexOf(group.title) : undefined} style={group ? { '--flow-chapter-hue': (260 + chapterTitles.indexOf(group.title) * 137.507764) % 360 } as import('react').CSSProperties : undefined} className={`flow-canvas__nav-step${matches.includes(i) ? ' flow-canvas__nav-step--match' : ''}`} aria-label={`定位第 ${i + 1} 步：${flowStepDisplayTitle(s.title)}`} aria-current={selectedId === s.id ? 'step' : undefined} onClick={() => {
        pendingFocus.current = s.id
        setSelectedId(s.id); setInfoOpen(false); setEditingId(null); closeMenu()
        if (selectedId === s.id && !infoOpen) center(i)
      }}><span className="flow-canvas__nav-mark" /><span className="flow-canvas__nav-label">{group && <strong><SafeText>{group.title}</SafeText></strong>}{!!group?.path.length && <span className="flow-canvas__nav-stage"><SafeText>{group.path.join(' › ')}</SafeText></span>}<span>{i + 1} · <SafeText>{flowStepDisplayTitle(s.title)}</SafeText></span></span></button>
      })}
    </nav>
    </div>
    <div className="flow-canvas__toolbar">
      <input className="input" aria-label="搜索流程步骤" placeholder="搜索标题、负责人、操作…" value={query} onChange={e => setQuery(e.target.value)} />
      <select className="input input--compact" aria-label="搜索方式" value={mode} onChange={e => setMode(e.target.value)}><option value="fuzzy">模糊搜索</option><option value="exact">精准搜索</option></select>
      <select className="input input--compact" aria-label="流程排列方向" value={layout} onChange={e => {
        setLayout(e.target.value)
        pendingFocus.current = steps[selected >= 0 ? selected : 0]?.id ?? null
      }}><option value="horizontal">横向顺排</option><option value="vertical">纵向顺排</option></select>
      {!readOnly && <button className="btn btn--ghost btn--sm" disabled={disabled} onClick={() => { onChange(autoArrange(steps)); pendingFocus.current = selectedId ?? steps[0]?.id ?? null }}>自动排版</button>}
      {query.trim() && <><span role="status">{matches.length} 个匹配</span><button className="btn btn--ghost btn--sm" disabled={!matches.length} aria-label="下一个搜索结果" onClick={() => focus(matches.find(i => i > selected) ?? matches[0]!)}>定位下一项</button></>}
      <div className="flow-canvas__zoom">
        <button className="btn btn--ghost btn--sm" data-flow-info-toggle onClick={() => { setEditingId(null); setInfoOpen(open => !open); closeMenu() }}>流程信息</button>
        <button className="btn btn--ghost btn--sm" aria-label="缩小画布" onClick={() => zoom(1 / 1.2)}>−</button>
        <span aria-label="画布缩放比例">{Math.round(view.scale * 100)}%</span>
        <button className="btn btn--ghost btn--sm" aria-label="放大画布" onClick={() => zoom(1.2)}>＋</button>
        <button className="btn btn--ghost btn--sm" onClick={fit}>查看全图</button>
      </div>
      {!readOnly && <button className="btn btn--primary btn--sm" disabled={disabled || steps.length >= 100} onClick={() => add()}>添加步骤</button>}
    </div>
    <p className="setgroup__hint flow-canvas__hint">{readOnly ? '中键拖动平移 · 滚轮缩放 · 顶部导航定位步骤' : '中键拖动平移 · 滚轮缩放 · 双击卡片编辑 · 点击序号调整顺序'}</p>
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
        {scene.headers.map(header => <header key={`${header.group.id}-${header.index}`} className="flow-canvas__group" aria-label={`章节分组：${header.group.title}`} style={{ left: header.x, top: header.y, width: header.width, height: header.height }}>
          <strong><SafeText>{header.group.title}</SafeText></strong>{header.group.path.length > 0 && <span><SafeText>{header.group.path.join(' › ')}</SafeText></span>}
        </header>)}
        <svg className="flow-canvas__edges" aria-hidden="true" style={{ left: scene.bounds.left, top: scene.bounds.top }} width={scene.bounds.width} height={scene.bounds.height} viewBox={`${scene.bounds.left} ${scene.bounds.top} ${scene.bounds.width} ${scene.bounds.height}`}>
          <defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" /></marker></defs>
          {steps.slice(1).map((s, i) => {
            const a = position(i), b = position(i + 1)
            const path = edgePath(a, b, layout)
            return <path key={s.id} d={path} stroke="currentColor" strokeWidth="2" markerEnd={`url(#${marker})`} />
          })}
        </svg>
        {steps.map((s, i) => { const p = position(i); return <article tabIndex={0} key={s.id} data-step-id={s.id} style={{ left: p.x, top: p.y, width: p.width, minHeight: transient?.id === s.id ? transient.layout.minHeight : s.layout?.minHeight ?? HEIGHT }}
          className={`flow-canvas__node memos__node${matches.includes(i) ? ' flow-canvas__node--match' : ''}`} aria-label={`查看第 ${i + 1} 步：${flowStepDisplayTitle(s.title)}`} aria-pressed={selectedId === s.id}
          onClick={e => { if (!space && !panning && !(e.target as Element).closest('a, button, input, textarea, select, .flow-canvas__inline-editor') && !window.getSelection()?.toString()) {
            if (editingId && editingId !== s.id) void finishEditing().then(ok => { if (ok) focus(i, false) })
            else focus(i, false)
          } }}
          onDoubleClick={e => { if (!space && !panning && !(e.target as Element).closest('a, button, img, input, textarea, select, .flow-canvas__inline-editor')) {
            if (editingId && editingId !== s.id) void finishEditing().then(ok => { if (ok) beginEdit(i) })
            else beginEdit(i)
          } }}
          onContextMenu={e => {
            if (disabled || readOnly || (e.target as Element).closest('input, textarea, select, [contenteditable="true"]')) return
            e.preventDefault(); e.stopPropagation()
            const anchor = e.currentTarget.querySelector<HTMLButtonElement>('.flow-canvas__number')!
            const next = { id: s.id, anchor, point: {x:e.clientX, y:e.clientY} }
            const open = () => { setSelectedId(s.id); setInfoOpen(false); setMenu(next) }
            if (editingId) void finishEditing().then(ok => { if (ok) open() })
            else open()
          }}
          onKeyDown={e => { if (e.key === 'Enter' && e.target === e.currentTarget) { e.preventDefault(); beginEdit(i) } }}>
          <button type="button" className="memos__step-number flow-canvas__number" aria-label={`第 ${i + 1} 步操作`} aria-haspopup="dialog" aria-expanded={menu?.id === s.id} disabled={readOnly || disabled} onClick={e => {
            setSelectedId(s.id); setInfoOpen(false)
            setMenu(menu?.id === s.id ? null : { id: s.id, anchor: e.currentTarget })
          }}>{i + 1}</button>
          <div className="flow-canvas__content">{!readOnly && <button type="button" className="flow-canvas__move-handle" aria-label={`移动第 ${i + 1} 步卡片`} title="拖动移动卡片" disabled={disabled} onPointerDown={e => startManipulation(e, s.id, 'move')} onPointerMove={changeManipulation} onPointerUp={e => finishManipulation(e)} onPointerCancel={e => finishManipulation(e, true)} onLostPointerCapture={e => finishManipulation(e, true)}>⠿</button>}{editingId === s.id && !readOnly ? <fieldset className="memos__step-fields flow-canvas__inline-editor" disabled={disabled}>
            <div className="memos__step-actions"><strong>编辑第 {i + 1} 步</strong><button type="button" className="btn btn--ghost btn--sm" onClick={() => void finishEditing()}>完成编辑</button></div>
            <label>步骤标题<input className="input selectable" aria-label={`第 ${i + 1} 步标题`} maxLength={300} placeholder="做什么" value={s.title} onChange={e => patch(s.id, { title: e.target.value })} /></label>
            <label>负责人（可留空）<input className="input selectable" aria-label={`第 ${i + 1} 步负责人`} maxLength={100} value={s.owner} onChange={e => patch(s.id, { owner: e.target.value })} /></label>
            <label>操作说明<ContentEditor key={s.id} extractFiles className="input selectable" aria-label={`第 ${i + 1} 步说明`} maxLength={5000} value={s.detail} disabled={disabled} onChange={e => patch(s.id, { detail: e.target.value })} /></label>
            <FlowImagePicker source={source} detail={s.detail} step={i + 1} onChange={detail => patch(s.id, { detail })} />
          </fieldset> : <><h3 className="flow-canvas__title"><SafeText>{flowStepDisplayTitle(s.title)}</SafeText></h3>{s.owner && <p className="memos__owner"><SafeText>{`负责人：${s.owner}`}</SafeText></p>}{s.detail ? <div className="flow-canvas__body"><ContentMarkdown preserveLines onUpdateImage={readOnly || disabled ? undefined : (id, replacement) => updateImage(s.id, id, replacement)}>{s.detail}</ContentMarkdown></div> : <p className="flow-canvas__summary">双击补充操作说明</p>}</>}
          </div>{!readOnly && <button type="button" className="flow-canvas__resize-handle" aria-label={`调整第 ${i + 1} 步卡片大小`} title="拖动调整卡片大小" disabled={disabled} onPointerDown={e => startManipulation(e, s.id, 'resize')} onPointerMove={changeManipulation} onPointerUp={e => finishManipulation(e)} onPointerCancel={e => finishManipulation(e, true)} onLostPointerCapture={e => finishManipulation(e, true)}>⌟</button>}</article> })}
        {!steps.length && <p className="flow-canvas__empty">还没有步骤，点击「添加步骤」开始。</p>}
      </div>
      {infoOpen && <aside className="flow-canvas__inspector" aria-label="流程信息">
        <div className="flow-canvas__inspector-head"><strong>流程信息</strong><button className="btn btn--ghost btn--sm" onClick={() => setInfoOpen(false)}>收起信息</button></div>
        {metadata && <div className="flow-canvas__metadata">{metadata}</div>}
      </aside>}
    </div>
    {menu && <FlowStepMenu key={menu.id} steps={steps} id={menu.id} anchor={menu.anchor} point={menu.point} disabled={disabled || readOnly} onClose={closeMenu} onMove={target => reorder(target)} onSwap={id => reorder(steps.findIndex(s => s.id === id), true)} onAdd={add} onDelete={() => {
      if (disabled || readOnly) return
      const index = steps.findIndex(s => s.id === menu.id)
      const nextId = steps[index - 1]?.id ?? steps[index + 1]?.id ?? null
      onChange(autoArrange(steps.filter(s => s.id !== menu.id))); setSelectedId(nextId); setEditingId(null); pendingFocus.current = nextId; closeMenu()
    }} />}
  </section>
}
