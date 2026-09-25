/**
 * 日历视图（任务书 §4.3：日、周、月日历；拖拽改期前展示目标日期）。
 *
 * ## 归属规则（界面必须明确说明，§4.2）
 *
 * 任务落在 `COALESCE(计划时间, 截止时间)` 所属的那一天。
 * 两者都没有的任务不会出现在日历里——它们只属于列表视图。
 * 这条规则显示在视图顶部，避免用户疑惑"为什么这个任务不在日历上"。
 *
 * ## 拖拽改期
 *
 * 用 HTML5 原生拖拽而非引入拖拽库：拖拽库的强项是复杂排序，
 * 而这里只需要"拖到某一天"，原生实现代码更少、无额外依赖。
 * 拖拽过程中目标日期格会高亮并显示日期，满足"改期前展示目标日期"。
 *
 * 改期只动**计划时间**，不动截止时间；原有具体时刻会被保留
 * （后端 task_reschedule 负责），这一点也在界面上说明。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import { fromUtcIso, toUtcIso } from '../lib/datetime'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import * as recurrence from '../lib/recurrence-ipc'
import type { Task } from '../lib/types'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

type Mode = 'month' | 'week' | 'day'

/** 周一为一周起点（与中国用户习惯一致） */
const WEEKDAY_NAMES = ['一', '二', '三', '四', '五', '六', '日']

/** 本地日期 → `yyyy-MM-dd` 作为分桶键 */
function dayKey(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

/** 取本地某天的 00:00 */
function startOfLocalDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate())
}

/** 取本地某天的 23:59:59.999 */
function endOfLocalDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59, 999)
}

/** 某月网格的起止（从当月首个周一到末个周日），共 6 周 42 天 */
function monthGrid(anchor: Date): { start: Date; days: Date[] } {
  const first = new Date(anchor.getFullYear(), anchor.getMonth(), 1)
  // getDay(): 0=周日 → 转成周一为 0
  const offset = (first.getDay() + 6) % 7
  const start = new Date(first.getFullYear(), first.getMonth(), first.getDate() - offset)
  const days: Date[] = []
  for (let i = 0; i < 42; i++) {
    days.push(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i))
  }
  return { start, days }
}

/** 某一周的 7 天（周一开头） */
function weekDays(anchor: Date): Date[] {
  const base = new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate())
  const offset = (base.getDay() + 6) % 7
  const monday = new Date(base.getFullYear(), base.getMonth(), base.getDate() - offset)
  return Array.from({ length: 7 }, (_, i) =>
    new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() + i),
  )
}

