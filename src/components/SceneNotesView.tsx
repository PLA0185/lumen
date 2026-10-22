import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { listenEvent } from '../lib/event-listener'
import * as scenes from '../lib/scene-notes-ipc'
import * as memo from '../lib/memos-ipc'
import * as win from '../lib/window-ipc'
import { IpcError } from '../lib/ipc'
import { isAndroidPlatform } from '../lib/platform'
import { ContentMarkdown } from './ContentMarkdown'
import './scene-notes.css'

function errorText(error: unknown) { return error instanceof IpcError ? error.userMessage() : String(error) }
function lines(value: string) { return value.split(/[\n,，;；]+/).map(item => item.trim()).filter(Boolean) }

function emptyRule(): scenes.SceneRuleInput {
  return {
    id: null, expectedRevision: null, name: '', enabled: false, priority: 0,
    programAllowlist: ['chrome.exe'], windowTitleContains: [], matchMode: 'ocr',
    templateConfig: null, templateImageBase64: null, existingTemplateId: null,
    requiredTerms: ['FNSKU'], excludedTerms: [], headline: 'FNSKU 要扫码试一下！',
    bodyMd: '', memoId: null, stepId: null, stableHits: 2, stableLeaveMs: 6000,
    defaultSnoozeMinutes: 10,
  }
}

