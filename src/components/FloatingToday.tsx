/**
 * 悬浮今日小窗（任务书 §8.4）。
 *
 * ## 为什么它必须是独立窗口而不是主窗口的一部分
 *
 * 它是一个"桌面组件"：无边框、可置顶、可半透明、可穿透。
 * 若塞进主窗口，这些窗口级能力会同时作用于主窗口，用户就无法
 * 一边让主窗口正常显示在任务栏、一边让今日清单浮在桌面角落。
 *
 * ## 交互设计上的两个坑（都踩过并修掉了）
 *
 * 1. **拖动区域不能盖住按钮**。曾经在整窗 `mousedown` 里调
 *    `startDragging()`，于是顶栏上的置顶/穿透/关闭按钮一按下去就变成
 *    "拖窗口"，`click` 事件永远不触发——表现为"按钮点了没反应"。
 *    现在只有从 `[data-drag-region]` 及其**非交互子元素**上起拖才移动窗口。
 * 2. **列表文字要能选中、输入框要能点**。所以拖动手柄只放在顶栏与底栏，
 *    列表区域完全不参与拖动。
 *
 * ## 就地编辑与完整编辑
 *
 * - 双击任务标题 → 就地改名（最快路径，不打断浏览）；
 * - 点 ✎ → 打开**与主窗口完全同一个** `TaskEditor`（描述、备注、链接、
 *   优先级、项目/分类、计划与截止时间、子任务、附件、提醒、依赖），
 *   因此"悬浮窗里能做的事"和主窗口一致，而不是只能加一条任务。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import * as win from '../lib/window-ipc'
import * as ipc from '../lib/ipc'
import { subtaskProgressBatch } from '../lib/organize-ipc'
import type { Subtask } from '../lib/organize-ipc'
import { SubtaskPreview } from './SubtaskPreview'
import { onDataChanged } from '../lib/data-change'
import { listenEvent } from '../lib/event-listener'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import { fromUtcIso, isOverdue, todayRange } from '../lib/datetime'
import { TaskEditor } from './TaskEditor'
import type { FloatingState } from '../lib/window-ipc'
import type { Task, TaskQuery } from '../lib/types'
import { Icon } from './Icons'
import { CheckMark } from './CheckMark'
import { addDays, format, startOfWeek } from 'date-fns'
import { QuickAdd } from './QuickAdd'
import { FocusPanel } from './FocusPanel'
import { ReminderEditor } from './ReminderEditor'
import { AiAssistant } from './AiAssistant'

/** 拖动结束后再落库的延迟：拖动过程中会连续触发 resize 事件 */
const SIZE_SAVE_DELAY = 400
/** 不透明度落库节流：滑块拖动时 input 事件非常密集 */
const OPACITY_SAVE_DELAY = 250

