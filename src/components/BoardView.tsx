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
 *
 * ## 跨窗口同步（第三轮整改任务书 §12 / §13）
 *
 * 看板自己分页（每页 `PAGE_SIZE` + 「加载更多」），而 OFFSET 分页在数据集变化时
 * 天然不稳定：已经加载了 1..200，另一个窗口删掉第 50 条，再用 OFFSET 200 取下一页
 * 就会**漏掉**原来的第 201 条。所以别的窗口一改数据，看板必须作废在飞请求、
 * 从第一页重取——这与 `App.tsx` 里主列表接 `bus.onTasksChanged` 是同一套做法。
 *
 * "分页 + 代际作废"的逻辑全部放在 `../lib/board-paging` 的无 DOM 状态机里：
 * 本仓库没有 jsdom / testing-library，逻辑留在组件里就等于测不到（§13）。
 */

import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import * as bus from '../lib/bus'
import { PAGE_SIZE } from '../lib/store'
import { createBoardPaging } from '../lib/board-paging'
import { formatTaskTime, isOverdue } from '../lib/datetime'
import type { Task, TaskQuery, TaskStatus } from '../lib/types'
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

/** 看板的查询条件（不显示归档与回收站内容） */
function boardQuery(): TaskQuery {
  return {
    statuses: ['todo', 'doing', 'waiting', 'done'],
    sortBy: 'manual',
  }
}

interface BoardViewProps {
  onEdit: (task: Task) => void
}

export function BoardView({ onEdit }: BoardViewProps) {
  const [dragging, setDragging] = useState<string | null>(null)
  const [dragOverCol, setDragOverCol] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)

  /**
   * 分页状态机（见 `../lib/board-paging`）。
   *
   * 用 `useMemo` 建一次并长期持有：它内部有"代数"，每次渲染都新建实例会把
   * 代数和在飞请求的归属一起换掉，下面的 `useEffect` 也会被反复触发
   * （重复加载、重复注册跨窗口监听）。
   */
  const paging = useMemo(
    () =>
      createBoardPaging({
        listTasks: ipc.listTasks,
        countTasks: ipc.countTasks,
        query: boardQuery,
        pageSize: PAGE_SIZE,
        describeError: errText,
      }),
    [],
  )

  // 状态机是 React 之外的可变对象，用官方订阅接口读它的快照
  const { tasks, total, hasMore, loading, loadingMore, error } = useSyncExternalStore(
    paging.subscribe,
    paging.getState,
  )

  useEffect(() => {
    void paging.reload()

    // 跨窗口同步：别的窗口删/加了任务，就作废在飞的请求、从第一页重取。
    // 与 App.tsx 里主列表的用法一致，只是看板走自己的分页状态机。
    const off = bus.onTasksChanged(() => void paging.handleExternalChange())

    return () => {
      off()
      // 卸载后不再接受任何在飞请求的结果（代数 +1，它们回来时会被丢弃）
      paging.dispose()
    }
  }, [paging])

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
    paging.setError(null)

    // 乐观更新：先动界面让操作跟手，失败再回滚（§3 交互状态要有反馈）
    const prev = tasks
    paging.applyLocalUpdate((cur) => cur.map((t) => (t.id === id ? { ...t, status } : t)))

    try {
      await ipc.updateTask(id, { status })
      await paging.reload()
    } catch (e) {
      paging.applyLocalUpdate(() => prev) // 回滚
      paging.setError(errText(e))
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
          <button
            type="button"
            className="icon-btn"
            aria-label="关闭"
            onClick={() => paging.setError(null)}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      )}

      {/*
        看板同样不得静默只显示一部分（§4.2）。看板会把每张卡片都渲染出来，
        所以这里按页加载，并把"还有多少没显示"写在界面上。
      */}
      {!loading && hasMore && (
        <div className="board__more">
          <span>
            已显示 {tasks.length} / {total} 条。条数很多时建议用列表视图查看全部。
          </span>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={loadingMore}
            onClick={() => void paging.loadMore()}
          >
            {loadingMore ? '正在加载…' : `加载更多（还有 ${total - tasks.length} 条）`}
          </button>
        </div>
      )}

      {/*
        hasMore 为假、但总数又比已加载多：只可能是两次请求之间数据被别处改了。
        这时**不能**渲染「加载更多」——它点了不会有任何反应（第三轮任务书 §6.1）。
        给一个真能用的刷新入口，并如实说明数字对不上。
      */}
      {!loading && !hasMore && total > tasks.length && (
        <div className="board__more">
          <span>
            已显示 {tasks.length} / {total} 条 · 数据在此期间有变化，可刷新查看
          </span>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            onClick={() => void paging.reload()}
          >
            刷新看板
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
