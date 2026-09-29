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
import type { Subtask } from '../lib/organize-ipc'
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

export function SubtaskList({ taskId, isRecurring = false, taskTitle = '' }: SubtaskListProps) {
  const [items, setItems] = useState<Subtask[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [newTitle, setNewTitle] = useState('')
  const [adding, setAdding] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [pending, setPending] = useState<org.SubtaskAction | null>(null)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      setItems(await org.subtaskList(taskId))
      setError(null)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
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
      if (action.kind === 'create') setNewTitle('')
      setEditingId(null)
      setPending(null)
    } finally {
      setAdding(false)
    }
  }
  const request = async (action: org.SubtaskAction) => {
    if (adding || pending) return
    if (isRecurring) { setPending(action); return }
    try { await apply(action) } catch (e) { setError(errText(e)) }
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
    if (!t || t === s.title) { setEditingId(null); return }
    await request({ kind: 'rename', id: s.id, title: t })
  }

  const remove = async (s: Subtask) => {
    await request({ kind: 'delete', id: s.id })
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

      {loading ? (
        <div className="skeleton" style={{ height: 26 }} />
      ) : items.length === 0 ? (
        <p className="reminders__empty">
          还没有子任务。把这件事拆成几步会更清楚。
          {isRecurring && <button type="button" className="btn btn--quiet btn--sm"
            disabled={adding || pending !== null} onClick={() => void request({ kind: 'copy_previous' })}>
            从上一次复制子任务
          </button>}
        </p>
      ) : (
        <ul className="sublist">
          {items.map((s) => (
            <li
              key={s.id}
              className={`subtask${s.isDone === 1 ? ' subtask--done' : ''}`}
            >
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
                    if (e.key === 'Enter') void saveRename(s)
                    if (e.key === 'Escape') setEditingId(null)
                  }}
                  onBlur={() => void saveRename(s)}
                />
              ) : (
                <span
                  className="subtask__title selectable"
                  onDoubleClick={() => {
                    setEditingId(s.id)
                    setEditTitle(s.title)
                  }}
                  title="双击可重命名"
                >
                  {s.title}
                </span>
              )}

              {s.isDone === 1 && <span className="subtask__completed selectable">{formatCompletionTime(s.completedAt)}</span>}
              <button
                type="button"
                className="icon-btn icon-btn--danger"
                aria-label={`删除子任务「${s.title}」`}
                title="删除子任务"
                onClick={() => void remove(s)}
              >
                <Icon name="close" size={14} />
              </button>
            </li>
          ))}
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
      {pending && <ScopeDialog taskId={taskId} taskTitle={taskTitle} intent={pending.kind === 'delete' ? 'delete' : 'edit'}
        subtaskOperation={{ create: '添加子任务', rename: '重命名子任务', delete: '删除子任务', copy_previous: '复制子任务' }[pending.kind]}
        onCancel={() => { setPending(null); setEditingId(null) }}
        onConfirm={(scope) => {
          if (scope !== 'this_only' && scope !== 'whole_series') throw new Error('子任务只支持本次或整个系列')
          return apply(pending, scope)
        }} />}
    </div>
  )
}
