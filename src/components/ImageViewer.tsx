import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { save } from '@tauri-apps/plugin-dialog'
import { assetExport, assetExportImage, assetImportFile, contentError, imageMarkdown, type ContentAsset } from '../lib/content-assets'
import { annotatedImage, annotationHit, arrowHead, arrowShaft, textBounds, type Annotation, type Point } from '../lib/image-annotations'

const drafts = new Map<string, Annotation[]>()
export function ImageViewer({ asset, url, caption, onClose, onUpdate }: { asset: ContentAsset; url: string; caption: string; onClose(): void; onUpdate?: (replacement: string | null) => void | Promise<void> }) {
  const [items, setItems] = useState<Annotation[]>(() => drafts.get(asset.id) ?? [])
  const [redo, setRedo] = useState<Annotation[][]>([])
  const undo = useRef<Annotation[][]>([])
  const [tool, setTool] = useState<'select' | Annotation['kind']>('select')
  const [color, setColor] = useState('#ff3b30'), [width, setWidth] = useState(3), [fontSize, setFontSize] = useState(28)
  const [textEdit, setTextEdit] = useState<Annotation | null>(null)
  const textEditRef = useRef<Annotation | null>(null), textInput = useRef<HTMLTextAreaElement>(null)
  const manipulation = useRef<{ original: Annotation; start: Point; mode: 'move' | 'scale'; preview: Annotation } | null>(null)
  const [note, setNote] = useState(caption), [error, setError] = useState(''), [busy, setBusy] = useState(false), [selected, setSelected] = useState<string | null>(null)
  const [size, setSize] = useState({ width: 0, height: 0 })
  const [view, setView] = useState({ x: 0, y: 0, scale: 1 })
  const [drawing, setDrawing] = useState<Annotation | null>(null)
  const pan = useRef<{ x: number; y: number; originX: number; originY: number } | null>(null)
  const drawingRef = useRef<Annotation | null>(null)
  const stage = useRef<HTMLDivElement>(null), overlay = useRef<SVGSVGElement>(null), panel = useRef<HTMLDivElement>(null), mounted = useRef(true)
  const latest = useRef({ items, busy, onUpdate, onClose }); latest.current = { items, busy, onUpdate, onClose }
  useEffect(() => { drafts.set(asset.id, items) }, [asset.id, items])
  useEffect(() => {
    mounted.current = true
    const previous = document.activeElement
    panel.current?.focus({ preventScroll: true })
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (textEditRef.current) { textEditRef.current = null; setTextEdit(null) } else if (!latest.current.busy) latest.current.onClose() }
      if (e.key === 'Tab') {
        const fields = panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)')
        if (!fields?.length) return
        if (e.shiftKey && (document.activeElement === fields[0] || document.activeElement === panel.current)) { e.preventDefault(); fields[fields.length - 1]!.focus() }
        else if (!e.shiftKey && document.activeElement === fields[fields.length - 1]) { e.preventDefault(); fields[0]!.focus() }
      }
    }
    document.addEventListener('keydown', key, true)
    return () => { mounted.current = false; document.removeEventListener('keydown', key, true); if (previous instanceof HTMLElement && previous.isConnected) previous.focus({ preventScroll: true }) }
  }, [])
  useEffect(() => { if (textEdit?.id) textInput.current?.focus({ preventScroll: true }) }, [textEdit?.id])
  function fit(dimensions = size) {
    const rect = stage.current?.getBoundingClientRect()
    if (!rect || !dimensions.width) return
    const scale = Math.min(1, Math.max(.01, (rect.width - 32) / dimensions.width), Math.max(.01, (rect.height - 32) / dimensions.height))
    setView({ scale, x: (rect.width - dimensions.width * scale) / 2, y: (rect.height - dimensions.height * scale) / 2 })
  }
  function zoom(factor: number, x?: number, y?: number) {
    const rect = stage.current?.getBoundingClientRect(), px = x ?? (rect?.width ?? 800) / 2, py = y ?? (rect?.height ?? 600) / 2
    setView(v => { const scale = Math.max(.01, Math.min(8, v.scale * factor)), ratio = scale / v.scale; return { scale, x: px - (px - v.x) * ratio, y: py - (py - v.y) * ratio } })
  }
  useEffect(() => {
    const element = stage.current
    if (!element) return
    const wheel = (e: WheelEvent) => { e.preventDefault(); const rect = element.getBoundingClientRect(); zoom(Math.exp(-Math.max(-300, Math.min(300, e.deltaY)) * .002), e.clientX - rect.left, e.clientY - rect.top) }
    element.addEventListener('wheel', wheel, { passive: false })
    return () => element.removeEventListener('wheel', wheel)
  }, [])
  function commit(next: Annotation[]) {
    undo.current.push(latest.current.items); replaceItems(next); setRedo([])
  }
  function replaceItems(next: Annotation[]) { latest.current.items = next; drafts.set(asset.id, next); setItems(next) }
  function editText(item: Annotation) { textEditRef.current = item; setTextEdit(item); setSelected(item.id) }
  function finishText(): Annotation[] {
    const edit = textEditRef.current
    if (!edit) return latest.current.items
    textEditRef.current = null; setTextEdit(null)
    const existing = latest.current.items.find(item => item.id === edit.id)
    const next = existing ? latest.current.items.map(item => item.id === edit.id ? edit : item) : [...latest.current.items, edit]
    if (edit.text.trim() && JSON.stringify(existing) !== JSON.stringify(edit)) commit(next)
    setSelected(edit.text.trim() ? edit.id : null); setTool('select')
    return latest.current.items
  }
  function point(e: { clientX: number; clientY: number }): Point {
    const rect = overlay.current!.getBoundingClientRect()
    return { x: Math.max(0, Math.min(size.width, (e.clientX - rect.left) * size.width / rect.width)), y: Math.max(0, Math.min(size.height, (e.clientY - rect.top) * size.height / rect.height)) }
  }
  const run = async (task: () => Promise<void>) => {
    if (latest.current.busy) return
    latest.current.busy = true
    setBusy(true); setError('')
    try { await task() } catch (e) { if (mounted.current) setError(contentError(e)) }
    finally { latest.current.busy = false; if (mounted.current) setBusy(false) }
  }
  async function update(replacement: string | null) {
    if (!mounted.current || !latest.current.onUpdate) throw new Error('当前图片已关闭或不可编辑')
    await latest.current.onUpdate(replacement)
  }
  return createPortal(<div className="image-viewer" onPointerDown={e => e.stopPropagation()}>
    <div ref={panel} tabIndex={-1} role="dialog" aria-modal="true" aria-label="图片查看与批注" className="image-viewer__panel">
      <header className="image-viewer__toolbar"><strong>图片</strong>
        {(['select', 'pen', 'arrow', 'text'] as const).map((value, i) => <button key={value} className={`btn btn--sm ${tool === value ? 'btn--primary' : 'btn--ghost'}`} disabled={busy} aria-pressed={tool === value} onClick={() => { finishText(); setTool(value); setSelected(null) }}>{['选择', '手绘', '箭头', '文字'][i]}</button>)}
        <label>颜色<input aria-label="批注颜色" type="color" value={color} disabled={busy} onChange={e => setColor(e.target.value)} /></label>
        <label>粗细<input aria-label="批注粗细" type="number" min={1} max={30} value={width} disabled={busy} onChange={e => setWidth(Math.max(1, Math.min(30, Number(e.target.value) || 1)))} /></label>
        <button className="btn btn--ghost btn--sm" disabled={busy || !undo.current.length} onClick={() => { finishText(); const previous = undo.current.pop()!, current = latest.current.items; setRedo(history => [...history, current]); replaceItems(previous); setSelected(null) }}>撤销</button>
        <button className="btn btn--ghost btn--sm" disabled={busy || !redo.length} onClick={() => { finishText(); undo.current.push(latest.current.items); replaceItems(redo[redo.length - 1]!); setRedo(redo.slice(0, -1)); setSelected(null) }}>重做</button>
        <button className="btn btn--ghost btn--sm" disabled={busy || !selected} onClick={() => commit(items.filter(a => a.id !== selected))}>删除批注</button>
        <button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => zoom(1 / 1.2)}>缩小</button><span>{Math.round(view.scale * 100)}%</span><button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => zoom(1.2)}>放大</button><button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => fit()}>适应窗口</button>
        <button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void run(async () => {
          const current = finishText()
          const path = await save({ defaultPath: current.length ? '批注图片.png' : asset.name, filters: [{ name: '图片', extensions: current.length ? ['png', 'jpg', 'webp'] : [asset.name.split('.').pop() || 'png'] }] })
          if (!path) return
          if (!current.length) { await assetExport(asset.id, path); return }
          const extension = path.split('.').pop()?.toLowerCase(), format = extension === 'jpg' || extension === 'jpeg' ? 'jpeg' : extension === 'webp' ? 'webp' : extension === 'png' ? 'png' : null
          if (!format) throw new Error('批注图片请选择 PNG、JPEG 或 WebP 格式')
          await assetExportImage(await annotatedImage(url, current, format), path)
        })}>另存为</button>
        {onUpdate && <><button className="btn btn--primary btn--sm" disabled={busy || (!items.length && !textEdit?.text.trim())} onClick={() => void run(async () => {
          const current = finishText()
          imageMarkdown(asset.id, note)
          const file = await annotatedImage(url, current)
          if (!mounted.current) return
          const newAsset = await assetImportFile(file)
          await update(imageMarkdown(newAsset.id, note, newAsset.name)); drafts.delete(asset.id); latest.current.onClose()
        })}>保存到流程</button><button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void run(async () => { await update(null); latest.current.onClose() })}>移除图片</button></>}
        <button className="btn btn--quiet btn--sm image-viewer__close" disabled={busy} onClick={() => { finishText(); onClose() }}>关闭图片</button>
      </header>
      <div className="image-viewer__toolbar"><label>字号<input type="number" aria-label="批注字号" min={6} max={400} value={fontSize} disabled={busy} onChange={e => setFontSize(Math.max(6, Math.min(400, Number(e.target.value) || 6)))} /></label><span>文字：点击图片就地输入 · Ctrl+Enter 完成 · 双击再次编辑</span></div>
      {error && <p role="alert" className="alert alert--err">{error}</p>}
      <div ref={stage} className={`image-viewer__stage image-viewer__stage--${tool}`} onPointerDown={e => {
        if (busy || e.button !== 1) return
        e.preventDefault(); e.currentTarget.setPointerCapture(e.pointerId); pan.current = { x: e.clientX, y: e.clientY, originX: view.x, originY: view.y }
      }} onPointerMove={e => { if (pan.current) setView(v => ({ ...v, x: pan.current!.originX + e.clientX - pan.current!.x, y: pan.current!.originY + e.clientY - pan.current!.y })) }} onPointerUp={() => { pan.current = null }} onPointerCancel={() => { pan.current = null }} onLostPointerCapture={() => { pan.current = null }} onAuxClick={e => { if (e.button === 1) e.preventDefault() }}>
        <div className="image-viewer__image" style={{ width: size.width || undefined, height: size.height || undefined, transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}>
          <img src={url} alt={caption || '图片'} draggable={false} onLoad={e => { const dimensions = { width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight }; setSize(dimensions); fit(dimensions) }} onError={() => setError('图片无法解码，请检查原文件')} />
          {size.width > 0 && <svg ref={overlay} className="image-viewer__overlay" width={size.width} height={size.height} viewBox={`0 0 ${size.width} ${size.height}`} onPointerDown={e => {
            if (busy || e.button !== 0) return
            if ((e.target as Element).closest('foreignObject')) return
            e.preventDefault(); e.stopPropagation(); const p = point(e)
            const current = finishText()
            if (tool === 'select') {
              const hit = [...current].reverse().find(a => annotationHit(a, p, 8 / view.scale))
              setSelected(hit?.id ?? null)
              if (hit) { manipulation.current = { original: hit, preview: hit, start: p, mode: 'move' }; e.currentTarget.setPointerCapture(e.pointerId) }
              return
            }
            const annotation: Annotation = { id: crypto.randomUUID(), kind: tool, points: [p], color, width, text: '', fontSize }
            if (tool === 'text') { editText(annotation); return }
            e.currentTarget.setPointerCapture(e.pointerId); drawingRef.current = annotation; setDrawing(annotation)
          }} onPointerMove={e => {
            if (manipulation.current) {
              const action = manipulation.current, p = point(e), anchor = action.original.points[0]!
              const ratio = Math.hypot(p.x - anchor.x, p.y - anchor.y) / Math.max(1, Math.hypot(action.start.x - anchor.x, action.start.y - anchor.y))
              action.preview = action.mode === 'scale' ? { ...action.original, fontSize: Math.max(6, Math.min(400, action.original.fontSize * ratio)) } : { ...action.original, points: action.original.points.map(q => ({ x: q.x + p.x - action.start.x, y: q.y + p.y - action.start.y })) }
              setDrawing(action.preview); return
            }
            const current = drawingRef.current; if (!current) return
            const next = { ...current, points: current.kind === 'pen' ? [...current.points, point(e)] : [current.points[0]!, point(e)] }
            drawingRef.current = next; setDrawing(next)
          }} onPointerUp={e => {
            if (manipulation.current) {
              const action = manipulation.current
              if (JSON.stringify(action.preview) !== JSON.stringify(action.original)) commit(latest.current.items.map(item => item.id === action.original.id ? action.preview : item))
              manipulation.current = null; setDrawing(null); e.currentTarget.releasePointerCapture(e.pointerId); return
            }
            const current = drawingRef.current; if (!current) return
            if (current.points.length > 1) { commit([...latest.current.items, current]); setSelected(current.id) }
            drawingRef.current = null; setDrawing(null); e.currentTarget.releasePointerCapture(e.pointerId)
          }} onPointerCancel={() => { drawingRef.current = null; manipulation.current = null; setDrawing(null) }} onLostPointerCapture={() => { drawingRef.current = null; manipulation.current = null; setDrawing(null) }} onDoubleClick={e => {
            if (busy || tool !== 'select' || (e.target as Element).closest('foreignObject')) return
            const hit = [...latest.current.items].reverse().find(item => item.kind === 'text' && annotationHit(item, point(e), 8 / view.scale))
            if (hit) { e.stopPropagation(); editText(hit) }
          }}>
            {[...items.filter(item => item.id !== drawing?.id && item.id !== textEdit?.id), ...(drawing ? [drawing] : [])].map(item => {
              const shaft = item.kind === 'arrow' && item.points.length > 1 ? arrowShaft(item.points[0]!, item.points[item.points.length - 1]!, item.width) : null
              return <g key={item.id} data-annotation-id={item.id}>
                {item.kind === 'text' ? <text x={item.points[0]!.x} y={item.points[0]!.y} fill={item.color} fontSize={item.fontSize} dominantBaseline="text-before-edge" fontFamily="Microsoft YaHei, sans-serif">{item.text.split('\n').map((line, index) => <tspan key={index} x={item.points[0]!.x} dy={index ? item.fontSize * 1.3 : 0}>{line}</tspan>)}</text> : <><polyline points={(shaft ? [item.points[0]!, shaft.end] : item.points).map(p => `${p.x},${p.y}`).join(' ')} stroke={item.color} strokeWidth={shaft?.width ?? item.width} fill="none" strokeLinecap="round" strokeLinejoin="round" />{shaft && <polygon points={arrowHead(item.points[0]!, item.points[item.points.length - 1]!, item.width).map(p => `${p.x},${p.y}`).join(' ')} fill={item.color} />}</>}
              </g>
            })}
            {(() => {
              const item = drawing?.id === selected ? drawing : items.find(a => a.id === selected)
              if (!item || item.kind !== 'text' || textEdit) return null
              const bounds = textBounds(item), anchor = item.points[0]!, handle = 10 / view.scale
              return <><rect x={anchor.x} y={anchor.y} width={bounds.width} height={bounds.height} fill="none" stroke="#7369ff" strokeWidth={1 / view.scale} pointerEvents="none" /><rect aria-label="等比缩放文字" x={anchor.x + bounds.width - handle / 2} y={anchor.y + bounds.height - handle / 2} width={handle} height={handle} fill="#7369ff" style={{ cursor: 'nwse-resize' }} onPointerDown={e => {
                if (busy || e.button !== 0) return
                e.preventDefault(); e.stopPropagation(); manipulation.current = { original: item, preview: item, start: point(e), mode: 'scale' }; overlay.current!.setPointerCapture(e.pointerId)
              }} /></>
            })()}
            {textEdit && <foreignObject x={textEdit.points[0]!.x} y={textEdit.points[0]!.y} width={Math.min(Math.max(240, textBounds(textEdit).width + 24), Math.max(100, size.width - textEdit.points[0]!.x))} height={Math.max(80, textBounds(textEdit).height + 24)}>
              <textarea ref={textInput} className="image-viewer__text-editor" aria-label="就地编辑批注文字" maxLength={300} value={textEdit.text} style={{ fontSize: textEdit.fontSize, color: textEdit.color }} onChange={e => { const next = { ...textEdit, text: e.target.value }; textEditRef.current = next; setTextEdit(next) }} onBlur={() => finishText()} onKeyDown={e => { e.stopPropagation(); if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.nativeEvent.isComposing) { e.preventDefault(); finishText() } }} />
            </foreignObject>}
          </svg>}
        </div>
      </div>
      <footer className="image-viewer__toolbar"><span>滚轮缩放 · 中键拖动</span><label>图片备注<input className="input" aria-label="图片备注" maxLength={500} value={note} readOnly={!onUpdate} disabled={busy} placeholder={onUpdate ? '可留空' : ''} onChange={e => setNote(e.target.value)} /></label>{onUpdate && <button className="btn btn--ghost btn--sm" disabled={busy || note === caption} onClick={() => void run(async () => update(imageMarkdown(asset.id, note, asset.name)))}>保存备注</button>}</footer>
    </div>
  </div>, document.body)
}
