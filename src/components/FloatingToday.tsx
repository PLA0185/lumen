/**
 * 悬浮今日小窗（任务书 §8.4）。
 *
 * ## 为什么它必须是独立窗口而不是主窗口的一部分
 *
 * 它是一个"桌面组件"：无边框、可置顶、可半透明、可穿透。
 * 若塞进主窗口，这些窗口级能力会同时作用于主窗口，用户就无法
 * 一边让主窗口正常显示在任务栏、一边让今日清单浮在桌面角落。
 *
 * ## 穿透时的可用性设计
 *
 * 穿透开启后本窗口收不到任何鼠标事件。因此：
 * - 界面上明确提示"穿透已开启"，避免用户以为程序卡死；
 * - 提供一个仅在**未穿透**时可见的关闭按钮；
 * - 穿透的关闭入口始终在托盘与全局快捷键上（§8.2 要求）。
 */

import { useCallback, useEffect, useState } from 'react'
import * as win from '../lib/window-ipc'
import * as ipc from '../lib/ipc'
import * as rec from '../lib/recurrence-ipc'
import { fromUtcIso, isOverdue, todayRange } from '../lib/datetime'
import type { FloatingState } from '../lib/window-ipc'
import type { Task } from '../lib/types'

export function FloatingToday() {
  const [tasks, setTasks] = useState<Task[]>([])
  const [state, setState] = useState<FloatingState | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

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
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
    // 主窗口里的增删改需要反映到悬浮窗，因此定时刷新 + 监听事件
    const t = window.setInterval(() => void reload(), 30_000)
    return () => window.clearInterval(t)
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
                }
              : s,
          )
          // 不透明度用 CSS 应用：比整窗 alpha 更可控，且能保证文字对比度（§8.3）
          document.documentElement.style.setProperty(
            '--floating-opacity',
            String(cfg.floatingOpacity),
          )
        })
      } catch {
        // 非 Tauri 环境忽略
      }
    })()
    return () => unlisten?.()
  }, [])

  const toggle = async (id: string, done: boolean) => {
    try {
      await ipc.toggleTaskDone(id, done)
      await reload()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const done = tasks.filter((t) => t.status === 'done').length
  const open = tasks.length - done

  /** 拖拽窗口：无边框窗口需要自己实现拖动 */
  const onDragStart = async (e: React.MouseEvent) => {
    // 只在穿透关闭时有效（穿透时本来就收不到事件）
    if (state?.clickThrough) return
    if (e.button !== 0) return
    const target = e.target as HTMLElement
    // 按钮上的点击不触发拖动
    if (target.closest('button')) return
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startDragging()
    } catch {
      // 拖动失败不影响功能
    }
  }

  return (
    <div
      className="floating"
      style={{ opacity: state?.opacity ?? 1 }}
      onMouseDown={(e) => void onDragStart(e)}
    >
      <header className="floating__head">
        <span className="floating__title">今日</span>
        <span className="floating__count">
          {open > 0 ? `${open} 项待办` : tasks.length > 0 ? '全部完成 ✓' : '暂无安排'}
        </span>
        {!state?.clickThrough && (
          <button
            type="button"
            className="floating__btn"
            title="隐藏悬浮窗（可在托盘或设置中重新打开）"
            aria-label="隐藏悬浮窗"
            onClick={async () => {
              try {
                await win.windowApplyAction('hide_floating')
              } catch {
                /* 隐藏失败时保持原样 */
              }
            }}
          >
            ✕
          </button>
        )}
      </header>

      {state?.clickThrough && (
        <div className="floating__through" role="note">
          鼠标穿透已开启，本窗口不可点击。
          <br />
          关闭方式：托盘菜单，或按 {win.humanizeAccel('CmdOrCtrl+Alt+A')} 打开主窗口后到设置中关闭。
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
          <span className="floating__hint">在主窗口里给任务设置「计划时间」为今天即可显示在这里。</span>
        </div>
      ) : (
        <ul className="floating__list">
          {tasks.map((t) => {
            const isDone = t.status === 'done'
            const overdue = isOverdue(t)
            const at = fromUtcIso(t.plannedAt)
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
                  disabled={state?.clickThrough}
                  onClick={() => void toggle(t.id, !isDone)}
                >
                  {isDone ? '✓' : ''}
                </button>
                <span className="floating__text" title={t.title}>
                  {t.title}
                </span>
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
                    ↻
                  </span>
                )}
              </li>
            )
          })}
        </ul>
      )}

      {!state?.clickThrough && (
        <footer className="floating__foot">
          <span className="floating__hint">拖动此区域可移动窗口</span>
        </footer>
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
      <div className="quickwin__hint">
        回车添加　·　Esc 关闭
      </div>
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

/** 避免 tree-shaking 误删 */
export { rec }