export function CalendarView() {
  const gate = useMemo(createRequestGate, [])
  const [mode, setMode] = useState<Mode>('month')
  const [anchor, setAnchor] = useState(() => startOfLocalDay(new Date()))
  const [tasks, setTasks] = useState<Task[]>([])
  const [calendarTotal, setCalendarTotal] = useState(0)
  const [truncated, setTruncated] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  /** 拖拽悬停的目标日期，用于高亮与提示 */
  const [dragOver, setDragOver] = useState<string | null>(null)
  /** 正在拖拽的任务 id */
  const [dragging, setDragging] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  /** 当前视图覆盖的日期集合 */
  const days = useMemo(() => {
    if (mode === 'day') return [startOfLocalDay(anchor)]
    if (mode === 'week') return weekDays(anchor)
    return monthGrid(anchor).days
  }, [mode, anchor])

  const range = useMemo(() => {
    const first = days[0] ?? anchor
    const last = days[days.length - 1] ?? anchor
    return { start: toUtcIso(startOfLocalDay(first)), end: toUtcIso(endOfLocalDay(last)) }
  }, [days, anchor])

  const reload = useCallback(async () => {
    setLoading(true)
    await runLatestRequest(gate, async () => {
      await recurrence.recurringEnsureRange(range.start, range.end)
      return ipc.tasksInRange(range.start, range.end)
    }, { apply: (page) => {
      setTasks(page.rows)
      setCalendarTotal(page.total)
      setTruncated(page.truncated)
      setError(null)
    }, reject: (e) => setError(errText(e)), finish: () => setLoading(false) })
  }, [gate, range.start, range.end])

  useEffect(() => {
    void reload()
    const off = onDataChanged(['tasks', 'all'], () => void reload())
    return () => {
      off()
      gate.invalidate()
    }
  }, [gate, reload])

  useEffect(() => () => gate.dispose(), [gate])

  /** 按本地日期分桶 */
  const buckets = useMemo(() => {
    const m = new Map<string, Task[]>()
    for (const t of tasks) {
      const at = fromUtcIso(t.plannedAt) ?? fromUtcIso(t.dueAt)
      if (!at) continue
      const key = dayKey(at)
      const arr = m.get(key)
      if (arr) arr.push(t)
      else m.set(key, [t])
    }
    return m
  }, [tasks])

  const todayKey = dayKey(new Date())

  /** 导航：上一页 / 下一页 */
  const shift = (dir: -1 | 1) => {
    setAnchor((a) => {
      if (mode === 'day') return new Date(a.getFullYear(), a.getMonth(), a.getDate() + dir)
      if (mode === 'week') return new Date(a.getFullYear(), a.getMonth(), a.getDate() + dir * 7)
      return new Date(a.getFullYear(), a.getMonth() + dir, 1)
    })
  }

  const headerLabel = useMemo(() => {
    if (mode === 'month') return `${anchor.getFullYear()} 年 ${anchor.getMonth() + 1} 月`
    if (mode === 'week') {
      const d = weekDays(anchor)
      const f = d[0]!
      const l = d[6]!
      const sameMonth = f.getMonth() === l.getMonth()
      return sameMonth
        ? `${f.getFullYear()} 年 ${f.getMonth() + 1} 月 ${f.getDate()}–${l.getDate()} 日`
        : `${f.getMonth() + 1} 月 ${f.getDate()} 日 – ${l.getMonth() + 1} 月 ${l.getDate()} 日`
    }
    return `${anchor.getFullYear()} 年 ${anchor.getMonth() + 1} 月 ${anchor.getDate()} 日（周${WEEKDAY_NAMES[(anchor.getDay() + 6) % 7]}）`
  }, [mode, anchor])

  /** 执行拖拽改期 */
  const dropOn = async (target: Date, droppedId: string) => {
    const id = droppedId || dragging
    setDragOver(null)
    setDragging(null)
    if (!id) return

    const task = tasks.find((t) => t.id === id)
    if (!task) return

    const oldAt = fromUtcIso(task.plannedAt) ?? fromUtcIso(task.dueAt)
    if (oldAt && dayKey(oldAt) === dayKey(target)) return // 拖回原地，不做无意义写入

    setBusy(true)
    setError(null)
    try {
      // 目标日期的本地零点转成 UTC 传给后端；后端会保留原任务的时刻
      await ipc.rescheduleTask(id, toUtcIso(startOfLocalDay(target)))
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  /** 单元格内单个任务的渲染 */
  const renderChip = (t: Task) => {
    const at = fromUtcIso(t.plannedAt) ?? fromUtcIso(t.dueAt)
    const done = t.status === 'done'
    const overdue = t.dueAt && !done && fromUtcIso(t.dueAt)!.getTime() < Date.now()
    return (
      <div
        key={t.id}
        className={[
          'calchip',
          done ? 'calchip--done' : '',
          overdue ? 'calchip--overdue' : '',
          (t.priority ?? 0) >= 3 ? 'calchip--high' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        draggable
        onDragStart={(e) => {
          setDragging(t.id)
          // 必须设置 dataTransfer，否则 Firefox 等不会触发 drop
          e.dataTransfer.setData('text/plain', t.id)
          e.dataTransfer.effectAllowed = 'move'
        }}
        onDragEnd={() => {
          setDragging(null)
          setDragOver(null)
        }}
        title={[
          t.title,
          t.plannedAt ? `计划：${at?.toLocaleString('zh-CN', { hour12: false })}` : '',
          t.dueAt ? `截止：${fromUtcIso(t.dueAt)?.toLocaleString('zh-CN', { hour12: false })}` : '',
          t.seriesId ? '重复任务的一次发生（改期只影响这一次）' : '',
        ]
          .filter(Boolean)
          .join('\n')}
      >
        {t.hasPlannedTime === 1 && at && (
          <span className="calchip__time">{at.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false })}</span>
        )}
        <span className="calchip__title">{t.title}</span>
        {t.seriesId && (
          <span className="calchip__mark" aria-label="重复任务">
            <Icon name="repeat" size={13} />
          </span>
        )}
      </div>
    )
  }

  /** 单元格：月视图与周/日视图共用 */
  const renderCell = (d: Date, opts: { compact: boolean }) => {
    const key = dayKey(d)
    const list = buckets.get(key) ?? []
    const isToday = key === todayKey
    const isDragTarget = dragOver === key
    const inCurrentMonth = mode !== 'month' || d.getMonth() === anchor.getMonth()

    return (
      <div
        key={key}
        className={[
          'calcell',
          opts.compact ? 'calcell--compact' : 'calcell--tall',
          isToday ? 'calcell--today' : '',
          !inCurrentMonth ? 'calcell--muted' : '',
          isDragTarget ? 'calcell--drop' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        onDragOver={(e) => {
          // React may not have committed the drag-start state before the
          // browser's first dragover. The transfer payload is the stable ID.
          if (!dragging && !e.dataTransfer.types.includes('text/plain')) return
          // preventDefault 是允许 drop 的前提
          e.preventDefault()
          e.dataTransfer.dropEffect = 'move'
          setDragOver(key)
        }}
        onDrop={(e) => {
          e.preventDefault()
          void dropOn(d, e.dataTransfer.getData('text/plain'))
        }}
        onClick={() => {
          // 点空白处切换到该日视图，方便聚焦某一天
          if (mode === 'month') {
            setAnchor(d)
            setMode('day')
          }
        }}
        role={mode === 'month' ? 'gridcell' : undefined}
        aria-label={`${d.getMonth() + 1} 月 ${d.getDate()} 日，${list.length} 项任务`}
      >
        <div className="calcell__head">
          <span className="calcell__day">{d.getDate()}</span>
          {mode !== 'day' && (
            <span className="calcell__wd">周{WEEKDAY_NAMES[(d.getDay() + 6) % 7]}</span>
          )}
          {list.length > 0 && <span className="calcell__count">{list.length}</span>}
        </div>

        {/* 拖拽提示：明确告知会落到哪一天（§4.3「拖拽改期前展示目标日期」） */}
        {isDragTarget && (
          <div className="calcell__drop-hint">
            移动到 {d.getMonth() + 1} 月 {d.getDate()} 日
          </div>
        )}

        <div className="calcell__body">{list.map(renderChip)}</div>
      </div>
    )
  }

  return (
    <div className="calendar">
      <div className="calbar">
        <div className="segmented" role="radiogroup" aria-label="日历视图">
          {(
            [
              ['day', '日'],
              ['week', '周'],
              ['month', '月'],
            ] as const
          ).map(([v, label]) => (
            <button
              key={v}
              type="button"
              role="radio"
              aria-checked={mode === v}
              className={`segmented__item${mode === v ? ' segmented__item--on' : ''}`}
              onClick={() => setMode(v)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="calbar__nav">
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            aria-label="上一页"
            onClick={() => shift(-1)}
          >
            ‹
          </button>
          <span className="calbar__label">{headerLabel}</span>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            aria-label="下一页"
            onClick={() => shift(1)}
          >
            ›
          </button>
        </div>

        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={() => setAnchor(startOfLocalDay(new Date()))}
        >
          回到今天
        </button>
      </div>

      <p className="calhint">
        任务显示在「计划时间」所属的日期；未填计划时间的显示在「截止时间」那天。
        两者都没填的任务不会出现在日历里。拖动任务到另一天即可改期——
        <strong>只改计划时间，不影响截止时间</strong>，原有的具体时刻会被保留。
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
      {truncated && (
        <div className="alert" role="status">
          当前范围有 {calendarTotal} 项任务，日历仅显示前 {tasks.length} 项。
          请缩小日期范围，或在列表视图查看全部。
        </div>
      )}

      {loading ? (
        <div className="skeleton" style={{ height: 320 }} />
      ) : mode === 'month' ? (
        <>
          <div className="calweekhead" aria-hidden="true">
            {WEEKDAY_NAMES.map((w) => (
              <div key={w}>周{w}</div>
            ))}
          </div>
          <div className="calgrid calgrid--month" role="grid" aria-label="月历">
            {days.map((d) => renderCell(d, { compact: true }))}
          </div>
        </>
      ) : mode === 'week' ? (
        <div className="calgrid calgrid--week">
          {days.map((d) => renderCell(d, { compact: false }))}
        </div>
      ) : (
        <div className="calgrid calgrid--day">{days.map((d) => renderCell(d, { compact: false }))}</div>
      )}
    </div>
  )
}