export function SceneNotesView() {
  const isAndroid = isAndroidPlatform()
  const [rules, setRules] = useState<scenes.SceneRule[]>([])
  const [status, setStatus] = useState<scenes.SceneMonitorStatus | null>(null)
  const [events, setEvents] = useState<scenes.SceneEvent[]>([])
  const [documents, setDocuments] = useState<memo.MemoSummary[]>([])
  const [steps, setSteps] = useState<memo.FlowStep[]>([])
  const [draft, setDraft] = useState<scenes.SceneRuleInput | null>(null)
  const [capture, setCapture] = useState<scenes.CaptureFramePayload | null>(null)
  const [selection, setSelection] = useState<{ x: number; y: number; width: number; height: number } | null>(null)
  const [test, setTest] = useState<scenes.SceneTestResult | null>(null)
  const [hotkey, setHotkey] = useState('Ctrl+Alt+Shift+S')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [waitingForCapture, setWaitingForCapture] = useState(false)
  const [error, setError] = useState('')
  const imageRef = useRef<HTMLImageElement>(null)
  const pointerStart = useRef<{ x: number; y: number } | null>(null)
  const captureTimeout = useRef<number | null>(null)

  const refresh = useCallback(async () => {
    try {
      const [nextRules, nextStatus, nextEvents] = await Promise.all([
        scenes.sceneRulesList(), scenes.sceneMonitorStatus(), scenes.sceneEventsList(),
      ])
      setRules(nextRules); setStatus(nextStatus); setEvents(nextEvents); setError('')
    } catch (e) { setError(errorText(e)) }
    finally { setLoading(false) }
  }, [])

  useEffect(() => {
    void refresh()
    void memo.memoList('', false).then(setDocuments).catch(e => setError(errorText(e)))
    if (!isAndroid) void win.windowGetConfig().then(result => setHotkey(win.humanizeAccel(result.config.sceneCaptureShortcut))).catch(() => {})
    const offReady = listenEvent<scenes.CaptureFramePayload>('scene-capture-ready', event => {
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = null
      setCapture(event.payload); setSelection(null); setWaitingForCapture(false); setTest(null)
    })
    const offArmed = listenEvent('scene-capture-armed', () => setWaitingForCapture(true))
    const offError = listenEvent<{ code: string; message: string }>('scene-capture-error', event => {
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = null
      setWaitingForCapture(false); setError(`${event.payload.message}（${event.payload.code}）`)
    })
    const offTest = listenEvent<scenes.SceneTestResult>('scene-test-result', event => {
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = null
      setTest(event.payload); setWaitingForCapture(false)
    })
    return () => {
      offReady(); offArmed(); offError(); offTest()
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = null
      void scenes.sceneCaptureCancel().catch(() => {})
    }
  }, [refresh, isAndroid])

  useEffect(() => {
    const id = draft?.memoId
    if (!id) { setSteps([]); return }
    let active = true
    void memo.memoGet(id).then(document => { if (active) setSteps(document.kind === 'flow' ? document.steps : []) })
      .catch(e => { if (active) setError(errorText(e)) })
    return () => { active = false }
  }, [draft?.memoId])

  const update = (patch: Partial<scenes.SceneRuleInput>) => setDraft(current => current ? { ...current, ...patch } : current)
  const visibleDocs = useMemo(() => documents.filter(document => !document.deletedAt), [documents])

  const edit = (rule?: scenes.SceneRule) => {
    setError(''); setCapture(null); setSelection(null); setTest(null)
    if (!rule) { setDraft(emptyRule()); return }
    setDraft({
      id: rule.id, expectedRevision: rule.revision, name: rule.name, enabled: rule.enabled,
      priority: rule.priority, programAllowlist: rule.programAllowlist,
      windowTitleContains: rule.windowTitleContains, matchMode: rule.matchMode,
      templateConfig: rule.templateConfig, templateImageBase64: null,
      existingTemplateId: rule.templateId, requiredTerms: rule.requiredTerms,
      excludedTerms: rule.excludedTerms, headline: rule.headline, bodyMd: rule.bodyMd,
      memoId: rule.memoId, stepId: rule.stepId, stableHits: rule.stableHits,
      stableLeaveMs: rule.stableLeaveMs, defaultSnoozeMinutes: rule.defaultSnoozeMinutes,
    })
  }

  const armCapture = async (forTest?: string) => {
    setError(''); setWaitingForCapture(true)
    try {
      if (forTest) await scenes.sceneTestArm(forTest); else await scenes.sceneCaptureArm()
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = window.setTimeout(() => {
        captureTimeout.current = null
        setWaitingForCapture(false)
        setError('一次性采集等待已过期，请重新开始。')
        void scenes.sceneCaptureCancel().catch(e => setError(errorText(e)))
      }, 60_000)
    } catch (e) {
      if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current)
      captureTimeout.current = null
      setWaitingForCapture(false); setError(errorText(e))
    }
  }

  const cropSelection = async () => {
    if (!capture || !imageRef.current || !selection || selection.width < 0.01 || selection.height < 0.01) {
      setError('请在画面中拖动框选要识别的局部区域。'); return
    }
    if (draft && !draft.programAllowlist.some(name => name.toLowerCase() === capture.processName.toLowerCase())) {
      setError(`当前采集程序是 ${capture.processName}，请先把它加入“允许的程序”，再确认局部选区。`); return
    }
    const x = Math.max(0, Math.round(selection.x * capture.width))
    const y = Math.max(0, Math.round(selection.y * capture.height))
    const width = Math.max(1, Math.min(capture.width - x, Math.round(selection.width * capture.width)))
    const height = Math.max(1, Math.min(capture.height - y, Math.round(selection.height * capture.height)))
    if (width < 8 || height < 8) { setError('选区至少需要 8 × 8 像素。'); return }
    const scale = Math.min(1, 1024 / Math.max(width, height))
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(width * scale)); canvas.height = Math.max(1, Math.round(height * scale))
    const ctx = canvas.getContext('2d')
    if (!ctx) { setError('无法裁剪选区。'); return }
    try {
      const preview = new Image(); preview.src = `data:image/png;base64,${capture.pngBase64}`
      await preview.decode()
      ctx.drawImage(preview, x, y, width, height, 0, 0, canvas.width, canvas.height)
      const dataUrl = canvas.toDataURL('image/png')
      const base64 = dataUrl.split(',')[1]
      if (!base64) { setError('无法读取裁剪后的局部参考图。'); return }
      const size = atob(base64).length
      if (size > 2 * 1024 * 1024) { setError('局部参考图超过 2 MiB，请缩小选区。'); return }
      const config: scenes.TemplateConfig = {
        x: x / capture.width, y: y / capture.height,
        width: width / capture.width, height: height / capture.height,
        sourceWidth: capture.width, sourceHeight: capture.height, threshold: 0.86,
      }
      update({ templateConfig: config, templateImageBase64: base64, existingTemplateId: null })
      setCapture(null); setSelection(null)
      setError('')
    } catch (e) { setError(`选区裁剪失败：${errorText(e)}`) }
  }

  const pointerPoint = (event: ReactPointerEvent<HTMLImageElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    return { x: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)), y: Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)) }
  }
  const onPointerDown = (event: ReactPointerEvent<HTMLImageElement>) => {
    const point = pointerPoint(event); pointerStart.current = point
    setSelection({ ...point, width: 0, height: 0 }); event.currentTarget.setPointerCapture(event.pointerId)
  }
  const onPointerMove = (event: ReactPointerEvent<HTMLImageElement>) => {
    if (!pointerStart.current) return
    const point = pointerPoint(event); const start = pointerStart.current
    setSelection({ x: Math.min(start.x, point.x), y: Math.min(start.y, point.y), width: Math.abs(point.x - start.x), height: Math.abs(point.y - start.y) })
  }
  const onPointerUp = () => { pointerStart.current = null }

  const save = async () => {
    if (!draft) return
    setBusy(true); setError('')
    try {
      const saved = await scenes.sceneRuleSave(draft)
      setRules(current => [saved, ...current.filter(rule => rule.id !== saved.id)])
      setDraft(null); setCapture(null); setSelection(null)
    } catch (e) { setError(errorText(e)) }
    finally { setBusy(false) }
  }

  const toggleRule = async (rule: scenes.SceneRule, enabled: boolean) => {
    try {
      const saved = await scenes.sceneRuleSetEnabled(rule.id, enabled)
      setRules(current => current.map(item => item.id === saved.id ? saved : item))
      if (!enabled) await refresh()
    } catch (e) { setError(errorText(e)) }
  }

  const toggleMonitor = async (enabled: boolean) => {
    if (enabled && !window.confirm('开启智能场景监测？\n\n只在你启用规则且目标程序位于前台时，对该单个窗口短暂采样。图像和 OCR 文字只在本机内存参与判断，不保存完整画面、不读取键盘或剪贴板、不会上传。参考图只会保存你手动框选并确认的局部区域。')) return
    setBusy(true); setError('')
    try { setStatus(await scenes.sceneMonitorSetEnabled(enabled, enabled)); await refresh() }
    catch (e) { setError(errorText(e)) }
    finally { setBusy(false) }
  }

  const deleteRule = async (rule: scenes.SceneRule) => {
    if (!window.confirm(`删除场景规则“${rule.name}”？`)) return
    try { await scenes.sceneRuleDelete(rule.id); await refresh() }
    catch (e) { setError(errorText(e)) }
  }

  if (isAndroid) return <section className="scene-page">
    <header className="scene-page__header"><div><p className="eyebrow">本机自动提醒</p><h1>场景便签</h1></div><span className="scene-status scene-status--off">Windows 专属</span></header>
    <div className="scene-callout"><strong>Android 不执行窗口画面监测。</strong><p>你的任务、备忘、流程和其它功能仍照常使用。场景识别需要 Windows 单窗口采集接口，本页在 Android 上不会读取屏幕。</p></div>
  </section>

  return <section className="scene-page">
    <header className="scene-page__header">
      <div><p className="eyebrow">本机自动提醒</p><h1>场景便签</h1><p>识别允许程序中的特定画面，在合适时机显示你保存的注意事项。</p></div>
      <div className={`scene-status${status?.enabled && !status.paused ? ' scene-status--on' : ' scene-status--off'}`}><span />{status?.enabled && !status.paused ? '监测已开启' : status?.paused ? '已暂停' : '默认关闭'}</div>
    </header>

    <div className="scene-callout">
      <strong>画面只在本机处理</strong>
      <p>仅对规则中明确允许的前台程序进行短暂采样；不会全屏录制、记录键盘或剪贴板、保存完整画面或上传内容。只有你框选并确认的局部参考图会留在本机完整备份中，不参与云同步。</p>
      <p>开启监测前，请先启用至少一条规则。随时可暂停或关闭。</p>
    </div>

    {error && <div className="alert alert--error" role="alert">{error}<button type="button" className="btn btn--ghost btn--sm" onClick={() => setError('')}>关闭</button></div>}

    <div className="scene-monitor-panel">
      <div><strong>自动监测</strong><p>{status?.reason ?? `已启用 ${status?.allowedRuleCount ?? 0} 条规则，前台画面按约 2 秒间隔判断。`}</p></div>
      <div className="scene-monitor-panel__actions">
        {status?.enabled && <button className="btn btn--ghost" type="button" disabled={busy} onClick={() => void scenes.sceneMonitorSetPaused(!status.paused).then(setStatus).catch(e => setError(errorText(e)))}>{status.paused ? '继续监测' : '暂停'}</button>}
        <button className="btn" type="button" disabled={busy || loading || !status?.platformSupported || (!status?.enabled && !rules.some(rule => rule.enabled))} onClick={() => void toggleMonitor(!status?.enabled)}>{status?.enabled ? '关闭监测' : '开启监测'}</button>
      </div>
    </div>

    <div className="scene-section-heading"><div><h2>规则</h2><span>{rules.length} 条</span></div><button type="button" className="btn btn--primary" onClick={() => edit()}>＋ 新建规则</button></div>
    {loading ? <p className="scene-empty">正在读取规则…</p> : rules.length === 0 ? <p className="scene-empty">还没有规则。新建后可选择文字识别、局部图像识别，或同时使用两者。</p> : <div className="scene-rule-list">
      {rules.map(rule => <article className="scene-rule-card" key={rule.id}>
        <div className="scene-rule-card__main"><div className="scene-rule-card__title"><strong>{rule.name}</strong><span className={rule.enabled ? 'scene-rule-tag scene-rule-tag--on' : 'scene-rule-tag'}>{rule.enabled ? '已启用' : '已关闭'}</span></div>
          <p>{rule.programAllowlist.join(', ')}{rule.windowTitleContains.length ? ` · 标题含 ${rule.windowTitleContains.join(' / ')}` : ''}</p>
          <div className="scene-rule-card__note"><b>{rule.headline}</b>{rule.bodyMd && <span>{rule.bodyMd.replace(/[#*_`]/g, '').slice(0, 90)}</span>}</div>
          <small>{rule.matchMode === 'image' ? '局部图像' : rule.matchMode === 'ocr' ? '文字识别' : '图像 + 文字'} · 连续命中 {rule.stableHits} 次 · 离开 {rule.stableLeaveMs / 1000} 秒后重置</small>
        </div>
        <div className="scene-rule-card__actions"><label className="scene-toggle"><input type="checkbox" checked={rule.enabled} onChange={event => void toggleRule(rule, event.target.checked)} /><span>启用</span></label><button type="button" className="btn btn--ghost btn--sm" onClick={() => void armCapture(rule.id)}>测试</button><button type="button" className="btn btn--ghost btn--sm" onClick={() => edit(rule)}>编辑</button><button type="button" className="btn btn--ghost btn--sm" onClick={() => void deleteRule(rule)}>删除</button></div>
      </article>)}
    </div>}

    {waitingForCapture && <div className="scene-capture-instructions" role="status"><b>已准备好一次性采集。</b><span>请在 60 秒内切换到允许的目标程序，按 <kbd>{hotkey}</kbd>。只会抓取该程序当前窗口的一帧；完成后可回到 Lumen 框选局部区域。</span><button type="button" className="btn btn--ghost btn--sm" onClick={() => { if (captureTimeout.current !== null) window.clearTimeout(captureTimeout.current); captureTimeout.current = null; setWaitingForCapture(false); void scenes.sceneCaptureCancel().catch(e => setError(errorText(e))) }}>取消等待</button></div>}
    {test && <div className={`scene-test-result${test.passed ? ' scene-test-result--pass' : ' scene-test-result--fail'}`}><strong>{test.passed ? '规则匹配' : '未匹配'}</strong><span>{test.explanation} {test.imageScore !== null ? `图像得分 ${(test.imageScore * 100).toFixed(1)}%` : ''} · {test.elapsedMs} ms</span><button type="button" className="btn btn--ghost btn--sm" onClick={() => setTest(null)}>关闭</button></div>}

    {capture && <div className="scene-capture-panel">
      <div className="scene-capture-panel__header"><div><strong>确认要识别的局部画面</strong><p>{capture.processName} · {capture.windowTitle || '无标题窗口'} · {capture.width} × {capture.height}</p></div><button type="button" className="btn btn--ghost btn--sm" onClick={() => { setCapture(null); setSelection(null) }}>丢弃画面</button></div>
      <p>拖动框选不含隐私的关键区域。完整画面只在当前预览中短暂保留，不会保存；保存规则时只保存框选区域。</p>
      <div className="scene-capture-image-wrap">
        <img ref={imageRef} src={`data:image/png;base64,${capture.pngBase64}`} alt="待选择的窗口局部参考画面" onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp} />
        {selection && <div className="scene-capture-selection" style={{ left: `${selection.x * 100}%`, top: `${selection.y * 100}%`, width: `${selection.width * 100}%`, height: `${selection.height * 100}%` }} />}
      </div>
      <div className="scene-capture-panel__actions"><button className="btn btn--primary" type="button" onClick={() => void cropSelection()}>确认局部选区</button><span>请避开客户信息、账号和其它敏感内容。</span></div>
    </div>}

    {draft && <div className="scene-editor">
      <div className="scene-editor__header"><div><p className="eyebrow">规则编辑</p><h2>{draft.id ? '修改规则' : '新建规则'}</h2></div><button type="button" className="btn btn--ghost" onClick={() => { setDraft(null); setCapture(null); setSelection(null) }}>取消</button></div>
      <div className="scene-form-grid">
        <label>规则名称<input className="input" value={draft.name} maxLength={120} onChange={e => update({ name: e.target.value })} placeholder="例如：FNSKU 标签检查" /></label>
        <label>匹配方式<select className="input" value={draft.matchMode} onChange={e => update({ matchMode: e.target.value as scenes.SceneMatchMode })}><option value="ocr">文字识别</option><option value="image">局部图像</option><option value="image_and_ocr">图像 + 文字</option></select></label>
        <label>允许的程序（.exe 文件名）<input className="input" value={draft.programAllowlist.join(', ')} onChange={e => update({ programAllowlist: lines(e.target.value) })} placeholder="例如：chrome.exe" /><small>必须填写具体程序文件名，不能用通配符；至少一个。</small></label>
        <label>窗口标题包含（可选）<input className="input" value={draft.windowTitleContains.join(', ')} onChange={e => update({ windowTitleContains: lines(e.target.value) })} placeholder="例如：Manage Inventory" /></label>
        {draft.matchMode !== 'image' && <>
          <label>必须识别到的文字（每行一项）<textarea className="input" rows={3} value={draft.requiredTerms.join('\n')} onChange={e => update({ requiredTerms: lines(e.target.value) })} placeholder="例如：FNSKU" /></label>
          <label>不能出现的文字（可选）<textarea className="input" rows={3} value={draft.excludedTerms.join('\n')} onChange={e => update({ excludedTerms: lines(e.target.value) })} placeholder="每行一项" /></label>
        </>}
        {draft.matchMode !== 'ocr' && <div className="scene-form-wide scene-template-field"><div><strong>局部参考图</strong><p>{draft.templateImageBase64 ? '已确认新选区；只保存裁剪区域。' : draft.existingTemplateId ? '已有关联的本地参考图。' : '尚未选择参考图。'}</p></div><button type="button" className="btn btn--ghost" onClick={() => void armCapture()}>采集当前窗口一帧</button></div>}
        <label>便签标题<input className="input" value={draft.headline} maxLength={240} onChange={e => update({ headline: e.target.value })} placeholder="例如：FNSKU 要扫码试一下！" /></label>
        <label>重复提醒间隔（分钟）<input className="input" type="number" min={1} max={1440} value={draft.defaultSnoozeMinutes} onChange={e => update({ defaultSnoozeMinutes: Number(e.target.value) })} /></label>
        <label className="scene-form-wide">便签正文（支持 Markdown）<textarea className="input" rows={5} value={draft.bodyMd} maxLength={20000} onChange={e => update({ bodyMd: e.target.value })} placeholder="例如：核对亚马逊下载的标签文本和 SKU。" /></label>
        <div className="scene-form-wide scene-form-row"><label>关联备忘或流程（可选）<select className="input" value={draft.memoId ?? ''} onChange={e => update({ memoId: e.target.value || null, stepId: null })}><option value="">不关联</option>{visibleDocs.map(document => <option key={document.id} value={document.id}>{document.kind === 'flow' ? '流程' : '备忘'} · {document.title}</option>)}</select></label>
          {draft.memoId && <label>关联流程步骤<select className="input" value={draft.stepId ?? ''} onChange={e => update({ stepId: e.target.value || null })}><option value="">不指定步骤</option>{steps.map(step => <option key={step.id} value={step.id}>{step.title}</option>)}</select></label>}
        </div>
        <label>连续命中次数<input className="input" type="number" min={1} max={10} value={draft.stableHits} onChange={e => update({ stableHits: Number(e.target.value) })} /></label>
        <label>离开多少秒后重置<input className="input" type="number" min={1} max={60} value={draft.stableLeaveMs / 1000} onChange={e => update({ stableLeaveMs: Number(e.target.value) * 1000 })} /></label>
      </div>
      <div className="scene-editor__footer"><span>新规则默认关闭；保存后需单独启用规则，再开启全局监测。</span><div><button type="button" className="btn btn--ghost" disabled={busy || !draft.id} onClick={() => draft.id && void armCapture(draft.id)}>测试规则</button><button type="button" className="btn btn--primary" disabled={busy} onClick={() => void save()}>{busy ? '保存中…' : '保存规则'}</button></div></div>
      {draft.templateConfig && <p className="scene-template-summary">本地局部样本已配置 · 匹配阈值 {(draft.templateConfig.threshold * 100).toFixed(0)}%</p>}
    </div>}

    {events.length > 0 && <details className="scene-diagnostics"><summary>最近的本机诊断记录（{events.length}）</summary><p>诊断不包含窗口标题、截图、OCR 文字或客户数据。</p><div>{events.slice(0, 20).map(event => <span key={event.id}>{event.occurredAt} · {event.eventType} · {event.resultCode}{event.imageScore !== null ? ` · ${(event.imageScore * 100).toFixed(0)}%` : ''}</span>)}</div></details>}
  </section>
}

export function SceneNoteWindow() {
  const [notes, setNotes] = useState<scenes.SceneNoteItem[]>([])
  const [busyId, setBusyId] = useState('')
  const [error, setError] = useState('')
  const refresh = useCallback(() => { void scenes.sceneNoteCurrent().then(setNotes).catch(e => setError(errorText(e))) }, [])
  useEffect(() => {
    refresh()
    const off = listenEvent('scene-note-refresh', refresh)
    return () => off()
  }, [refresh])
  const action = async (note: scenes.SceneNoteItem, snooze: boolean) => {
    setBusyId(note.ruleId); setError('')
    try { if (snooze) await scenes.sceneNoteSnooze(note.ruleId); else await scenes.sceneNoteAck(note.ruleId); refresh() }
    catch (e) { setError(errorText(e)) }
    finally { setBusyId('') }
  }
  return <main className="scene-note-window">
    <header><strong>场景便签</strong><span>置顶 · 本机</span></header>
    {error && <p role="alert" className="scene-note-window__error">{error}</p>}
    {notes.length === 0 ? <p className="scene-note-window__empty">当前没有待处理的场景便签。</p> : notes.map(note => <article key={note.ruleId}>
      <h1>{note.headline}</h1>{note.bodyMd && <div className="scene-note-window__body"><ContentMarkdown>{note.bodyMd}</ContentMarkdown></div>}
      {note.memoId && <button type="button" className="btn btn--ghost btn--sm scene-note-window__link-button" onClick={() => void scenes.sceneNoteOpenFlow(note.ruleId).catch(e => setError(errorText(e)))}>打开关联备忘或流程{note.stepId ? '步骤' : ''}</button>}
      <div className="scene-note-window__actions"><button type="button" className="btn btn--ghost" disabled={busyId === note.ruleId} onClick={() => void action(note, true)}>稍后提醒</button><button type="button" className="btn btn--primary" disabled={busyId === note.ruleId} onClick={() => void action(note, false)}>我知道了</button></div>
    </article>)}
  </main>
}
