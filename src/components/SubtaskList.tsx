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
import type { Subtask } from '../lib/organize-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface SubtaskListProps {
  taskId: string
}

export function SubtaskList({ taskId }: SubtaskListProps) {
  const [items, setItems] = useState<Subtask[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [newTitle, setNewTitle] = useState('')
  const [adding, setAdding] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')

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
  }, [reload])

  const add = async () => {
    const t = newTitle.trim()
    if (!t) return
    setAdding(true)
    setError(null)
    try {
      await org.subtaskCreate(taskId, t)
      setNewTitle('')
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setAdding(false)
    }
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
    setEditingId(null)
    if (!t || t === s.title) return
    try {
      await org.subtaskUpdate(s.id, { title: t })
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  const remove = async (s: Subtask) => {
    try {
      await org.subtaskDelete(s.id)
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  const done = items.filter((s) => s.isDone === 1).length
  const percent = items.length > 0 ? Math.round((done * 100) / items.length) : null

  return (
    <div className="subtasks">
      <div className="subtasks__head">
        <span>子任务</span>
        {percent !== null && (
          <>
            <span className="progress">
              <span className="progress__bar" style={{ width: `${percent}%` }} />
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
        <p className="reminders__empty">还没有子任务。把这件事拆成几步会更清楚。</p>
      ) : (
        <ul className="sublist">
          {items.map((s) => (
            <li key={s.id} className={`subtask${s.isDone === 1 ? ' subtask--done' : ''}`}>
              <button
                type="button"
                role="checkbox"
                aria-checked={s.isDone === 1}
                aria-label={
                  s.isDone === 1 ? `将子任务「${s.title}」标记为未完成` : `完成子任务「${s.title}」`
                }
                className="task__check task__check--sm"
                onClick={() => void toggle(s)}
              >
                {s.isDone === 1 && (
                  <svg width="9" height="9" viewBox="0 0 12 12" aria-hidden="true">
                    <path
                      d="M2.5 6.2l2.3 2.3L9.5 3.8"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
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
                  className="subtask__title"
                  onDoubleClick={() => {
                    setEditingId(s.id)
                    setEditTitle(s.title)
                  }}
                  title="双击可重命名"
                >
                  {s.title}
                </span>
              )}

              <button
                type="button"
                className="icon-btn icon-btn--danger"
                aria-label={`删除子任务「${s.title}」`}
                title="删除子任务"
                onClick={() => void remove(s)}
              >
                ✕
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
          disabled={adding || !newTitle.trim()}
          onClick={() => void add()}
        >
          添加
        </button>
      </div>

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      )}
    </div>
  )
}
