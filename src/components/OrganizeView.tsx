/**
 * 组织管理界面：项目、分类、标签（任务书 §4.2）。
 *
 * 三者语义在界面上必须区分清楚，否则用户会混用：
 * - 项目 = 任务集合（一个任务只属于一个项目）
 * - 分类 = 统计归类维度（§7 的"类别占比"）
 * - 标签 = 跨项目横向标记（一个任务可有多个）
 *
 * 删除时按 §4.2 要求展示影响面并让用户选择关联任务的处理方式，
 * 不使用"删除成功"这种掩盖后果的反馈。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import * as org from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import type { Category, OrphanStrategy, ProjectWithCount, TagWithCount } from '../lib/organize-ipc'
import { Icon } from './Icons'

/** 统一的错误文案提取 */
function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

// =============================================================================
// 删除确认对话框（§3「重要操作可撤销或二次确认」）
// =============================================================================

interface DeleteDialogProps {
  kind: '项目' | '分类'
  name: string
  impact: org.DeleteImpact | null
  busy: boolean
  onCancel: () => void
  onConfirm: (strategy: OrphanStrategy) => void
}

function DeleteDialog({ kind, name, impact, busy, onCancel, onConfirm }: DeleteDialogProps) {
  const [strategy, setStrategy] = useState<OrphanStrategy>('detach')

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="del-title">
      <div className="modal">
        <h2 className="modal__title" id="del-title">
          删除{kind}「{name}」
        </h2>

        {impact === null ? (
          <p className="modal__text">正在统计关联任务…</p>
        ) : impact.affectedTasks + impact.completedTasks === 0 ? (
          <p className="modal__text">该{kind}下没有任务，可以安全删除。</p>
        ) : (
          <>
            <p className="modal__text">
              该{kind}下有 <strong>{impact.affectedTasks}</strong> 个未完成任务
              {impact.completedTasks > 0 && (
                <>
                  ，以及 <strong>{impact.completedTasks}</strong> 个已完成任务
                </>
              )}
              。请选择如何处理这些任务：
            </p>

            <div className="radio-group">
              <label className="radio">
                <input
                  type="radio"
                  name="strategy"
                  checked={strategy === 'detach'}
                  onChange={() => setStrategy('detach')}
                />
                <span>
                  <strong>保留任务，仅解除归属</strong>
                  <br />
                  <span className="radio__hint">
                    任务会变成「无{kind}」，其他信息（标题、日期、标签等）完全不变。推荐。
                  </span>
                </span>
              </label>

              <label className="radio">
                <input
                  type="radio"
                  name="strategy"
                  checked={strategy === 'cascade_soft_delete'}
                  onChange={() => setStrategy('cascade_soft_delete')}
                />
                <span>
                  <strong>同时把任务移入回收站</strong>
                  <br />
                  <span className="radio__hint">
                    任务进回收站，之后仍可从回收站恢复，不会立即永久消失。
                  </span>
                </span>
              </label>
            </div>
          </>
        )}

        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--danger-solid"
            onClick={() => onConfirm(strategy)}
            disabled={busy || impact === null}
          >
            {busy ? '处理中…' : '确认删除'}
          </button>
        </div>
      </div>
    </div>
  )
}

// =============================================================================
// 合并对话框（§4.2 项目 / 分类 / 标签的重复项合并）
// =============================================================================

/** 合并候选：只用到 id / 名称 / 关联任务数 */
interface MergeItem {
  id: string
  name: string
  count?: number
}

interface MergeDialogProps {
  kind: '项目' | '分类' | '标签'
  items: MergeItem[]
  /** 从哪一行的「合并」按钮打开的：默认勾选为被合并项 */
  initialSourceId: string
  busy: boolean
  onCancel: () => void
  onConfirm: (sourceIds: string[], targetId: string) => void
}

/**
 * 合并对话框。
 *
 * 语义刻意写成"保留一个，合并掉其余"：合并是**破坏性**的
 * （源会被删除），所以必须让用户明确看到"哪个留下、哪些消失、
 * 有多少任务会转移"，而不是一句"合并成功"。
 */
