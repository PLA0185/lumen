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

import { useCallback, useEffect, useState } from 'react'
import * as org from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import type { Category, OrphanStrategy, ProjectWithCount, TagWithCount } from '../lib/organize-ipc'

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
// 主组件
// =============================================================================

type Tab = 'projects' | 'categories' | 'tags'

export function OrganizeView() {
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

  const reload = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [p, c, t] = await Promise.all([
        org.projectList(includeArchived),
        org.categoryList(),
        org.tagList(),
      ])
      setProjects(p)
      setCategories(c)
      setTags(t)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
    }
  }, [includeArchived])

  useEffect(() => {
    void reload()
  }, [reload])

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
            ✕
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
                          ★
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
                      ✎
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
                      {p.isArchived === 1 ? '↺' : '📦'}
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除项目"
                      aria-label={`删除项目 ${p.name}`}
                      onClick={() => void askDelete('项目', p.id, p.name)}
                    >
                      ✕
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
                      ✎
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除分类"
                      aria-label={`删除分类 ${c.name}`}
                      onClick={() => void askDelete('分类', c.id, c.name)}
                    >
                      ✕
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
                      ✎
                    </button>
                    <button
                      type="button"
                      className="icon-btn icon-btn--danger"
                      title="删除标签"
                      aria-label={`删除标签 ${t.name}`}
                      onClick={() => void deleteTag(t.id, t.name)}
                    >
                      ✕
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
    </div>
  )
}