export function FloatingToday() {
  const [selectedDate, setSelectedDate] = useState(() =>
    format(new Date(), 'yyyy-MM-dd'),
  )
  const [adding, setAdding] = useState(false)
  const [panel, setPanel] = useState<'focus' | 'reminder' | 'ai' | null>(null)
  const [reminderTask, setReminderTask] = useState<Task | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [hideDone, setHideDone] = useState(false)
  const [weekCounts, setWeekCounts] = useState<number[]>([])
  const selectedDay = useMemo(
    () => new Date(`${selectedDate}T00:00:00`),
    [selectedDate],
  )
  const weekStart = useMemo(
    () => startOfWeek(selectedDay, { weekStartsOn: 1 }),
    [selectedDay],
  )
  const weekDays = useMemo(
    () => Array.from({ length: 7 }, (_, i) => addDays(weekStart, i)),
    [weekStart],
  )
  const gate = useMemo(createRequestGate, [])
  const [tasks, setTasks] = useState<Task[]>([])
  const [subtasks, setSubtasks] = useState<Record<string, Subtask[]>>({})
  const [total, setTotal] = useState(0)
  const [doneTotal, setDoneTotal] = useState(0)
  const [state, setState] = useState<FloatingState | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  /** 正在就地改名的任务 id */
  const [editingId, setEditingId] = useState<string | null>(null)
  /** 就地编辑的草稿 */
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  /** 正在用完整表单编辑的任务（null 表示未打开） */
  const [fullEditing, setFullEditing] = useState<Task | null>(null)
  /** 本地不透明度：拖动时即时生效，不等后端往返 */
  const [opacity, setOpacity] = useState(1)

  const sizeTimer = useRef<number | undefined>(undefined)
  const opacityTimer = useRef<number | undefined>(undefined)

  const reload = useCallback(async () => {
    await runLatestRequest(
      gate,
      async () => {
        const r = todayRange(selectedDay)
        const query: TaskQuery = {
          statuses: ['todo', 'doing', 'waiting', 'done'],
          plannedFrom: r.start,
          plannedTo: r.end,
        }
        const [list, allCount, doneCount, st, counts] = await Promise.all([
          ipc.listTasks({
            ...query,
            sortBy: 'manual',
            limit: 100,
          }),
          ipc.countTasks(query),
          ipc.countTasks({ ...query, statuses: ['done'] }),
          win.windowFloatingState(),
          Promise.all(
            weekDays.map((day) => {
              const range = todayRange(day)
              return ipc.countTasks({
                statuses: ['todo', 'doing', 'waiting'],
                plannedFrom: range.start,
                plannedTo: range.end,
              })
            }),
          ),
        ])
        const summaries = await subtaskProgressBatch(
          list.map((task) => task.id),
        )
        return { list, allCount, doneCount, st, summaries, counts }
      },
      {
        apply: ({ list, allCount, doneCount, st, summaries, counts }) => {
          setWeekCounts(counts.map((c) => c.total))
          setTasks(list)
          setSubtasks(
            Object.fromEntries(summaries.map((p) => [p.taskId, p.items ?? []])),
          )
          setTotal(allCount.total)
          setDoneTotal(doneCount.total)
          setState(st)
          setOpacity(st.opacity)
          document.documentElement.style.setProperty(
            '--floating-opacity',
            String(st.opacity),
          )
          setError(null)
        },
        reject: (e) => setError(e instanceof Error ? e.message : String(e)),
        finish: () => setLoading(false),
      },
    )
  }, [gate, selectedDay, weekDays])

  useEffect(() => {
    gate.activate()
    setLoading(true)
    void reload()
    // 定时刷新作为兜底（例如主窗口没开、事件丢了）
    const t = window.setInterval(() => void reload(), 60_000)
    // 主窗口里的增删改会广播 tasks-changed，收到就立刻刷新，
    // 不必等下一次定时轮询——"改了没反应"最容易被当成没保存。
    const off = onDataChanged(['tasks', 'all'], () => void reload())
    return () => {
      window.clearInterval(t)
      off()
      gate.dispose()
    }
  }, [gate, reload])

  /** 监听后端广播的配置变化，实时更新不透明度与穿透提示 */
  useEffect(() => {
    return listenEvent<win.WindowConfig>('floating-config', (e) => {
      const cfg = e.payload
      setState((s) => s ? {
        ...s,
        clickThrough: cfg.floatingClickThrough,
        alwaysOnTop: cfg.floatingAlwaysOnTop,
        opacity: cfg.floatingOpacity,
        enabled: cfg.floatingEnabled,
        width: cfg.floatingWidth,
        height: cfg.floatingHeight,
      } : s)
      // 不透明度用 CSS 应用，保证文字对比度（§8.3）。
      setOpacity(cfg.floatingOpacity)
      document.documentElement.style.setProperty('--floating-opacity', String(cfg.floatingOpacity))
    }, (e) => {
        // 事件监听不可用时不影响主流程，但要说明原因，避免"改了不生效"却查不到
        setError(
          `悬浮窗实时同步不可用：${e instanceof Error ? e.message : String(e)}`,
        )
    })
  }, [])

  /**
   * 窗口尺寸变化 → 防抖落库。
   *
   * 这是"大小调节不出显示 bug"的关键：拖动过程中只让系统改窗口，
   * 停下来之后才写一次配置；否则每个 resize 事件都写库 + 反设尺寸，
   * 会出现窗口抖动、拖不动、甚至尺寸被反复覆盖的观感问题。
   */
  useEffect(() => {
    let unlisten: (() => void) | undefined
    let disposed = false
    void (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const w = getCurrentWindow()
        const off = await w.onResized(({ payload }) => {
          if (sizeTimer.current) window.clearTimeout(sizeTimer.current)
          sizeTimer.current = window.setTimeout(() => {
            void (async () => {
              try {
                const scale = await w.scaleFactor()
                // 物理像素 → 逻辑像素，与配置的语义保持一致
                const width = payload.width / scale
                const height = payload.height / scale
                const applied = await win.windowSetFloatingSize(width, height)
                setState((s) =>
                  s
                    ? { ...s, width: applied.width, height: applied.height }
                    : s,
                )
              } catch (e) {
                setError(e instanceof Error ? e.message : String(e))
              }
            })()
          }, SIZE_SAVE_DELAY)
        })
        if (disposed) off()
        else unlisten = off
      } catch {
        // 拿不到窗口对象（浏览器预览）时忽略：尺寸记忆是增强项
      }
    })()
    return () => {
      disposed = true
      unlisten?.()
      if (sizeTimer.current) window.clearTimeout(sizeTimer.current)
    }
  }, [])

  const toggle = async (id: string, done: boolean) => {
    try {
      await ipc.toggleTaskDone(id, done)
      await reload()
      // 让主窗口也立刻看到这次勾选
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  /** 就地改名：Enter 保存，Esc 取消，失焦保存 */
  const saveTitle = async (id: string) => {
    const next = draft.trim()
    setEditingId(null)
    const cur = tasks.find((t) => t.id === id)
    if (!cur || next === '' || next === cur.title) {
      return
    }
    setBusy(true)
    try {
      await ipc.updateTask(id, { title: next })
      await reload()
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const open = total - doneTotal
  const clickThrough = state?.clickThrough ?? false

  /**
   * 拖动窗口：无边框窗口需要自己实现拖动。
   *
   * 只允许从 `[data-drag-region]` 上的**非交互元素**起拖。
   * 这里必须显式排除 button / input / select / textarea / [role=button]：
   * 顶栏既是拖动区又放着按钮，不排除的话按钮永远点不动。
   */
  const onMouseDown = async (e: React.MouseEvent) => {
    if (clickThrough) return
    if (e.button !== 0) return
    const target = e.target as HTMLElement
    if (!target.closest('[data-drag-region]')) return
    if (
      target.closest(
        'button, input, select, textarea, a, [role="button"], .selectable, [data-no-drag]',
      )
    ) {
      return
    }
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startDragging()
    } catch {
      // 拖动失败不影响功能
    }
  }

  /** 立即切换窗口级开关（不等设置页） */
  const act = async (action: win.WindowAction) => {
    try {
      const cfg = await win.windowApplyAction(action)
      setState((s) =>
        s
          ? {
              ...s,
              clickThrough: cfg.floatingClickThrough,
              alwaysOnTop: cfg.floatingAlwaysOnTop,
              enabled: cfg.floatingEnabled,
            }
          : s,
      )
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  /** 拖动右下角把手：交给系统做真实的无边框缩放 */
  const onResizeStart = async (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startResizeDragging('SouthEast')
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  /** 滑块拖动：本地立即生效（CSS 变量），停止拖动后再落库 */
  const onOpacityInput = (v: number) => {
    setOpacity(v)
    document.documentElement.style.setProperty('--floating-opacity', String(v))
    if (opacityTimer.current) window.clearTimeout(opacityTimer.current)
    opacityTimer.current = window.setTimeout(() => {
      void (async () => {
        try {
          const applied = await win.windowSetFloatingOpacity(v)
          setOpacity(applied)
        } catch (e) {
          setError(e instanceof Error ? e.message : String(e))
        }
      })()
    }, OPACITY_SAVE_DELAY)
  }

  return (
    <div
      className="floating"
      style={{ opacity }}
      onMouseDown={(e) => void onMouseDown(e)}
    >
      <header className="floating__head" data-drag-region>
        <span className="floating__title">Lumen 待办</span>
        <span className="floating__count">
          {open > 0 ? `${open} 项待办` : total > 0 ? '全部完成 ✓' : '暂无安排'}
        </span>
        {(
          <span className="floating__tools" data-no-drag>
            <button
              type="button"
              className="floating__btn"
              aria-label="AI 助手"
              title="AI 助手"
              onClick={() => setPanel(panel === 'ai' ? null : 'ai')}
            >
              <Icon name="star" size={15} />
            </button>
            <button
              type="button"
              className="floating__btn"
              aria-label="打开主窗口"
              title="打开主窗口"
              onClick={() => void act('show_main')}
            >
              <Icon name="board" size={15} />
            </button>
            <button
              type="button"
              className={`floating__btn${state?.alwaysOnTop ? ' floating__btn--on' : ''}`}
              aria-pressed={state?.alwaysOnTop ?? false}
              title={
                state?.alwaysOnTop
                  ? '取消置顶（会被其它窗口盖住）'
                  : '置顶显示（始终浮在最前）'
              }
              aria-label={state?.alwaysOnTop ? '取消置顶' : '置顶显示'}
              onClick={() => void act('toggle_floating_top')}
            >
              <Icon name="pin" size={15} />
            </button>
            <button
              type="button"
              className={`floating__btn${clickThrough ? ' floating__btn--on' : ''}`}
              aria-pressed={clickThrough}
              disabled={clickThrough}
              title={clickThrough ? '鼠标穿透已开启；可从托盘或设置关闭，重新呼出也会关闭穿透' : '开启鼠标穿透（开启后需从托盘或设置关闭）'}
              aria-label={clickThrough ? '鼠标穿透已开启' : '开启鼠标穿透'}
              onClick={() => void act('toggle_floating_click_through')}
            >
              <Icon name="ban" size={15} />
            </button>
            <button
              type="button"
              className="floating__btn"
              title="隐藏悬浮窗（可在托盘或设置中重新打开）"
              aria-label="隐藏悬浮窗"
              onClick={() => void act('hide_floating')}
            >
              <Icon name="close" size={14} />
            </button>
          </span>
        )}
      </header>

      {!clickThrough && (
        <>
          <div className="floating__actions">
            <button
              type="button"
              aria-expanded={panel === 'focus'}
              onClick={() => setPanel(panel === 'focus' ? null : 'focus')}
            >
              <Icon name="play" size={16} />
              开始专注
            </button>
            <button
              type="button"
              aria-expanded={panel === 'reminder'}
              onClick={() => setPanel(panel === 'reminder' ? null : 'reminder')}
            >
              <Icon name="clock" size={16} />
              添加提醒
            </button>
          </div>
          <nav className="floating__week" aria-label="一周日期">
            <button
              type="button"
              className="floating__week-arrow"
              aria-label="上一周"
              onClick={() =>
                setSelectedDate(format(addDays(selectedDay, -7), 'yyyy-MM-dd'))
              }
            >
              ‹
            </button>
            {weekDays.map((day, i) => {
              const date = format(day, 'yyyy-MM-dd')
              return (
                <button
                  key={date}
                  type="button"
                  className={`floating__day${selectedDate === date ? ' floating__day--selected' : ''}`}
                  aria-pressed={selectedDate === date}
                  aria-label={`${date}${weekCounts[i] ? `，${weekCounts[i]}项待办` : ''}`}
                  onClick={() => {
                    setSelectedDate(date)
                    setNotice(null)
                  }}
                >
                  <span>
                    {
                      ['周一', '周二', '周三', '周四', '周五', '周六', '周日'][
                        i
                      ]
                    }
                  </span>
                  <strong>{format(day, 'MM-dd')}</strong>
                  {!!weekCounts[i] && (
                    <i className="floating__day-dot" aria-hidden="true" />
                  )}
                </button>
              )
            })}
            <button
              type="button"
              className="floating__week-arrow"
              aria-label="下一周"
              onClick={() =>
                setSelectedDate(format(addDays(selectedDay, 7), 'yyyy-MM-dd'))
              }
            >
              ›
            </button>
          </nav>
        </>
      )}

      {!clickThrough && panel && (
        <section
          className="floating__panel"
          aria-label={
            panel === 'focus'
              ? '专注面板'
              : panel === 'ai'
                ? 'AI 助手面板'
                : '提醒面板'
          }
        >
          <button
            type="button"
            className="floating__panel-close icon-btn"
            aria-label="收起面板"
            onClick={() => setPanel(null)}
          >
            <Icon name="close" size={14} />
          </button>
          {panel === 'focus' && <FocusPanel compact />}
          {panel === 'ai' && <AiAssistant compact />}
          {panel === 'reminder' && (
            <>
              <label className="formrow">
                <span className="formlabel">给哪条任务提醒</span>
                <select
                  className="input"
                  aria-label="选择提醒任务"
                  value={reminderTask?.id ?? ''}
                  onChange={(e) =>
                    setReminderTask(
                      tasks.find((t) => t.id === e.target.value) ?? null,
                    )
                  }
                >
                  <option value="">选择当天任务</option>
                  {tasks
                    .filter((t) => t.status !== 'done')
                    .map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.title}
                      </option>
                    ))}
                </select>
              </label>
              {reminderTask ? (
                <ReminderEditor
                  key={reminderTask.id}
                  taskId={reminderTask.id}
                  hasPlanned={!!reminderTask.plannedAt}
                  hasDue={!!reminderTask.dueAt}
                  taskDone={reminderTask.status === 'done'}
                />
              ) : (
                <p className="setgroup__hint">
                  先选择任务；还没有任务时，点击下方 ＋ 新建。
                </p>
              )}
            </>
          )}
        </section>
      )}

      {clickThrough && (
        <div className="floating__through" role="note">
          鼠标穿透已开启，本窗口不可点击。
          <br />
          关闭方式：托盘菜单「悬浮窗鼠标穿透」，或打开主窗口到设置中关闭。
        </div>
      )}

      {error && (
        <div className="floating__error" role="alert">
          {error}
        </div>
      )}

      {total > tasks.length && (
        <div className="floating__hint">
          仅显示前 {tasks.length} 项，共 {total} 项。
          <button type="button" onClick={() => void act('show_main')}>
            在主窗口查看全部
          </button>
        </div>
      )}

      <div className="floating__workspace">
        <div className="floating__list-head">
          <span>
            {selectedDate === format(new Date(), 'yyyy-MM-dd')
              ? '今天'
              : format(selectedDay, 'M月d日')}{' '}
            · {open} 项待办
          </span>
          <button
            type="button"
            onClick={() => setSelectedDate(format(new Date(), 'yyyy-MM-dd'))}
          >
            回到今天
          </button>
        </div>
        {notice && (
          <div className="floating__hint" role="status">
            {notice}
          </div>
        )}
        {loading ? (
          <div className="floating__empty">载入中…</div>
        ) : tasks.length === 0 ? (
          <div className="floating__empty">
            这一天还没有安排。
            <br />
            <span className="floating__hint">
              点击下方 ＋，直接填写任务并选择选项。
            </span>
          </div>
        ) : (
          <ul className="floating__list">
            {tasks
              .filter((t) => !hideDone || t.status !== 'done')
              .map((t) => {
                const isDone = t.status === 'done'
                const overdue = isOverdue(t)
                const at = fromUtcIso(t.plannedAt)
                const editing = editingId === t.id
                return (
                  <li
                    key={t.id}
                    className={`floating__item${isDone ? ' floating__item--done' : ''}${
                      overdue ? ' floating__item--overdue' : ''
                    }`}
                  >
                    <button
                      type="button"
                      role="checkbox"
                      aria-checked={isDone}
                      aria-label={
                        isDone
                          ? `将「${t.title}」标记为未完成`
                          : `完成「${t.title}」`
                      }
                      className="floating__check"
                      disabled={clickThrough}
                      onClick={() => void toggle(t.id, !isDone)}
                    >
                      {isDone && <CheckMark size={11} />}
                    </button>
                    {(subtasks[t.id]?.length ?? 0) > 0 && (
                      <span
                        className="floating__progress"
                        aria-label="子任务进度"
                      >
                        {
                          (subtasks[t.id] ?? []).filter((s) => s.isDone === 1)
                            .length
                        }
                        /{subtasks[t.id]?.length}
                      </span>
                    )}

                    {editing ? (
                      <input
                        className="floating__edit selectable"
                        value={draft}
                        autoFocus
                        aria-label={`修改「${t.title}」的标题`}
                        disabled={busy}
                        onChange={(e) => setDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') {
                            e.preventDefault()
                            void saveTitle(t.id)
                          } else if (e.key === 'Escape') {
                            e.preventDefault()
                            setEditingId(null)
                          }
                        }}
                        onBlur={() => void saveTitle(t.id)}
                      />
                    ) : (
                      <span
                        className="floating__text selectable"
                        title={`${t.title}\n双击可直接改名，点右侧 ✎ 打开完整编辑`}
                        role="button"
                        tabIndex={0}
                        onDoubleClick={() => {
                          if (clickThrough) return
                          setDraft(t.title)
                          setEditingId(t.id)
                        }}
                        onKeyDown={(e) => {
                          if (e.key === 'F2' || e.key === 'Enter') {
                            e.preventDefault()
                            setDraft(t.title)
                            setEditingId(t.id)
                          }
                        }}
                      >
                        {t.title}
                      </span>
                    )}

                    {t.hasPlannedTime === 1 && at && (
                      <span className="floating__time">
                        {at.toLocaleTimeString('zh-CN', {
                          hour: '2-digit',
                          minute: '2-digit',
                          hour12: false,
                        })}
                      </span>
                    )}
                    {t.seriesId && (
                      <span
                        className="floating__mark"
                        title="重复任务的一次发生"
                      >
                        <Icon name="repeat" size={13} />
                      </span>
                    )}

                    {/* 完整编辑：打开与主窗口同一个表单，而不是只能改标题 */}
                    <button
                      type="button"
                      className="floating__item-btn"
                      title="打开完整编辑（描述、时间、子任务、附件、提醒…）"
                      aria-label={`编辑「${t.title}」`}
                      disabled={clickThrough}
                      onClick={() => setFullEditing(t)}
                    >
                      <Icon name="edit" size={15} />
                    </button>
                    {(subtasks[t.id]?.length ?? 0) > 0 && (
                      <SubtaskPreview
                        items={subtasks[t.id] ?? []}
                        disabled={clickThrough}
                      />
                    )}
                  </li>
                )
              })}
          </ul>
        )}

        {!clickThrough && (
          <>
            {adding && (
              <QuickAdd
                compact
                onConfigureReminder={(task) => {
                  setReminderTask(task)
                  setPanel('reminder')
                }}
                defaultPlannedDate={selectedDate}
                onCancel={() => setAdding(false)}
                onCreated={(task) => {
                  const date = fromUtcIso(task.plannedAt)
                  if (date) setSelectedDate(format(date, 'yyyy-MM-dd'))
                  else setNotice('任务已保存到收件箱（未设置计划日期）')
                  void reload()
                }}
                onRecurringCreated={(warning) => {
                  setNotice(warning ?? '重复任务已添加')
                  void reload()
                }}
              />
            )}
            <button
              type="button"
              className="floating__new"
              aria-label="新建任务"
              onClick={() => setAdding(true)}
            >
              <Icon name="plus" size={18} />
              <span>新建任务</span>
            </button>
          </>
        )}
      </div>

      {!clickThrough && (
        <footer className="floating__foot" data-drag-region>
          <label className="floating__opacity" data-no-drag>
            <span className="floating__opacity-label" aria-hidden="true">
              <Icon name="contrast" size={14} />
            </span>
            <input
              type="range"
              min={25}
              max={100}
              step={1}
              value={Math.round(opacity * 100)}
              aria-label="悬浮窗不透明度"
              title={`不透明度 ${Math.round(opacity * 100)}%（拖动调节，最低 25%）`}
              onChange={(e) => onOpacityInput(Number(e.target.value) / 100)}
            />
            <span className="floating__opacity-value">
              {Math.round(opacity * 100)}%
            </span>
          </label>
          <div className="floating__foot-row">
            <span className="floating__hint">拖动顶栏移动窗口</span>
            <button
              type="button"
              className="floating__btn"
              aria-label="隐藏已完成"
              aria-pressed={hideDone}
              title="显示 / 隐藏已完成"
              onClick={() => setHideDone((v) => !v)}
            >
              <Icon name="completed" size={14} />
            </button>
            <span className="floating__hint">
              完成 {doneTotal} / {total}
            </span>
          </div>
        </footer>
      )}

      {/* 右下角缩放把手：拖它改窗口大小，松手后尺寸会被记住 */}
      <div
        className="floating__resize"
        role="separator"
        aria-label="拖动调整悬浮窗大小"
        title="拖动调整大小（松手后记住）"
        onMouseDown={(e) => void onResizeStart(e)}
      />

      {/* 完整编辑表单：与主窗口共用同一个组件，能力完全一致 */}
      {fullEditing && (
        <TaskEditor
          task={fullEditing}
          onClose={() => setFullEditing(null)}
          onSaved={async () => {
            setFullEditing(null)
            await reload()
          }}
        />
      )}
    </div>
  )
}