function MergeDialog({ kind, items, initialSourceId, busy, onCancel, onConfirm }: MergeDialogProps) {
  const [sources, setSources] = useState<string[]>([initialSourceId])
  const [target, setTarget] = useState<string>(
    () => items.find((i) => i.id !== initialSourceId)?.id ?? '',
  )

  // 目标不能同时是被合并项：选了新目标就把冲突项移出
  const toggleSource = (id: string) => {
    if (id === target) return
    setSources((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]))
  }

  const pickTarget = (id: string) => {
    setTarget(id)
    setSources((s) => s.filter((x) => x !== id))
  }

  const movingCount = items
    .filter((i) => sources.includes(i.id))
    .reduce((n, i) => n + (i.count ?? 0), 0)
  const targetName = items.find((i) => i.id === target)?.name ?? ''
  const canSubmit = sources.length > 0 && target !== '' && !busy

  if (items.length < 2) {
    return (
      <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="merge-title">
        <div className="modal">
          <h2 className="modal__title" id="merge-title">
            合并{kind}
          </h2>
          <p className="modal__text">
            至少要有两个{kind}才能合并。当前只有一个，无需合并。
          </p>
          <div className="modal__actions">
            <button type="button" className="btn btn--primary" onClick={onCancel}>
              知道了
            </button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="merge-title">
      <div className="modal">
        <h2 className="modal__title" id="merge-title">
          合并{kind}
        </h2>
        <p className="modal__text">
          选择一个{kind}作为保留项，其余勾选的{kind}会被删除，它们名下的任务全部转到这里。
          任务本身不会被删除，只改归属。
        </p>

        <label className="field field--block">
          保留为
          <select
            className="input selectable"
            value={target}
            aria-label={`保留为哪个${kind}`}
            onChange={(e) => pickTarget(e.target.value)}
          >
            {items.map((i) => (
              <option key={i.id} value={i.id}>
                {i.name}
                {i.count !== undefined ? `（${i.count} 个任务）` : ''}
              </option>
            ))}
          </select>
        </label>

        <div className="field field--block">
          合并掉（可多选）
          <div className="mergepick">
            {items
              .filter((i) => i.id !== target)
              .map((i) => (
                <label key={i.id} className="checkbox">
                  <input
                    type="checkbox"
                    checked={sources.includes(i.id)}
                    onChange={() => toggleSource(i.id)}
                  />
                  {i.name}
                  {i.count !== undefined && (
                    <span className="orgrow__meta">{i.count} 个任务</span>
                  )}
                </label>
              ))}
          </div>
        </div>

        <p className="modal__text">
          {sources.length === 0
            ? '请至少勾选一个要合并掉的项。'
            : `将把 ${sources.length} 个${kind}合并进「${targetName}」` +
              (sources.some((s) => items.find((i) => i.id === s)?.count !== undefined)
                ? `，预计转移 ${movingCount} 个任务。`
                : '。')}
        </p>

        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary"
            disabled={!canSubmit}
            onClick={() => onConfirm(sources, target)}
          >
            {busy ? '合并中…' : '确认合并'}
          </button>
        </div>
      </div>
    </div>
  )
}

// =============================================================================
// 主组件
// =============================================================================

type Tab = 'projects' | 'categories' | 'tags'

