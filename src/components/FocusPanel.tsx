/**
 * 专注模式面板（任务书 §4.4）。
 *
 * ## 计时的真相来源是数据库，不是这个组件
 *
 * 本地只做**显示用**的秒数推进（每 200ms 读一次时间戳算差值），
 * 真实的已进行时长由后端按 `elapsed_seconds + (now - last_resumed_at)` 计算。
 * 这样即使界面重渲染、切走再回来、甚至程序被杀掉重开，
 * 时间都不会丢失或重复计算。
 *
 * 刻意**不用 setInterval 累加一个计数器**——那种做法在窗口被系统挂起
 * （比如笔记本合盖）后会少算时间，用户会发现"我明明专注了半小时，
 * 它只记了 10 分钟"。
 *
 * ## 到时间后的行为
 *
 * 不自动结束。到时间只是把按钮变成鼓励性的"完成并记录"，
 * 用户可能想多做一会儿。这与 §4.4 的"可暂停/恢复"一致：
 * 计时器服务于人，不该反过来打断人。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate } from '../lib/request-gate'
import * as focus from '../lib/focus-ipc'
import { IpcError } from '../lib/ipc'
import { useApp } from '../lib/store'
import type { FocusSummary, FocusView } from '../lib/focus-ipc'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface FocusPanelProps {
  /** 可选：绑定到某个任务 */
  taskId?: string
  taskTitle?: string
  /** 紧凑模式（用于任务卡片展开区） */
  compact?: boolean
}