/**
 * 快速添加窗（§3 要求独立的快速添加窗口）。
 *
 * 全局快捷键唤出后应立即能输入，因此这里自动聚焦输入框。
 */
export function QuickAddWindow() {
  const [notice, setNotice] = useState<string | null>(null)

  const close = useCallback(async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().hide()
    } catch {
      /* 忽略 */
    }
  }, [])

  return (
    <div className="quickwin">
      <QuickAddBody
        onCreated={async (title) => {
          setNotice(`已添加「${title}」`)
          window.setTimeout(() => setNotice(null), 2000)
          // 添加后保持窗口打开，方便连续录入；用户按 Esc 关闭
        }}
      />
      {notice && (
        <div className="quickwin__notice" role="status">
          {notice}
        </div>
      )}
      <div className="quickwin__hint">回车添加　·　Esc 关闭</div>
      <button type="button" className="sr-only" onClick={() => void close()}>
        关闭
      </button>
    </div>
  )
}

/** 快速添加窗内的输入区（复用主界面的解析逻辑但样式更紧凑） */
function QuickAddBody({
  onCreated,
}: {
  onCreated: (title: string) => void | Promise<void>
}) {
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async () => {
    const t = text.trim()
    if (!t) return
    setBusy(true)
    setError(null)
    try {
      const { parseQuickInput } = await import('../lib/nlp')
      const { combineDateTime } = await import('../lib/datetime')
      const parsed = parseQuickInput(t)
      const title = (parsed.title || t).trim()
      if (!title) {
        setError('请输入任务标题')
        return
      }
      const dt = parsed.date
        ? combineDateTime(
            `${parsed.date.getFullYear()}-${String(parsed.date.getMonth() + 1).padStart(2, '0')}-${String(parsed.date.getDate()).padStart(2, '0')}`,
            parsed.hasTime
              ? `${String(parsed.date.getHours()).padStart(2, '0')}:${String(parsed.date.getMinutes()).padStart(2, '0')}`
              : '',
          )
        : null

      await ipc.createTask({
        title,
        priority: parsed.priority ?? undefined,
        plannedAt: dt?.utc ?? null,
        hasPlannedTime: dt?.hasTime ?? false,
      })
      setText('')
      await onCreated(title)
      // 快速添加窗创建后，主窗口与悬浮窗都应立刻看到
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="quickwin__row">
      <input
        className="quickwin__input selectable"
        value={text}
        autoFocus
        placeholder="添加任务，可直接写「明天 10:00 交周报」"
        aria-label="任务标题，可包含日期"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            void submit()
          } else if (e.key === 'Escape') {
            e.preventDefault()
            void (async () => {
              const { getCurrentWindow } =
                await import('@tauri-apps/api/window')
              await getCurrentWindow().hide()
            })()
          }
        }}
      />
      <button
        type="button"
        className="btn btn--primary btn--sm"
        disabled={busy || !text.trim()}
        onClick={() => void submit()}
      >
        {busy ? '添加中…' : '添加'}
      </button>
      {error && (
        <div className="quickwin__error" role="alert">
          {error}
        </div>
      )}
    </div>
  )
}

/** 供悬浮窗使用：判断某个任务是否属于"今天"（与主界面口径一致） */
export function isToday(task: Task): boolean {
  const r = todayRange()
  const at = task.plannedAt
  return at !== null && at >= r.start && at <= r.end
}
