import { ContentEditor } from './ContentEditor'
import { ContentMarkdown } from './ContentMarkdown'
import { useCallback, useEffect, useRef, useState } from 'react'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as memo from '../lib/memos-ipc'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import { Icon } from './Icons'
import { CloudHistory } from './CloudHistory'
import { AiFlowDialog } from './AiFlowDialog'
import { FlowRestructureDialog } from './FlowRestructureDialog'
import { FlowImagePicker } from './FlowImagePicker'
import { contentImages } from '../lib/content-assets'
import { FlowCanvas } from './FlowCanvas'
import { FlowSwitcher } from './FlowSwitcher'
import { FitToolbar } from './FitToolbar'

function draftOf(doc: memo.MemoDocument): memo.SaveMemoInput {
  return {
    id: doc.id,
    expectedRevision: doc.revision,
    title: doc.title,
    category: doc.category,
    kind: doc.kind,
    bodyMd: doc.bodyMd,
    steps: doc.steps,
  }
}
function errorText(e: unknown) {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

export function MemosView({
  query = '',
  onDirtyChange,
}: {
  query?: string
  onDirtyChange?: (dirty: boolean) => void
}) {
  const [items, setItems] = useState<memo.MemoSummary[]>([])
  const [trash, setTrash] = useState(false)
  const [category, setCategory] = useState('')
  const [selected, setSelected] = useState<memo.MemoDocument | null>(null)
  const [draft, setDraft] = useState<memo.SaveMemoInput | null>(null)
  const latestDraft = useRef(draft); latestDraft.current = draft
  const [editing, setEditing] = useState(false)
  const [loading, setLoading] = useState(true)
  const listLoaded = useRef(false)
  const [busy, setBusy] = useState(false)
  const [autoSaving, setAutoSaving] = useState(false)
  const [historyId, setHistoryId] = useState('')
  const [aiOpen, setAiOpen] = useState(false)
  const [restructuring, setRestructuring] = useState<memo.SaveMemoInput | null>(null)
  const [aiDraft, setAiDraft] = useState(false)
  const [flowView, setFlowView] = useState<'canvas' | 'list'>('canvas')
  const [canvasSession, setCanvasSession] = useState(0)
  const [showList, setShowList] = useState(false)
  const saving = useRef(false)
  const [error, setError] = useState<string | null>(null)
  const [listError, setListError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const imageSource = draft ? `${draft.bodyMd}\n${draft.steps.map(step => step.detail).join('\n')}` : ''
  const originalImages = contentImages(draft?.bodyMd ?? '')
  const attachedImages = new Set(draft?.steps.flatMap(step => contentImages(step.detail).map(image => image.id)))
  const missingImages = originalImages.filter(image => !attachedImages.has(image.id))
  const listGate = useRef(createRequestGate()).current
  const detailGate = useRef(createRequestGate()).current
  const dirty =
    !!draft &&
    (!selected || !memo.sameMemoDraft(draft, draftOf(selected)))
  const reload = useCallback(async () => {
    if (!listLoaded.current) setLoading(true)
    await runLatestRequest(listGate, () => memo.memoList(query, trash), {
      apply: (rows) => {
        listLoaded.current = true
        setItems(current => JSON.stringify(current) === JSON.stringify(rows) ? current : rows)
        setListError(null)
      },
      reject: (e) => setListError(errorText(e)),
      finish: () => setLoading(false),
    })
  }, [listGate, query, trash])
  useEffect(() => {
    listGate.activate()
    detailGate.activate()
    return () => {
      listGate.dispose()
      detailGate.dispose()
    }
  }, [listGate, detailGate])
  useEffect(() => {
    const timer = setTimeout(() => void reload(), 250)
    return () => {
      clearTimeout(timer)
      listGate.invalidate()
    }
  }, [reload, listGate])
  useEffect(() => onDataChanged(['memos'], () => {
      void reload()
      if (selected && !editing && !busy && !restructuring) {
        void runLatestRequest(detailGate, () => memo.memoGet(selected.id), {
          apply: doc => {
            setSelected(current => JSON.stringify(current) === JSON.stringify(doc) ? current : doc)
            const next = draftOf(doc)
            setDraft(current => JSON.stringify(current) === JSON.stringify(next) ? current : next)
          },
          reject: e => setError(errorText(e)),
        })
      }
    }), [reload, selected, editing, busy, detailGate, restructuring])
  useEffect(() => {
    onDirtyChange?.(dirty || busy || autoSaving)
  }, [dirty, busy, autoSaving, onDirtyChange])
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange])
  useEffect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault()
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [dirty])

  const canLeave = () =>
    !busy && !saving.current && (!dirty || window.confirm('当前修改尚未保存。放弃修改吗？'))
  const load = async (id: string, flowOnly = false) => {
    setBusy(true)
    setError(null)
    setNotice(null)
    return runLatestRequest(detailGate, async () => {
      const doc = await memo.memoGet(id)
      if (flowOnly && (doc.kind !== 'flow' || doc.deletedAt)) throw new Error('此流程已经删除或不再可用，请刷新流程列表')
      return doc
    }, {
      apply: (doc) => {
        setCanvasSession(n => n + 1)
        setFlowView('canvas')
        setAiDraft(false)
        setSelected(doc)
        setDraft(draftOf(doc))
        setEditing(false)
      },
      reject: (e) => setError(errorText(e)),
      finish: () => setBusy(false),
    })
  }
  const open = async (id: string) => { if (canLeave()) await load(id) }
  const switchFlow = async (id: string): Promise<boolean> => {
    if (id === selected?.id || busy || saving.current) return false
    if (dirty && (aiDraft || !draft?.title.trim())) { setError('请先填写流程名称并确认保存当前草稿，再切换流程。'); return false }
    if (dirty && draft) {
      const captured = draft, token = detailGate.begin()
      saving.current = true; setBusy(true); setError(null)
      try {
        const doc = await memo.memoSave(captured)
        if (!detailGate.isCurrent(token)) return false
        setSelected(doc)
        if (JSON.stringify(latestDraft.current) !== JSON.stringify(captured)) {
          setDraft(current => current ? { ...current, id: doc.id, expectedRevision: doc.revision } : current)
          setError('保存期间内容发生变化，新输入已保留，请重新选择要切换的流程。')
          return false
        }
        setDraft(draftOf(doc)); setEditing(false); setNotice('已保存到本机')
      } catch (e) { if (detailGate.isCurrent(token)) setError(errorText(e)); return false }
      finally { saving.current = false; setBusy(false) }
    }
    return load(id, true)
  }
  const create = (kind: 'memo' | 'flow') => {
    if (!canLeave()) return
    detailGate.invalidate()
    setSelected(null)
    setCanvasSession(n => n + 1)
    setFlowView('canvas')
    setAiDraft(false)
    setEditing(true)
    setError(null)
    setNotice(null)
    setDraft({
      id: null,
      expectedRevision: null,
      title: '',
      category: category || '',
      kind,
      bodyMd: '',
      steps:
        kind === 'flow'
          ? [{ id: crypto.randomUUID(), title: '', owner: '', detail: '' }]
          : [],
    })
  }
  const save = useCallback(async (automatic = false) => {
    if (!draft || busy || saving.current || (automatic && aiDraft)) return false
    saving.current = true
    if (automatic) setAutoSaving(true)
    else setBusy(true)
    setError(null)
    if (!automatic) setNotice(null)
    try {
      const doc = await memo.memoSave(draft)
      setSelected(doc)
      // Accept persisted normalization, but keep newer input typed during the write.
      if (automatic) setDraft(current => current ? memo.sameMemoDraft(current, draft) ? draftOf(doc) : { ...current, id: doc.id, expectedRevision: doc.revision } : current)
      else { setDraft(draftOf(doc)); setEditing(false); setAiDraft(false) }
      setNotice('已保存到本机')
      await reload()
      return true
    } catch (e) {
      setError(errorText(e))
      return false
    } finally {
      setBusy(false)
      setAutoSaving(false)
      saving.current = false
    }
  }, [draft, busy, reload, aiDraft])
  useEffect(() => {
    if (!editing || !dirty || busy || autoSaving || error || aiOpen || restructuring || aiDraft || !draft?.title.trim()) return
    const timer = setTimeout(() => void save(true), 1000)
    return () => clearTimeout(timer)
  }, [editing, dirty, busy, autoSaving, error, draft, save, aiOpen, aiDraft, restructuring])
  const applyRestructure = async (steps: memo.FlowStep[]) => {
    if (!restructuring || saving.current) throw new Error('当前流程正在保存，请稍后重试')
    if (JSON.stringify(latestDraft.current) !== JSON.stringify(restructuring)) throw new Error('当前流程内容已变化，请取消后重新生成细分预览，当前输入已保留')
    const token = detailGate.begin()
    saving.current = true; setBusy(true)
    try {
      const doc = await memo.memoSave({ ...restructuring, steps })
      if (!detailGate.isCurrent(token)) throw new Error('流程已切换，请重新打开查看保存结果')
      setSelected(doc); setDraft(draftOf(doc)); setRestructuring(null); setEditing(false); setAiDraft(false)
      setCanvasSession(n => n + 1); setNotice('细分已保存为新版本，可从历史版本恢复'); setError(null)
      await reload()
    } finally { saving.current = false; setBusy(false) }
  }
  const removeOrRestore = async () => {
    if (!selected || !canLeave()) return
    const deleted = !selected.deletedAt
    if (
      deleted &&
      !window.confirm(`把「${selected.title}」放入备忘回收站？可以恢复。`)
    )
      return
    setBusy(true)
    setError(null)
    try {
      await memo.memoSetDeleted(selected.id, selected.revision, deleted)
      setSelected(null)
      setDraft(null)
      setEditing(false)
      setNotice(deleted ? '已放入备忘回收站' : '已恢复')
      await reload()
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }
  const patch = (changes: Partial<memo.SaveMemoInput>) => {
    setError(null)
    setDraft((d) => (d ? { ...d, ...changes } : d))
  }
  const patchStep = (index: number, changes: Partial<memo.FlowStep>) => {
    if (draft)
      patch({
        steps: draft.steps.map((s, i) =>
          i === index ? { ...s, ...changes } : s,
        ),
      })
  }
  const moveStep = (index: number, direction: number) => {
    if (!draft) return
    const steps = [...draft.steps]
    const target = index + direction
    if (target < 0 || target >= steps.length) return
    ;[steps[index], steps[target]] = [steps[target]!, steps[index]!]
    patch({ steps })
  }
  const categories = [
    ...new Set([category, ...items.map((i) => i.category)].filter(Boolean)),
  ].sort()
  const visible = category
    ? items.filter((i) => i.category === category)
    : items

  const fullCanvas = draft?.kind === 'flow' && flowView === 'canvas'
  const metadataFields = draft ? <>
                  <label>
                    标题
                    <input
                      className="input selectable"
                      aria-label="备忘标题"
                      maxLength={500}
                      value={draft.title}
                      disabled={busy}
                      placeholder="例如：客户订单处理流程"
                      onChange={(e) => patch({ title: e.target.value })}
                    />
                  </label>
                  <label>
                    分类
                    <input
                      className="input selectable"
                      aria-label="备忘分类"
                      maxLength={100}
                      value={draft.category}
                      disabled={busy}
                      placeholder="例如：销售、财务、入职学习"
                      onChange={(e) => patch({ category: e.target.value })}
                    />
                  </label>
                  <details className="memos__materials" open={draft.kind !== 'flow' || flowView === 'list'}>
                    <summary>内容 / 流程说明与原始材料</summary>
                    <label>
                    <ContentEditor extractFiles
                      className="input selectable"
                      aria-label="备忘内容"
                      value={draft.bodyMd}
                      disabled={busy}
                      maxLength={100000}
                      placeholder="记录背景、术语、材料清单和注意事项，支持 Markdown 标题、清单、链接。"
                      onChange={(e) => patch({ bodyMd: e.target.value })}
                    />
                    </label>
                  </details>
  </> : null

  const saveStatus = aiDraft ? 'AI 草稿待确认，尚未保存' : autoSaving || busy ? '保存到本机中…' : dirty ? '编辑停顿后自动保存' : notice ?? '已保存到本机'
  const documentActions = draft ? (
              <div className="memos__document-actions memos__document-actions--merged">
                {draft.kind === 'flow' && (fullCanvas ? <button className="btn btn--ghost" onClick={() => setFlowView('list')}>返回列表</button> : <button className="btn btn--ghost" onClick={() => setFlowView('canvas')}>画布</button>)}
                {fullCanvas ? <FlowSwitcher id={draft.id} title={draft.title} disabled={busy || autoSaving} onSwitch={switchFlow} /> : <span className="chip">{draft.kind === 'flow' ? '业务流程' : '备忘录'}</span>}
                {(fullCanvas || dirty) && <span className="setgroup__hint memos__save-status" role="status" title={saveStatus}><span aria-hidden="true">{aiDraft ? '待确认' : autoSaving || busy ? '保存中' : dirty ? '待保存' : '已保存'}</span><span className="sr-only">{saveStatus}</span></span>}
                {draft.kind === 'flow' && !selected?.deletedAt && draft.steps.length > 0 && <button className="btn btn--ghost" disabled={busy || autoSaving || aiDraft} onClick={() => { if (missingImages.length) { setError('当前流程还有原图未关联步骤，自动细分无法确定这些图片的位置；现有流程已保留'); return }; setRestructuring(structuredClone(draft)) }}>细分流程</button>}
                {selected && <button className="btn btn--ghost" disabled={busy || autoSaving || dirty} onClick={() => setHistoryId(selected.id)}>历史版本</button>}
                {!selected?.deletedAt &&
                  (editing ? (
                    <>
                      <button
                        className="btn btn--primary"
                        disabled={
                          busy || autoSaving ||
                          !draft.title.trim()
                        }
                        onClick={() => void save(false)}
                      >
                        {aiDraft ? '确认保存流程' : '保存并查看'}
                      </button>
                      <button
                        className="btn btn--ghost"
                        disabled={busy}
                        onClick={() => {
                          if (!canLeave()) return
                          setDraft(selected ? draftOf(selected) : null)
                          setEditing(false)
                          setAiDraft(false)
                        }}
                      >
                        取消编辑
                      </button>
                    </>
                  ) : (
                    <button
                      className="btn btn--ghost"
                      disabled={busy}
                      onClick={() => setEditing(true)}
                    >
                      编辑记录
                    </button>
                  ))}
                <button
                  className="btn btn--ghost"
                  disabled={busy}
                  onClick={() =>
                    void (async () => {
                      try {
                        await writeText(memo.memoMarkdown(draft))
                        setNotice('已复制记录内容')
                      } catch (e) {
                        setError(errorText(e))
                      }
                    })()
                  }
                >
                  复制内容
                </button>
                {selected && (
                  <button
                    className="btn btn--ghost"
                    disabled={busy}
                    onClick={() => void removeOrRestore()}
                  >
                    {selected.deletedAt ? '恢复记录' : '删除记录'}
                  </button>
                )}
              </div>
  ) : null

  return (
    <section className={`memos${fullCanvas ? ' memos--canvas' : ''}`}>
      <FitToolbar>
        {documentActions}
        {draft?.kind === 'flow' && flowView === 'canvas' && <button className="btn btn--ghost" aria-pressed={showList} onClick={() => setShowList(!showList)}>{showList ? '收起记录列表' : '显示记录列表'}</button>}
        <button
          className="btn btn--primary"
          disabled={busy || trash}
          onClick={() => create('memo')}
        >
          <Icon name="plus" size={15} />
          新建备忘
        </button>
        <button
          className="btn btn--primary"
          disabled={busy || trash}
          onClick={() => create('flow')}
        >
          <Icon name="plus" size={15} />
          新建流程
        </button>
        <button className="btn btn--primary" disabled={busy || autoSaving || trash} onClick={() => {
          if (!canLeave()) return
          if (dirty) {
            setDraft(selected ? draftOf(selected) : null)
            setEditing(false)
            setAiDraft(false)
            setError(null)
            setNotice(null)
          }
          setAiOpen(true)
        }}>AI 生成流程</button>
        <select
          className="input input--compact"
          aria-label="备忘分类筛选"
          value={category}
          onChange={(e) => setCategory(e.target.value)}
        >
          <option value="">全部分类</option>
          {categories.map((c) => (
            <option key={c}>{c}</option>
          ))}
        </select>
        <button
          className="btn btn--ghost"
          aria-pressed={trash}
          disabled={busy}
          onClick={() => {
            if (!canLeave()) return
            setTrash(!trash)
            setCategory('')
            setSelected(null)
            setDraft(null)
            setEditing(false)
          }}
        >
          {trash ? '返回备忘' : '备忘回收站'}
        </button>
        <button
          className="btn btn--ghost"
          disabled={busy}
          onClick={() => void reload()}
        >
          刷新列表
        </button>
      </FitToolbar>
      {(error || listError) && (
        <p className="alert alert--error selectable" role="alert">
          {error || listError}
        </p>
      )}
      {notice && (
        <p className="setgroup__hint" role="status">
          {notice}
        </p>
      )}
      <div className={`memos__workspace${draft?.kind === 'flow' && flowView === 'canvas' && !showList ? ' memos__workspace--canvas' : ''}`}>
        <aside className="memos__list" aria-label="备忘与流程列表">
          <div className="memos__count">
            {trash ? '回收站' : '全部记录'} · {visible.length} 条
          </div>
          {loading && <p className="setgroup__hint">读取中…</p>}
          {!loading && visible.length === 0 && (
            <p className="setgroup__hint">
              {query
                ? '没有匹配的记录'
                : trash
                  ? '回收站为空'
                  : '把容易忘的业务步骤记在这里。'}
            </p>
          )}
          {visible.map((item) => (
            <button
              key={item.id}
              className={`memos__item${selected?.id === item.id ? ' memos__item--active' : ''}`}
              disabled={busy}
              onClick={() => void open(item.id)}
              aria-pressed={selected?.id === item.id}
            >
              <strong>{item.title}</strong>
              <span>
                {item.kind === 'flow' ? '流程' : '备忘'} ·{' '}
                {item.category || '未分类'}
              </span>
            </button>
          ))}
        </aside>
        <article className="memos__document">
          {!draft ? (
            <div className="memos__empty">
              <h2>把业务流程变成自己的随身手册</h2>
              <p>
                选择一条记录查看，或新建备忘、流程。流程中可记录操作顺序、负责人、所需材料和容易漏掉的注意事项。
              </p>
              <p className="setgroup__hint">
                内容保存在本机，并纳入「设置 → 数据与备份」的完整备份。
              </p>
            </div>
          ) : (
            <>
              {!fullCanvas && (editing && !selected?.deletedAt ? (
                <div className="memos__editor">
                  {metadataFields}
                  {draft.kind === 'flow' && flowView === 'list' && (
                    <div className="memos__step-editor">
                      <h3>流程步骤</h3>
                      <p className="setgroup__hint">
                        保存后按以下顺序显示流程路线。每个步骤可写负责人和具体操作。
                      </p>
                      {originalImages.length > 0 && <p className="setgroup__hint">{missingImages.length ? `还有 ${missingImages.length} 张原图未关联步骤，请在对应步骤下选择原图并核对。` : '原图均已关联步骤，请核对图片是否放在正确位置。'}</p>}
                      {draft.steps.map((step, i) => (
                        <fieldset
                          className="memos__step-fields"
                          key={step.id}
                          disabled={busy}
                        >
                          <legend>第 {i + 1} 步</legend>
                          <input
                            className="input selectable"
                            aria-label={`第 ${i + 1} 步标题`}
                            maxLength={300}
                            value={step.title}
                            placeholder="做什么，例如：核对订单资料"
                            onChange={(e) =>
                              patchStep(i, { title: e.target.value })
                            }
                          />
                          <input
                            className="input selectable"
                            aria-label={`第 ${i + 1} 步负责人`}
                            maxLength={100}
                            value={step.owner}
                            placeholder="找谁 / 哪个部门"
                            onChange={(e) =>
                              patchStep(i, { owner: e.target.value })
                            }
                          />
                          <ContentEditor extractFiles
                            className="input selectable"
                            aria-label={`第 ${i + 1} 步说明`}
                            maxLength={5000}
                            value={step.detail}
                            placeholder="怎么做、需要什么材料、完成标准和注意事项"
                            onChange={(e) =>
                              patchStep(i, { detail: e.target.value })
                            }
                          />
                          <FlowImagePicker source={imageSource} detail={step.detail} step={i + 1} onChange={detail => patchStep(i, { detail })} />
                          <div className="memos__step-actions">
                            <button
                              className="btn btn--ghost btn--sm"
                              aria-label={`第 ${i + 1} 步上移`}
                              disabled={i === 0}
                              onClick={() => moveStep(i, -1)}
                            >
                              上移
                            </button>
                            <button
                              className="btn btn--ghost btn--sm"
                              aria-label={`第 ${i + 1} 步下移`}
                              disabled={i === draft.steps.length - 1}
                              onClick={() => moveStep(i, 1)}
                            >
                              下移
                            </button>
                            <button
                              className="btn btn--ghost btn--sm"
                              aria-label={`删除第 ${i + 1} 步`}
                              onClick={() =>
                                patch({
                                  steps: draft.steps.filter(
                                    (s) => s.id !== step.id,
                                  ),
                                })
                              }
                            >
                              删除步骤
                            </button>
                          </div>
                        </fieldset>
                      ))}
                      <button
                        className="btn btn--ghost"
                        disabled={busy || draft.steps.length >= 100}
                        onClick={() =>
                          patch({
                            steps: [
                              ...draft.steps,
                              {
                                id: crypto.randomUUID(),
                                title: '',
                                owner: '',
                                detail: '',
                              },
                            ],
                          })
                        }
                      >
                        添加步骤
                      </button>
                    </div>
                  )}
                </div>
              ) : (
                <div className="memos__reading selectable">
                  <h2>{draft.title}</h2>
                  {draft.category && (
                    <p className="setgroup__hint">分类：{draft.category}</p>
                  )}
                  {selected?.deletedAt && (
                    <p className="alert alert--warn">
                      这条记录在回收站，恢复后可继续编辑。
                    </p>
                  )}
                  <details className="memos__materials" open={draft.kind !== 'flow' || flowView === 'list'}><summary>内容 / 流程说明与原始材料</summary><div className="mdpreview">
                    <ContentMarkdown>
                      {draft.bodyMd || '暂无补充说明。'}
                    </ContentMarkdown>
                  </div></details>
                  {draft.kind === 'flow' && flowView === 'list' && (
                    <ol className="memos__flow" aria-label="业务流程路线">
                      {draft.steps.map((step, i) => (
                        <li className="memos__node" key={step.id}>
                          <span className="memos__step-number">{i + 1}</span>
                          <div>
                            <h3>{step.title || '未命名步骤（待补充）'}</h3>
                            {step.owner && (
                              <p className="memos__owner">
                                负责人：{step.owner}
                              </p>
                            )}
                            <div className="mdpreview">
                              <ContentMarkdown>
                                {step.detail}
                              </ContentMarkdown>
                            </div>
                          </div>
                        </li>
                      ))}
                    </ol>
                  )}
                </div>
              ))}
              {draft.kind === 'flow' && flowView === 'canvas' && <>
                <FlowCanvas key={canvasSession} steps={draft.steps} source={imageSource} metadata={<div className="memos__editor">{originalImages.length > 0 && <p className="setgroup__hint">{missingImages.length ? `还有 ${missingImages.length} 张原图未关联步骤，请在对应步骤下选择原图并核对。` : '原图均已关联步骤，请核对图片是否放在正确位置。'}</p>}{editing && !selected?.deletedAt ? metadataFields : <><h2>{draft.title}</h2>{draft.category && <p className="setgroup__hint">分类：{draft.category}</p>}<ContentMarkdown>{draft.bodyMd}</ContentMarkdown></>}</div>} initialEdit={editing} readOnly={!!selected?.deletedAt} disabled={busy} onFinishEditing={async () => { if (aiDraft) return true; if (!dirty) { setEditing(false); return true }; return save(false) }} onChange={steps => { setEditing(true); patch({ steps }) }} />
              </>}
            </>
          )}
        </article>
      </div>
      {historyId && <CloudHistory id={historyId} onClose={() => setHistoryId('')} onRestored={doc => { setSelected(doc); setDraft(draftOf(doc)); setEditing(false); void reload() }} />}
      {restructuring && <FlowRestructureDialog original={restructuring} onClose={() => setRestructuring(null)} onApply={applyRestructure} />}
      {aiOpen && <AiFlowDialog onClose={() => setAiOpen(false)} onGenerated={flow => { detailGate.invalidate(); setCanvasSession(n => n + 1); setFlowView('canvas'); setSelected(null); setDraft({ ...flow, id: null, expectedRevision: null }); setEditing(true); setAiDraft(true); setAiOpen(false); setError(null); setNotice('流程草稿已生成。请核对并修改，点击「确认保存流程」后才保存。') }} />}
    </section>
  )
}
