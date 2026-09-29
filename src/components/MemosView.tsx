import { useCallback, useEffect, useRef, useState } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize from 'rehype-sanitize'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as memo from '../lib/memos-ipc'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import { Icon } from './Icons'

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
  const [editing, setEditing] = useState(false)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [listError, setListError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const listGate = useRef(createRequestGate()).current
  const detailGate = useRef(createRequestGate()).current
  const dirty =
    !!draft &&
    (!selected || JSON.stringify(draft) !== JSON.stringify(draftOf(selected)))
  const reload = useCallback(async () => {
    setLoading(true)
    await runLatestRequest(listGate, () => memo.memoList(query, trash), {
      apply: (rows) => {
        setItems(rows)
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
    const off = onDataChanged(['memos'], () => void reload())
    return () => {
      clearTimeout(timer)
      listGate.invalidate()
      off()
    }
  }, [reload, listGate])
  useEffect(() => {
    onDirtyChange?.(dirty || busy)
  }, [dirty, busy, onDirtyChange])
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange])
  useEffect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault()
    }
    window.addEventListener('beforeunload', warn)
    return () => window.removeEventListener('beforeunload', warn)
  }, [dirty])

  const canLeave = () =>
    !busy && (!dirty || window.confirm('当前修改尚未保存。放弃修改吗？'))
  const open = async (id: string) => {
    if (!canLeave()) return
    setBusy(true)
    setError(null)
    setNotice(null)
    await runLatestRequest(detailGate, () => memo.memoGet(id), {
      apply: (doc) => {
        setSelected(doc)
        setDraft(draftOf(doc))
        setEditing(false)
      },
      reject: (e) => setError(errorText(e)),
      finish: () => setBusy(false),
    })
  }
  const create = (kind: 'memo' | 'flow') => {
    if (!canLeave()) return
    detailGate.invalidate()
    setSelected(null)
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
  const save = async () => {
    if (!draft || busy) return
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const doc = await memo.memoSave(draft)
      setSelected(doc)
      setDraft(draftOf(doc))
      setEditing(false)
      setNotice('已保存到本机')
      await reload()
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
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
  const patch = (changes: Partial<memo.SaveMemoInput>) =>
    setDraft((d) => (d ? { ...d, ...changes } : d))
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

  return (
    <section className="memos">
      <div className="memos__toolbar">
        <button
          className="btn btn--primary"
          disabled={busy || trash}
          onClick={() => create('memo')}
        >
          <Icon name="plus" size={15} />
          新建备忘
        </button>
        <button
          className="btn btn--ghost"
          disabled={busy || trash}
          onClick={() => create('flow')}
        >
          <Icon name="list" size={15} />
          新建流程
        </button>
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
      </div>
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
      <div className="memos__workspace">
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
              <div className="memos__document-actions">
                <span className="chip">
                  {draft.kind === 'flow' ? '业务流程' : '备忘录'}
                </span>
                {dirty && <span className="setgroup__hint">尚未保存</span>}
                {!selected?.deletedAt &&
                  (editing ? (
                    <>
                      <button
                        className="btn btn--primary"
                        disabled={
                          busy ||
                          !draft.title.trim() ||
                          (draft.kind === 'flow' && draft.steps.length === 0) ||
                          draft.steps.some((s) => !s.title.trim())
                        }
                        onClick={() => void save()}
                      >
                        {busy ? '保存中…' : '保存并查看'}
                      </button>
                      <button
                        className="btn btn--ghost"
                        disabled={busy}
                        onClick={() => {
                          if (!canLeave()) return
                          setDraft(selected ? draftOf(selected) : null)
                          setEditing(false)
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
              {editing && !selected?.deletedAt ? (
                <div className="memos__editor">
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
                  <label>
                    内容 / 流程说明
                    <textarea
                      className="input selectable"
                      aria-label="备忘内容"
                      value={draft.bodyMd}
                      disabled={busy}
                      maxLength={100000}
                      placeholder="记录背景、术语、材料清单和注意事项，支持 Markdown 标题、清单、链接。"
                      onChange={(e) => patch({ bodyMd: e.target.value })}
                    />
                  </label>
                  {draft.kind === 'flow' && (
                    <div className="memos__step-editor">
                      <h3>流程步骤</h3>
                      <p className="setgroup__hint">
                        保存后按以下顺序显示流程路线。每个步骤可写负责人和具体操作。
                      </p>
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
                          <textarea
                            className="input selectable"
                            aria-label={`第 ${i + 1} 步说明`}
                            maxLength={5000}
                            value={step.detail}
                            placeholder="怎么做、需要什么材料、完成标准和注意事项"
                            onChange={(e) =>
                              patchStep(i, { detail: e.target.value })
                            }
                          />
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
                  <div className="mdpreview">
                    <Markdown
                      remarkPlugins={[remarkGfm]}
                      rehypePlugins={[rehypeSanitize]}
                    >
                      {draft.bodyMd || '暂无补充说明。'}
                    </Markdown>
                  </div>
                  {draft.kind === 'flow' && (
                    <ol className="memos__flow" aria-label="业务流程路线">
                      {draft.steps.map((step, i) => (
                        <li className="memos__node" key={step.id}>
                          <span className="memos__step-number">{i + 1}</span>
                          <div>
                            <h3>{step.title}</h3>
                            {step.owner && (
                              <p className="memos__owner">
                                负责人：{step.owner}
                              </p>
                            )}
                            <div className="mdpreview">
                              <Markdown
                                remarkPlugins={[remarkGfm]}
                                rehypePlugins={[rehypeSanitize]}
                              >
                                {step.detail}
                              </Markdown>
                            </div>
                          </div>
                        </li>
                      ))}
                    </ol>
                  )}
                </div>
              )}
            </>
          )}
        </article>
      </div>
    </section>
  )
}
