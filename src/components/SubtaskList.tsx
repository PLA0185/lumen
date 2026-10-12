/**
 * 子任务列表（任务书 §4.1「子任务、进度显示」）。
 *
 * 设计取舍：子任务只做「勾选 + 增删 + 重命名」，不做嵌套。
 * 任务书未要求多层子任务，而多层会让进度统计与"主子任务完成规则"
 * 变得难以向用户解释清楚。
 */

import { useCallback, useEffect, useState } from 'react'
import * as org from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import { subtaskSequenceBlocked } from '../lib/organize-ipc'
import type { Subtask, SubtaskCompletionMode } from '../lib/organize-ipc'
import { Icon } from './Icons'
import { CheckMark } from './CheckMark'
import { formatCompletionTime } from '../lib/datetime'
import { ScopeDialog } from './ScopeDialog'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface SubtaskListProps {
  taskId: string
  isRecurring?: boolean
  taskTitle?: string
}

export function SubtaskList({
  taskId,
  isRecurring = false,
  taskTitle = '',
}: SubtaskListProps) {
  const [items, setItems] = useState<Subtask[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [newTitle, setNewTitle] = useState('')
  const [adding, setAdding] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [pending, setPending] = useState<org.SubtaskAction | null>(null)
  const [completionMode, setCompletionMode] =
    useState<SubtaskCompletionMode>('free')
  const [modeLoading, setModeLoading] = useState(true)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [nextItems, nextMode] = await Promise.all([
        org.subtaskList(taskId),
        org.subtaskCompletionModeGet(taskId),
      ])
      setItems(nextItems)
      setCompletionMode(nextMode)
      setError(null)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
      setModeLoading(false)
    }
  }, [taskId])

  useEffect(() => {
    void reload()
    return onDataChanged(['subtasks', 'tasks', 'all'], () => void reload())
  }, [reload])

  const apply = async (action: org.SubtaskAction, scope?: org.SubtaskScope) => {
    setAdding(true)
    setError(null)
    try {
      setItems(await org.subtaskChange(taskId, action, scope))
      if (action.kind === 'set_completion_mode') {
        // Whole-series edits preserve completed history, so read back the mode
        // of this occurrence instead of assuming it changed too.
        setCompletionMode(await org.subtaskCompletionModeGet(taskId))
      }
      if (action.kind === 'create') setNewTitle('')
      setEditingId(null)
      setPending(null)
    } finally {
      setAdding(false)
    }
  }
  const request = async (action: org.SubtaskAction) => {
    if (adding || pending) return
    if (isRecurring) {
      setPending(action)
      return
    }
    try {
      await apply(action)
    } catch (e) {
      setError(errText(e))
    }
  }
  const add = () => {
    const title = newTitle.trim()
    if (title) void request({ kind: 'create', title })
  }

  const toggle = async (s: Subtask) => {
    try {
      await org.subtaskUpdate(s.id, { isDone: s.isDone !== 1 })
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  const saveRename = async (s: Subtask) => {
    const t = editTitle.trim()
    if (!t || t === s.title) {
      setEditingId(null)
      return
    }
    await request({ kind: 'rename', id: s.id, title: t })
  }
  const startRename = (s: Subtask) => {
    setEditingId(s.id)
    setEditTitle(s.title)
  }

  const remove = async (s: Subtask) => {
    await request({ kind: 'delete', id: s.id })
  }

  const move = (index: number, delta: -1 | 1) => {
    const next = [...items]
    const target = index + delta
    if (target < 0 || target >= next.length) return
    const [moved] = next.splice(index, 1)
    if (!moved) return
    next.splice(target, 0, moved)
    void request({ kind: 'reorder', orderedIds: next.map((item) => item.id) })
  }

  const done = items.filter((s) => s.isDone === 1).length
  const percent =
    items.length > 0 ? Math.round((done * 100) / items.length) : null

  return (
    <div className="subtasks">
      <div className="subtasks__head">
        <span>子任务</span>
        {percent !== null && (
          <>
            <span className="progress">
              <span
                className="progress__bar"
                style={{ width: `${percent}%` }}
              />
            </span>
            <span>
              {done}/{items.length}（{percent}%）
            </span>
          </>
        )}
      </div>

      <label className="subtask-mode">
        <span>完成方式</span>
        <select
          className="input input--compact"
          aria-label="子任务完成方式"
          value={completionMode}
          disabled={adding || pending !== null || modeLoading || editingId !== null}
          onChange={(event) =>
            void request({
              kind: 'set_completion_mode',
              mode: event.target.value as SubtaskCompletionMode,
            })
          }
        >
          <option value="free">自由完成</option>
          <option value="sequential">按顺序完成</option>
        </select>
        <span className="subtask-mode__hint">
          {completionMode === 'sequential'
            ? '完成和撤销都按列表顺序'
            : '可按任意顺序完成'}
        </span>
      </label>

      {loading ? (
        <div className="skeleton" style={{ height: 26 }} />
      ) : items.length === 0 ? (
        <p className="reminders__empty">
          还没有子任务。把这件事拆成几步会更清楚。
          {isRecurring && (
            <button
              type="button"
              className="btn btn--quiet btn--sm"
              disabled={adding || pending !== null}
              onClick={() => void request({ kind: 'copy_previous' })}
            >
              从上一次复制子任务
            </button>
          )}
        </p>
      ) : (
        <ul className="sublist">
          {items.map((s, index) => {
            const sequenceBlocked = subtaskSequenceBlocked(
              items,
              index,
              completionMode,
            )
            const orderLocked = completionMode === 'sequential' && done > 0
            return (
              <li
                key={s.id}
                className={`subtask${s.isDone === 1 ? ' subtask--done' : ''}`}
              >
                <span
                  className="subtask__order"
                  aria-label={`第 ${index + 1} 步`}
                >
                  {index + 1}
                </span>
                <span className="subtask__move">
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={`上移子任务「${s.title}」`}
                    title={
                      orderLocked ? '按顺序完成已有进度，不能调整顺序' : '上移'
                    }
                    disabled={adding || pending !== null || editingId !== null || orderLocked || index === 0}
                    onClick={() => move(index, -1)}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={`下移子任务「${s.title}」`}
                    title={
                      orderLocked ? '按顺序完成已有进度，不能调整顺序' : '下移'
                    }
                    disabled={
                      adding ||
                      pending !== null ||
                      editingId !== null ||
                      orderLocked ||
                      index === items.length - 1
                    }
                    onClick={() => move(index, 1)}
                  >
                    ↓
                  </button>
                </span>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={s.isDone === 1}
                  aria-label={
                    s.isDone === 1
                      ? `将子任务「${s.title}」标记为未完成`
                      : `完成子任务「${s.title}」`
                  }
                  className="task__check task__check--sm"
                  disabled={adding || pending !== null || sequenceBlocked}
                  title={sequenceBlocked ? '请按列表顺序完成子任务' : undefined}
                  onClick={() => void toggle(s)}
                >
                  {s.isDone === 1 && <CheckMark size={11} />}
                </button>

                {editingId === s.id ? (
                  <input
                    className="input input--inline selectable"
                    value={editTitle}
                    autoFocus
                    aria-label="子任务标题"
                    onChange={(e) => setEditTitle(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.nativeEvent.isComposing || e.keyCode === 229) return
                      if (e.key === 'Enter') void saveRename(s)
                      if (e.key === 'Escape') setEditingId(null)
                    }}
                    onBlur={() => void saveRename(s)}
                  />
                ) : (
                  <button
                    type="button"
                    className="subtask__title selectable"
                    style={{ textAlign: 'left', padding: 0 }}
                    aria-label={`重命名子任务「${s.title}」`}
                    disabled={adding || pending !== null}
                    onClick={(e) => {
                      if (e.detail === 0 || !window.getSelection()?.toString())
                        startRename(s)
                    }}
                    onDoubleClick={() => startRename(s)}
                    title="点击或双击可重命名，也可聚焦后按回车"
                  >
                    {s.title}
                  </button>
                )}

                {s.isDone === 1 && (
                  <span className="subtask__completed selectable">
                    {formatCompletionTime(s.completedAt)}
                  </span>
                )}
                <button
                  type="button"
                  className="icon-btn icon-btn--danger"
                  aria-label={`删除子任务「${s.title}」`}
                  title="删除子任务"
                  disabled={adding || pending !== null || editingId !== null}
                  onClick={() => void remove(s)}
                >
                  <Icon name="close" size={14} />
                </button>
              </li>
            )
          })}
        </ul>
      )}

      <div className="subnew">
        <input
          className="input input--compact selectable"
          value={newTitle}
          placeholder="添加子任务，回车确认"
          aria-label="新子任务标题"
          onChange={(e) => setNewTitle(e.target.value)}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || e.keyCode === 229) return
            if (e.key === 'Enter') void add()
          }}
        />
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          disabled={adding || pending !== null || !newTitle.trim()}
          onClick={() => void add()}
        >
          添加
        </button>
      </div>

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button
            type="button"
            className="icon-btn"
            aria-label="关闭"
            onClick={() => setError(null)}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
      {pending && (
        <ScopeDialog
          taskId={taskId}
          taskTitle={taskTitle}
          intent={pending.kind === 'delete' ? 'delete' : 'edit'}
          subtaskOperation={
            {
              create: '添加子任务',
              rename: '重命名子任务',
              delete: '删除子任务',
              copy_previous: '复制子任务',
              reorder: '调整子任务顺序',
              set_completion_mode: '设置子任务完成方式',
            }[pending.kind]
          }
          onCancel={() => {
            setPending(null)
            setEditingId(null)
          }}
          onConfirm={(scope) => {
            if (scope !== 'this_only' && scope !== 'whole_series')
              throw new Error('子任务只支持本次或整个系列')
            return apply(pending, scope)
          }}
        />
      )}
    </div>
  )
}
