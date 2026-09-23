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

import { useCallback, useEffect, useRef, useState } from 'react'
import * as win from '../lib/window-ipc'
import * as ipc from '../lib/ipc'
import * as bus from '../lib/bus'
import { fromUtcIso, isOverdue, todayRange } from '../lib/datetime'
import { TaskEditor } from './TaskEditor'
import type { FloatingState } from '../lib/window-ipc'
import type { Task } from '../lib/types'
import { Icon } from './Icons'

/** 拖动结束后再落库的延迟：拖动过程中会连续触发 resize 事件 */
const SIZE_SAVE_DELAY = 400
/** 不透明度落库节流：滑块拖动时 input 事件非常密集 */
const OPACITY_SAVE_DELAY = 250

export function FloatingToday() {
  const [tasks, setTasks] = useState<Task[]>([])
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
    try {
      const r = todayRange()
      const [list, st] = await Promise.all([
        ipc.listTasks({
          statuses: ['todo', 'doing', 'waiting', 'done'],
          plannedFrom: r.start,
          plannedTo: r.end,
          sortBy: 'manual',
          limit: 100,
        }),
        win.windowFloatingState(),
      ])
      setTasks(list)
      setState(st)
      setOpacity(st.opacity)
      document.documentElement.style.setProperty('--floating-opacity', String(st.opacity))
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
    // 定时刷新作为兜底（例如主窗口没开、事件丢了）
    const t = window.setInterval(() => void reload(), 60_000)
    // 主窗口里的增删改会广播 tasks-changed，收到就立刻刷新，
    // 不必等下一次定时轮询——"改了没反应"最容易被当成没保存。
    const off = bus.onTasksChanged(() => void reload())
    return () => {
      window.clearInterval(t)
      off()
    }
  }, [reload])

  /** 监听后端广播的配置变化，实时更新不透明度与穿透提示 */
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen<win.WindowConfig>('floating-config', (e) => {
          const cfg = e.payload
          setState((s) =>
            s
              ? {
                  ...s,
                  clickThrough: cfg.floatingClickThrough,
                  alwaysOnTop: cfg.floatingAlwaysOnTop,
                  opacity: cfg.floatingOpacity,
                  enabled: cfg.floatingEnabled,
                  width: cfg.floatingWidth,
                  height: cfg.floatingHeight,
                }
              : s,
          )
          // 不透明度用 CSS 应用：比整窗 alpha 更可控，且能保证文字对比度（§8.3）
          setOpacity(cfg.floatingOpacity)
          document.documentElement.style.setProperty(
            '--floating-opacity',
            String(cfg.floatingOpacity),
          )
        })
      } catch (e) {
        // 事件监听不可用时不影响主流程，但要说明原因，避免"改了不生效"却查不到
        setError(`悬浮窗实时同步不可用：${e instanceof Error ? e.message : String(e)}`)
      }
    })()
    return () => unlisten?.()
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
                setState((s) => (s ? { ...s, width: applied.width, height: applied.height } : s))
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
      void bus.notifyTasksChanged()
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
      void bus.notifyTasksChanged()
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  /** 就地新增：默认计划在今天，且不带具体时刻 */
  const addToday = async (text: string) => {
    const title = text.trim()
    if (!title) return
    setBusy(true)
    try {
      const r = todayRange()
      await ipc.createTask({
        title,
        plannedAt: r.start,
        hasPlannedTime: false,
      })
      await reload()
      void bus.notifyTasksChanged()
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const done = tasks.filter((t) => t.status === 'done').length
  const open = tasks.length - done
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
    if (target.closest('button, input, select, textarea, a, [role="button"], [data-no-drag]')) {
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
        <span className="floating__title">今日</span>
        <span className="floating__count">
          {open > 0 ? `${open} 项待办` : tasks.length > 0 ? '全部完成 ✓' : '暂无安排'}
        </span>
        {!clickThrough && (
          <span className="floating__tools" data-no-drag>
            <button
              type="button"
              className={`floating__btn${state?.alwaysOnTop ? ' floating__btn--on' : ''}`}
              aria-pressed={state?.alwaysOnTop ?? false}
              title={state?.alwaysOnTop ? '取消置顶（会被其它窗口盖住）' : '置顶显示（始终浮在最前）'}
              aria-label={state?.alwaysOnTop ? '取消置顶' : '置顶显示'}
              onClick={() => void act('toggle_floating_top')}
            >
              <Icon name="pin" size={15} />
            </button>
            <button
              type="button"
              className="floating__btn"
              title="开启鼠标穿透（开启后本窗口不可点击，需从托盘或设置关闭）"
              aria-label="开启鼠标穿透"
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

      {loading ? (
        <div className="floating__empty">载入中…</div>
      ) : tasks.length === 0 ? (
        <div className="floating__empty">
          今天还没有安排。
          <br />
          <span className="floating__hint">在下面的输入框里直接添加，或在主窗口给任务设置「计划时间」为今天。</span>
        </div>
      ) : (
        <ul className="floating__list">
          {tasks.map((t) => {
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
                  aria-label={isDone ? `将「${t.title}」标记为未完成` : `完成「${t.title}」`}
                  className="floating__check"
                  disabled={clickThrough}
                  onClick={() => void toggle(t.id, !isDone)}
                >
                  {isDone ? <Icon name="completed" size={11} strokeWidth={2.4} /> : null}
                </button>

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
                    className="floating__text"
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
                  <span className="floating__mark" title="重复任务的一次发生">
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
              </li>
            )
          })}
        </ul>
      )}

      {!clickThrough && <FloatingAdd busy={busy} onAdd={addToday} />}

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
            <span className="floating__opacity-value">{Math.round(opacity * 100)}%</span>
          </label>
          <span className="floating__hint">按住空白处可移动窗口</span>
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
            void bus.notifyTasksChanged()
          }}
        />
      )}
    </div>
  )
}

/** 悬浮窗底部的快速添加：回车即加到今天，不跳窗口 */
function FloatingAdd({
  busy,
  onAdd,
}: {
  busy: boolean
  onAdd: (text: string) => void | Promise<void>
}) {
  const [text, setText] = useState('')
  const ref = useRef<HTMLInputElement>(null)

  const submit = async () => {
    const t = text.trim()
    if (!t) return
    await onAdd(t)
    setText('')
    ref.current?.focus()
  }

  return (
    <div className="floating__add">
      <input
        ref={ref}
        className="floating__add-input selectable"
        value={text}
        placeholder="加一条今天的任务，回车确认"
        aria-label="在悬浮窗中添加今天的任务"
        disabled={busy}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            void submit()
          } else if (e.key === 'Escape') {
            e.preventDefault()
            setText('')
          }
        }}
      />
      <button
        type="button"
        className="floating__add-btn"
        aria-label="添加任务"
        disabled={busy || !text.trim()}
        onClick={() => void submit()}
      >
        <Icon name="plus" size={15} />
      </button>
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
function QuickAddBody({ onCreated }: { onCreated: (title: string) => void | Promise<void> }) {
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
      void bus.notifyTasksChanged()
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
              const { getCurrentWindow } = await import('@tauri-apps/api/window')
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