export function OrganizeView() {
  const gate = useMemo(createRequestGate, [])
  const [tab, setTab] = useState<Tab>('projects')
  const [projects, setProjects] = useState<ProjectWithCount[]>([])
  const [categories, setCategories] = useState<Category[]>([])
  const [tags, setTags] = useState<TagWithCount[]>([])
  const [includeArchived, setIncludeArchived] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  // 新建输入
  const [newName, setNewName] = useState('')
  const [newColor, setNewColor] = useState('#4f46e5')
  const [creating, setCreating] = useState(false)

  // 行内编辑
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')

  // 删除确认
  const [deleting, setDeleting] = useState<{
    kind: '项目' | '分类'
    id: string
    name: string
    impact: org.DeleteImpact | null
  } | null>(null)
  const [busy, setBusy] = useState(false)

  // 合并（§4.2 重复项清理）
  const [merging, setMerging] = useState<{
    kind: '项目' | '分类' | '标签'
    sourceId: string
  } | null>(null)

  const reload = useCallback(async () => {
    setLoading(true)
    setError(null)
    await runLatestRequest(gate, () => Promise.all([
        org.projectList(includeArchived),
        org.categoryList(),
        org.tagList(),
      ]), { apply: ([p, c, t]) => {
      setProjects(p)
      setCategories(c)
      setTags(t)
    }, reject: (e) => setError(errText(e)), finish: () => setLoading(false) })
  }, [gate, includeArchived])

  useEffect(() => {
    void reload()
    const off = onDataChanged(['tasks', 'organization', 'all'], () => void reload())
    return () => { off(); gate.invalidate() }
  }, [gate, reload])

  useEffect(() => () => gate.dispose(), [gate])

  /** 提示条自动消失 */
  useEffect(() => {
    if (!notice) return
    const t = window.setTimeout(() => setNotice(null), 3500)
    return () => window.clearTimeout(t)
  }, [notice])

  // ------------------------------ 新建 ------------------------------
  const create = async () => {
    const name = newName.trim()
    if (!name) return
    setCreating(true)
    setError(null)
    try {
      if (tab === 'projects') {
        await org.projectCreate({ name, color: newColor })
      } else if (tab === 'categories') {
        await org.categoryCreate({ name, color: newColor })
      } else {
        await org.tagCreate({ name, color: newColor })
      }
      setNewName('')
      setNotice(`已创建${tab === 'projects' ? '项目' : tab === 'categories' ? '分类' : '标签'}「${name}」`)
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setCreating(false)
    }
  }

  // ------------------------------ 重命名 ------------------------------
  const saveRename = async () => {
    if (!editingId) return
    const name = editName.trim()
    if (!name) return
    setBusy(true)
    setError(null)
    try {
      if (tab === 'projects') await org.projectUpdate(editingId, { name })
      else if (tab === 'categories') await org.categoryUpdate(editingId, { name })
      else await org.tagUpdate(editingId, { name })
      setEditingId(null)
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  // ------------------------------ 删除 ------------------------------
  const askDelete = async (kind: '项目' | '分类', id: string, name: string) => {
    setDeleting({ kind, id, name, impact: null })
    try {
      const impact =
        kind === '项目' ? await org.projectDeleteImpact(id) : await org.categoryDeleteImpact(id)
      setDeleting({ kind, id, name, impact })
    } catch (e) {
      setDeleting(null)
      setError(errText(e))
    }
  }

  const confirmDelete = async (strategy: OrphanStrategy) => {
    if (!deleting) return
    setBusy(true)
    try {
      const affected =
        deleting.kind === '项目'
          ? await org.projectDelete(deleting.id, strategy)
          : await org.categoryDelete(deleting.id, strategy)
      setDeleting(null)
      setNotice(
        strategy === 'detach'
          ? `已删除，${affected} 个任务已解除归属但完整保留`
          : `已删除，${affected} 个任务已移入回收站`,
      )
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const deleteTag = async (id: string, name: string) => {
    if (!window.confirm(`删除标签「${name}」？\n\n任务本身不会被删除，只会解除该标签的关联。`)) {
      return
    }
    try {
      const used = await org.tagDelete(id)
      setNotice(`已删除标签「${name}」，解除了 ${used} 个任务的关联`)
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  // ------------------------------ 合并 ------------------------------
  /** 当前标签页可参与合并的条目 */
  const mergeItems: MergeItem[] =
    merging === null
      ? []
      : merging.kind === '项目'
        ? projects.map((p) => ({ id: p.id, name: p.name, count: p.totalCount }))
        : merging.kind === '分类'
          ? categories.map((c) => ({ id: c.id, name: c.name }))
          : tags.map((t) => ({ id: t.id, name: t.name, count: t.taskCount }))

  const confirmMerge = async (sourceIds: string[], targetId: string) => {
    if (!merging) return
    const targetName = mergeItems.find((i) => i.id === targetId)?.name ?? ''
    setBusy(true)
    setError(null)
    try {
      const moved =
        merging.kind === '项目'
          ? await org.projectMerge(sourceIds, targetId)
          : merging.kind === '分类'
            ? await org.categoryMerge(sourceIds, targetId)
            : await org.tagMerge(sourceIds, targetId)
      setMerging(null)
      setNotice(
        moved > 0
          ? `已合并 ${sourceIds.length} 个${merging.kind}到「${targetName}」，转移 ${moved} 个任务`
          : `已合并 ${sourceIds.length} 个${merging.kind}到「${targetName}」，没有需要转移的任务`,
      )
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  // ------------------------------ 渲染 ------------------------------
  const tabs: { id: Tab; label: string; count: number }[] = [
    { id: 'projects', label: '项目', count: projects.length },
    { id: 'categories', label: '分类', count: categories.length },
    { id: 'tags', label: '标签', count: tags.length },
  ]

  const tabHint: Record<Tab, string> = {
    projects: '项目是任务的集合，一个任务最多属于一个项目。可用于归档与合并。',
    categories: '分类用于统计中的「类别占比」，与项目相互独立，可同时设置。',
    tags: '标签是跨项目的横向标记，一个任务可以有多个，便于灵活筛选。',
  }

  return (
    <div className="organize">
      <div className="tabs" role="tablist" aria-label="组织管理">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={`tab${tab === t.id ? ' tab--active' : ''}`}
            onClick={() => {
              setTab(t.id)
              setEditingId(null)
              setError(null)
            }}
          >
            {t.label}
            <span className="tab__count">{t.count}</span>
          </button>
        ))}
      </div>

      <p className="organize__hint">{tabHint[tab]}</p>

      {/* 新建栏 */}
      <div className="organize__new">
        <input
          className="input selectable"
          value={newName}
          placeholder={`新建${tab === 'projects' ? '项目' : tab === 'categories' ? '分类' : '标签'}名称`}
          aria-label={`新建${tab === 'projects' ? '项目' : tab === 'categories' ? '分类' : '标签'}`}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void create()
          }}
        />
        <label className="field">
          颜色
          <input
            type="color"
            value={newColor}
            aria-label="颜色"
            onChange={(e) => setNewColor(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="btn btn--primary"
          disabled={creating || !newName.trim()}
          onClick={() => void create()}
        >
          {creating ? '创建中…' : '创建'}
        </button>
      </div>

      {tab === 'projects' && (
        <label className="checkbox">
          <input
            type="checkbox"
            checked={includeArchived}
            onChange={(e) => setIncludeArchived(e.target.checked)}
          />
          显示已归档项目
        </label>
      )}

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
      {notice && (
        <div className="alert alert--ok" role="status">
          {notice}
        </div>
      )}

      {loading ? (
        <div className="skeleton" style={{ height: 48, marginTop: 10 }} />
      ) : (
        <ul className="orglist">
          {tab === 'projects' &&
            (projects.length === 0 ? (
              <li className="orglist__empty">还没有项目。创建第一个项目来归类你的任务。</li>
            ) : (
              projects.map((p) => (
                <li key={p.id} className="orgrow">
                  <span
                    className="orgrow__swatch"
                    style={{ background: p.color ?? 'var(--c-border-strong)' }}
                    aria-hidden="true"
                  />
                  {editingId === p.id ? (
                    <input
                      className="input input--inline selectable"
                      value={editName}
                      autoFocus
                      aria-label="项目名称"
                      onChange={(e) => setEditName(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void saveRename()
                        if (e.key === 'Escape') setEditingId(null)
                      }}
                      onBlur={() => void saveRename()}
                    />
                  ) : (
                    <span className="orgrow__name">
                      {p.name}
                      {p.isArchived === 1 && <span className="tagchip">已归档</span>}
                      {p.isFavorite === 1 && (
                        <span className="tagchip tagchip--fav" title="已收藏">
                          <Icon name="star" size={15} />
                        </span>
                      )}
                    </span>
                  )}

                  <span className="orgrow__meta">
                    {p.openCount} 未完成 / 共 {p.totalCount}
                  </span>

                  <span className="orgrow__actions">
                    <button
                      type="button"
                      className="icon-btn"
                      title="重命名"
                      aria-label={`重命名项目 ${p.name}`}
                      onClick={() => {
                        setEditingId(p.id)
                        setEditName(p.name)
                      }}
                    >
                      <Icon name="edit" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      title={p.isArchived === 1 ? '取消归档' : '归档（保留任务，从列表隐藏）'}
                      aria-label={p.isArchived === 1 ? `取消归档 ${p.name}` : `归档 ${p.name}`}
                      onClick={async () => {
                        try {
                          await org.projectSetArchived(p.id, p.isArchived !== 1)
                          await reload()
                        } catch (e) {
                          setError(errText(e))
                        }
                      }}
                    >
                      <Icon name={p.isArchived === 1 ? 'restore' : 'archive'} size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      title="合并到其它项目"
                      aria-label={`合并项目 ${p.name}`}
                      onClick={() => setMerging({ kind: '项目', sourceId: p.id })}
                    >
                      <Icon name="merge" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除项目"
                      aria-label={`删除项目 ${p.name}`}
                      onClick={() => void askDelete('项目', p.id, p.name)}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </span>
                </li>
              ))
            ))}

          {tab === 'categories' &&
            (categories.length === 0 ? (
              <li className="orglist__empty">
                还没有分类。分类用于统计中的「类别占比」，例如「工作 / 生活 / 学习」。
              </li>
            ) : (
              categories.map((c) => (
                <li key={c.id} className="orgrow">
                  <span
                    className="orgrow__swatch"
                    style={{ background: c.color ?? 'var(--c-border-strong)' }}
                    aria-hidden="true"
                  />
                  {editingId === c.id ? (
                    <input
                      className="input input--inline selectable"
                      value={editName}
                      autoFocus
                      aria-label="分类名称"
                      onChange={(e) => setEditName(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void saveRename()
                        if (e.key === 'Escape') setEditingId(null)
                      }}
                      onBlur={() => void saveRename()}
                    />
                  ) : (
                    <span className="orgrow__name">{c.name}</span>
                  )}
                  <span className="orgrow__meta">{c.description || ''}</span>
                  <span className="orgrow__actions">
                    <button
                      type="button"
                      className="icon-btn"
                      title="重命名"
                      aria-label={`重命名分类 ${c.name}`}
                      onClick={() => {
                        setEditingId(c.id)
                        setEditName(c.name)
                      }}
                    >
                      <Icon name="edit" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      title="合并到其它分类"
                      aria-label={`合并分类 ${c.name}`}
                      onClick={() => setMerging({ kind: '分类', sourceId: c.id })}
                    >
                      <Icon name="merge" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除分类"
                      aria-label={`删除分类 ${c.name}`}
                      onClick={() => void askDelete('分类', c.id, c.name)}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </span>
                </li>
              ))
            ))}

          {tab === 'tags' &&
            (tags.length === 0 ? (
              <li className="orglist__empty">还没有标签。标签可以跨项目使用，例如「紧急」「等回复」。</li>
            ) : (
              tags.map((t) => (
                <li key={t.id} className="orgrow">
                  <span
                    className="orgrow__swatch orgrow__swatch--round"
                    style={{ background: t.color ?? 'var(--c-border-strong)' }}
                    aria-hidden="true"
                  />
                  {editingId === t.id ? (
                    <input
                      className="input input--inline selectable"
                      value={editName}
                      autoFocus
                      aria-label="标签名称"
                      onChange={(e) => setEditName(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void saveRename()
                        if (e.key === 'Escape') setEditingId(null)
                      }}
                      onBlur={() => void saveRename()}
                    />
                  ) : (
                    <span className="orgrow__name">{t.name}</span>
                  )}
                  <span className="orgrow__meta">{t.taskCount} 个任务使用</span>
                  <span className="orgrow__actions">
                    <button
                      type="button"
                      className="icon-btn"
                      title="重命名"
                      aria-label={`重命名标签 ${t.name}`}
                      onClick={() => {
                        setEditingId(t.id)
                        setEditName(t.name)
                      }}
                    >
                      <Icon name="edit" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      title="合并到其它标签"
                      aria-label={`合并标签 ${t.name}`}
                      onClick={() => setMerging({ kind: '标签', sourceId: t.id })}
                    >
                      <Icon name="merge" size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除标签"
                      aria-label={`删除标签 ${t.name}`}
                      onClick={() => void deleteTag(t.id, t.name)}
                    >
                      <Icon name="close" size={14} />
                    </button>
                  </span>
                </li>
              ))
            ))}
        </ul>
      )}

      {deleting && (
        <DeleteDialog
          kind={deleting.kind}
          name={deleting.name}
          impact={deleting.impact}
          busy={busy}
          onCancel={() => setDeleting(null)}
          onConfirm={(s) => void confirmDelete(s)}
        />
      )}

      {merging && (
        <MergeDialog
          kind={merging.kind}
          items={mergeItems}
          initialSourceId={merging.sourceId}
          busy={busy}
          onCancel={() => setMerging(null)}
          onConfirm={(sources, target) => void confirmMerge(sources, target)}
        />
      )}
    </div>
  )
}