export function FocusPanel({ taskId, taskTitle, compact = false }: FocusPanelProps) {
  const gate = useMemo(createRequestGate, [])
  const pushToast = useApp((s) => s.pushToast)
  const [view, setView] = useState<FocusView | null>(null)
  const [summary, setSummary] = useState<FocusSummary | null>(null)
  const [minutes, setMinutes] = useState(25)
  const [kind, setKind] = useState<focus.FocusKind>('pomodoro')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** 仅用于驱动重渲染的本地秒数（真实值来自后端视图） */
  const [tick, setTick] = useState(0)
  /** 上一次向后端同步的时间，避免频繁请求 */
  const lastSyncRef = useRef(0)

  const reload = useCallback(async () => {
    const token = gate.begin()
    try {
      const [cur, sum] = await Promise.all([focus.focusCurrent(), focus.focusSummary()])
      if (!gate.isCurrent(token)) return
      setView(cur)
      setSummary(sum)
      setError(null)
    } catch (e) {
      if (gate.isCurrent(token)) setError(errText(e))
    } finally {
      if (gate.isCurrent(token)) setLoading(false)
    }
  }, [gate])

  useEffect(() => {
    void reload()
    const off = onDataChanged(['focus', 'tasks', 'all'], () => void reload())
    return () => { off(); gate.invalidate() }
  }, [gate, reload])

  useEffect(() => () => gate.dispose(), [gate])

  /**
   * 本地时钟：每 200ms 推进一次显示。
   *
   * 只在 running 时启动，避免空闲时白耗电。
   * 每 15 秒向后端同步一次，让"程序被杀掉后残留会话"的判定能及时生效。
   */
  useEffect(() => {
    if (view?.state !== 'running') return

    const id = window.setInterval(() => {
      setTick((t) => t + 1)

      const now = Date.now()
      if (now - lastSyncRef.current > 15_000) {
        lastSyncRef.current = now
        const token = gate.begin()
        // 同步失败不打扰用户：本地显示仍在走，后端值稍后会对上
        void focus
          .focusCurrent()
          .then((cur) => {
            if (cur && gate.isCurrent(token)) setView(cur)
          })
          .catch(() => {})
      }
    }, 200)

    return () => window.clearInterval(id)
  }, [gate, view?.state])

  /** 由本地推进推算的显示秒数 */
  const displaySeconds = (() => {
    if (!view) return 0
    if (view.state !== 'running' || !view.lastResumedAt) return view.currentSeconds
    const last = new Date(view.lastResumedAt).getTime()
    if (Number.isNaN(last)) return view.currentSeconds
    const delta = Math.max(0, Math.floor((Date.now() - last) / 1000))
    return view.elapsedSeconds + delta
    // tick 只是为了触发重算
    void tick
  })()

  const remaining =
    view && view.kind === 'pomodoro' ? Math.max(0, view.plannedSeconds - displaySeconds) : null
  const progress =
    view && view.plannedSeconds > 0
      ? Math.min(100, Math.floor((displaySeconds * 100) / view.plannedSeconds))
      : 0
  const isDue = view?.kind === 'pomodoro' && displaySeconds >= view.plannedSeconds
  const acts = focus.availableActions(view?.state ?? null)

  const start = async () => {
    setBusy(true)
    setError(null)
    try {
      const v = await focus.focusStart({ taskId: taskId ?? null, kind, minutes })
      setView(v)
      lastSyncRef.current = Date.now()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doPause = async () => {
    if (!view) return
    setBusy(true)
    try {
      setView(await focus.focusPause(view.id))
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doResume = async () => {
    if (!view) return
    setBusy(true)
    try {
      setView(await focus.focusResume(view.id))
      lastSyncRef.current = Date.now()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doEnd = async () => {
    if (!view) return
    setBusy(true)
    try {
      const v = await focus.focusEnd(view.id, true)
      setView(null)
      await reload()
      const secs = v.elapsedSeconds
      pushToast(
        'success',
        `本轮专注 ${focus.formatHms(secs)}` +
          (v.taskId ? '，已累加到任务的实际耗时' : ''),
      )
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doCancel = async () => {
    if (!view) return
    if (
      !window.confirm(
        '放弃这次专注？\n\n本次时长不会被记录到任务的实际耗时里。',
      )
    ) {
      return
    }
    setBusy(true)
    try {
      await focus.focusCancel(view.id)
      setView(null)
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  if (loading) {
    return <div className="skeleton" style={{ height: compact ? 60 : 120 }} />
  }

  const idle = !view

  return (
    <div className={`focuspanel${compact ? ' focuspanel--compact' : ''}`}>
      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}

      {idle ? (
        <>
          {/* ---------------- 未开始：选择时长并开始 ---------------- */}
          <div className="focuspanel__setup">
            {!compact && (
              <div className="formrow">
                <span className="formlabel">计时方式</span>
                <div className="segmented" role="radiogroup" aria-label="计时方式">
                  {(
                    [
                      ['pomodoro', '番茄钟（倒计时）'],
                      ['stopwatch', '正计时'],
                    ] as const
                  ).map(([v, label]) => (
                    <button
                      key={v}
                      type="button"
                      role="radio"
                      aria-checked={kind === v}
                      className={`segmented__item${kind === v ? ' segmented__item--on' : ''}`}
                      onClick={() => setKind(v)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {kind === 'pomodoro' && (
              <div className="formrow">
                <span className="formlabel">时长</span>
                <div className="rulerow">
                  {focus.POMODORO_PRESETS.map((m) => (
                    <button
                      key={m}
                      type="button"
                      className={`tagtoggle${minutes === m ? ' tagtoggle--on' : ''}`}
                      aria-pressed={minutes === m}
                      onClick={() => setMinutes(m)}
                    >
                      {m} 分钟
                    </button>
                  ))}
                  <input
                    type="number"
                    className="input input--tiny"
                    min={1}
                    max={480}
                    value={minutes}
                    aria-label="自定义分钟数"
                    onChange={(e) =>
                      setMinutes(Math.max(1, Math.min(480, Number(e.target.value) || 25)))
                    }
                  />
                </div>
              </div>
            )}

            <div className="setactions">
              <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void start()}>
                {busy ? '启动中…' : '开始专注'}
              </button>
              {taskTitle && (
                <span className="setgroup__hint" style={{ margin: 0 }}>
                  将记录到「{taskTitle}」
                </span>
              )}
            </div>
          </div>

          {/* 今日/本周汇总 */}
          {summary && (summary.todaySessions > 0 || summary.weekSeconds > 0) && (
            <div className="focuspanel__stats">
              今日 <strong>{summary.todaySessions}</strong> 轮、
              <strong>{focus.formatHms(summary.todaySeconds)}</strong>
              　本周累计 <strong>{focus.formatHms(summary.weekSeconds)}</strong>
            </div>
          )}
        </>
      ) : (
        <>
          {/* ---------------- 计时中 ---------------- */}
          <div className="focuspanel__timer">
            <div
              className={`focuspanel__clock${isDue ? ' focuspanel__clock--due' : ''}`}
              role="timer"
              aria-live="off"
            >
              {remaining != null ? focus.formatHms(remaining) : focus.formatHms(displaySeconds)}
            </div>
            <div className="focuspanel__meta">
              <span className={`chip ${view.state === 'running' ? 'chip--ok' : 'chip--warn'}`}>
                {focus.FOCUS_STATE_LABELS[view.state]}
              </span>
              {view.kind === 'stopwatch' && <span className="chip chip--muted">正计时</span>}
              {view.taskTitle && <span className="chip chip--muted">{view.taskTitle}</span>}
              {view.state === 'interrupted' && (
                <span className="chip chip--warn" title="程序可能被关闭或计时中断过">
                  中断后可恢复
                </span>
              )}
            </div>

            {view.kind === 'pomodoro' && (
              <div className="progress" style={{ marginTop: 8 }}>
                <div className="progress__bar" style={{ width: `${progress}%` }} />
              </div>
            )}

            {isDue && (
              <div className="alert alert--warn" role="status" style={{ marginTop: 8 }}>
                <span>
                  时间到了。你可以<strong>继续做</strong>（计时会接着走），
                  或点「完成并记录」把当前时长写入
                  {view.taskId ? '任务的实际耗时' : '专注记录'}。
                </span>
              </div>
            )}
          </div>

          <div className="setactions">
            {acts.canPause && (
              <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => void doPause()}>
                暂停
              </button>
            )}
            {acts.canResume && (
              <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => void doResume()}>
                继续
              </button>
            )}
            {acts.canEnd && (
              <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void doEnd()}>
                完成并记录
              </button>
            )}
            {acts.canCancel && (
              <button type="button" className="btn btn--danger" disabled={busy} onClick={() => void doCancel()}>
                放弃
              </button>
            )}
          </div>

          <p className="setgroup__hint">
            计时以数据库为准：即使程序被强制关闭，重新打开后也能恢复或明确显示为已中断，
            不会悄悄少算时间。
          </p>
        </>
      )}
    </div>
  )
}
