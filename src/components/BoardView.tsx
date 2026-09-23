/**
 * 看板视图（任务书 §4.1：列表/看板视图，拖拽排序和状态变更）。
 *
 * ## 拖拽语义
 *
 * 看板里拖动卡片 = **改状态**（拖到哪一列就是哪个状态），
 * 这与日历里拖动 = 改日期是两件事，界面分别说明，避免混淆。
 *
 * 用 HTML5 原生拖拽而非拖拽库：这里只需要"跨列移动"，
 * 不需要复杂排序动画，原生实现更少依赖、行为更可预期。
 *
 * ## 列的定义
 *
 * 直接对应数据库中的五种状态中的四种（归档不在看板中显示，
 * 它属于"从视野中移除"而不是一个工作阶段）。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import { formatTaskTime, isOverdue } from '../lib/datetime'
import type { Task, TaskStatus } from '../lib/types'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

/** 看板的列（顺序即工作流顺序） */
const COLUMNS: { status: TaskStatus; label: string; hint: string }[] = [
  { status: 'todo', label: '待办', hint: '还没开始' },
  { status: 'doing', label: '进行中', hint: '正在做' },
  { status: 'waiting', label: '等待', hint: '等别人或等条件' },
  { status: 'done', label: '已完成', hint: '完成时间会被真实记录' },
]

interface BoardViewProps {
  onEdit: (task: Task) => void
}

export function BoardView({ onEdit }: BoardViewProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [dragging, setDragging] = useState<string | null>(null)
  const [dragOverCol, setDragOverCol] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      // 看板不显示归档任务；回收站中的也不显示
      const list = await ipc.listTasks({
        statuses: ['todo', 'doing', 'waiting', 'done'],
        sortBy: 'manual',
        limit: 500,
      })
      setTasks(list)
      setError(null)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const byStatus = useMemo(() => {
    const m: Record<string, Task[]> = { todo: [], doing: [], waiting: [], done: [] }
    for (const t of tasks) {
      const arr = m[t.status]
      if (arr) arr.push(t)
    }
    return m
  }, [tasks])

  /** 拖拽落列：改状态 */
  const dropOn = async (status: TaskStatus) => {
    const id = dragging
    setDragOverCol(null)
    setDragging(null)
    if (!id) return

    const task = tasks.find((t) => t.id === id)
    if (!task || task.status === status) return // 原地放下，不产生无意义写入

    setBusy(true)
    setError(null)

    // 乐观更新：先动界面让操作跟手，失败再回滚（§3 交互状态要有反馈）
    const prev = tasks
    setTasks((cur) => cur.map((t) => (t.id === id ? { ...t, status } : t)))

    try {
      await ipc.updateTask(id, { status })
      await reload()
    } catch (e) {
      setTasks(prev) // 回滚
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="board">
      <p className="calhint">
        把卡片拖到另一列即可改变状态。与日历不同——
        <strong>看板拖动改的是状态，不是日期</strong>。
        已完成的任务会记录真实的完成时间。
        {busy && ' 正在保存…'}
      </p>

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}

      {loading ? (
        <div className="skeleton" style={{ height: 300 }} />
      ) : (
        <div className="boardgrid">
          {COLUMNS.map((col) => {
            const list = byStatus[col.status] ?? []
            const isTarget = dragOverCol === col.status
            return (
              <section
                key={col.status}
                className={`boardcol${isTarget ? ' boardcol--drop' : ''}`}
                aria-label={`${col.label}，${list.length} 项`}
                onDragOver={(e) => {
                  if (!dragging) return
                  e.preventDefault()
                  e.dataTransfer.dropEffect = 'move'
                  setDragOverCol(col.status)
                }}
                onDragLeave={() => setDragOverCol((c) => (c === col.status ? null : c))}
                onDrop={(e) => {
                  e.preventDefault()
                  void dropOn(col.status)
                }}
              >
                <header className="boardcol__head">
                  <span className="boardcol__title">{col.label}</span>
                  <span className="boardcol__count">{list.length}</span>
                </header>
                <div className="boardcol__hint">{col.hint}</div>

                {isTarget && (
                  <div className="boardcol__drop-hint">放到「{col.label}」</div>
                )}

                <div className="boardcol__body">
                  {list.length === 0 ? (
                    <div className="boardcol__empty">暂无任务</div>
                  ) : (
                    list.map((t) => {
                      const overdue = isOverdue(t)
                      const timeText = formatTaskTime(t)
                      return (
                        <article
                          key={t.id}
                          className={[
                            'boardcard',
                            t.status === 'done' ? 'boardcard--done' : '',
                            overdue ? 'boardcard--overdue' : '',
                            dragging === t.id ? 'boardcard--dragging' : '',
                          ]
                            .filter(Boolean)
                            .join(' ')}
                          draggable
                          onDragStart={(e) => {
                            setDragging(t.id)
                            e.dataTransfer.setData('text/plain', t.id)
                            e.dataTransfer.effectAllowed = 'move'
                          }}
                          onDragEnd={() => {
                            setDragging(null)
                            setDragOverCol(null)
                          }}
                        >
                          <div className="boardcard__title">{t.title}</div>

                          <div className="boardcard__meta">
                            {t.priority > 0 && (
                              <span
                                className={`prio prio--${t.priority}`}
                                title={`优先级 ${t.priority}`}
                                aria-label={`优先级 ${t.priority}`}
                              />
                            )}
                            {overdue && <span className="badge badge--overdue">已逾期</span>}
                            {t.seriesId && (
                              <span className="badge badge--recurring" title="重复任务的一次发生">
                                <Icon name="repeat" size={14} />
                              </span>
                            )}
                            {timeText && <span>{timeText}</span>}
                          </div>

                          <div className="boardcard__actions">
                            <button
                              type="button"
                              className="icon-btn"
                              title="编辑"
                              aria-label={`编辑「${t.title}」`}
                              onClick={() => onEdit(t)}
                            >
                              <Icon name="edit" size={15} />
                            </button>
                          </div>
                        </article>
                      )
                    })
                  )}
                </div>
              </section>
            )
          })}
        </div>
      )}
    </div>
  )
}
