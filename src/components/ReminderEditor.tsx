/**
 * 提醒设置组件（任务书 §4.3）。
 *
 * 关键交互设计：
 * - 「到期时 / 计划时间到点」是绝对提醒；「提前 N 分钟」是相对提醒，
 *   任务时间一改就会跟着走（后端负责重算）。
 * - 相对提醒在任务还没填对应时间时**无法创建**，界面直接禁用并说明原因，
 *   而不是让用户点了才报错。
 * - 已完成任务的提醒不再触发，界面明确标注，避免用户困惑"为什么没提醒"。
 */

import { useCallback, useEffect, useState } from 'react'
import * as rem from '../lib/reminder-ipc'
import { IpcError } from '../lib/ipc'
import type { Reminder, ReminderKind } from '../lib/reminder-ipc'
import { fromUtcIso } from '../lib/datetime'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

/** 常用提前量（分钟），覆盖绝大多数使用场景 */
const PRESETS = [5, 10, 15, 30, 60, 120, 24 * 60]

interface ReminderEditorProps {
  taskId: string
  /** 任务是否有计划时间 / 截止时间，用于决定哪些提醒类型可用 */
  hasPlanned: boolean
  hasDue: boolean
  /** 任务是否已完成（已完成任务的提醒不会触发） */
  taskDone: boolean
}

export function ReminderEditor({ taskId, hasPlanned, hasDue, taskDone }: ReminderEditorProps) {
  const [items, setItems] = useState<Reminder[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // 新建表单
  const [kind, setKind] = useState<ReminderKind>('at_due')
  const [offset, setOffset] = useState(30)
  const [customAt, setCustomAt] = useState('')

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      setItems(await rem.reminderList(taskId))
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

  /** 当前选择的类型是否满足创建条件 */
  const canCreate = (() => {
    switch (kind) {
      case 'at_due':
      case 'before_due':
        return hasDue
      case 'at_planned':
      case 'before_planned':
        return hasPlanned
      case 'custom':
        return customAt.trim().length > 0
      default:
        return false
    }
  })()

  const disabledReason = (() => {
    if (canCreate) return null
    switch (kind) {
      case 'at_due':
      case 'before_due':
        return '该任务还没有截止时间，请先设置截止时间，或改用「自定义时间」'
      case 'at_planned':
      case 'before_planned':
        return '该任务还没有计划时间，请先设置计划时间，或改用「自定义时间」'
      case 'custom':
        return '请选择提醒的具体日期与时间'
      default:
        return null
    }
  })()

  const create = async () => {
    setBusy(true)
    setError(null)
    try {
      await rem.reminderCreate({
        taskId,
        kind,
        offsetMinutes:
          kind === 'before_due' || kind === 'before_planned' ? offset : undefined,
        remindAt: kind === 'custom' ? new Date(customAt).toISOString() : undefined,
      })
      setCustomAt('')
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const kindLabel: Record<string, string> = {
    at_due: '截止时',
    before_due: '截止前',
    at_planned: '计划时间到点',
    before_planned: '计划时间前',
    custom: '自定义时间',
  }

  /** 人类可读的提醒描述 */
  const describe = (r: Reminder): string => {
    const base = kindLabel[r.kind] ?? r.kind
    if ((r.kind === 'before_due' || r.kind === 'before_planned') && r.offsetMinutes) {
      const m = r.offsetMinutes
      const human =
        m >= 1440 && m % 1440 === 0
          ? `${m / 1440} 天`
          : m >= 60 && m % 60 === 0
            ? `${m / 60} 小时`
            : `${m} 分钟`
      return `${base} ${human}`
    }
    return base
  }

  const stateOf = (r: Reminder): { text: string; cls: string } => {
    if (r.firedAt === 'expired') return { text: '已过期', cls: 'chip chip--muted' }
    if (r.firedAt) return { text: '已提醒', cls: 'chip chip--ok' }
    if (r.isEnabled !== 1) return { text: '已停用', cls: 'chip chip--muted' }
    if (taskDone) return { text: '任务已完成，不再提醒', cls: 'chip chip--muted' }
    return { text: '等待中', cls: 'chip' }
  }

  return (
    <div className="reminders">
      <div className="reminders__head">
        <span>提醒</span>
        {taskDone && (
          <span className="chip chip--muted" title="任务已完成，所有提醒都不会再触发">
            任务已完成
          </span>
        )}
      </div>

      {loading ? (
        <div className="skeleton" style={{ height: 30 }} />
      ) : items.length === 0 ? (
        <p className="reminders__empty">还没有提醒。添加一个，到点会通过 Windows 系统通知提醒你。</p>
      ) : (
        <ul className="remlist">
          {items.map((r) => {
            const st = stateOf(r)
            const at = fromUtcIso(r.remindAt)
            return (
              <li key={r.id} className="remrow">
                <span className="remrow__kind">{describe(r)}</span>
                <span className="remrow__time">
                  {at ? at.toLocaleString('zh-CN', { hour12: false }) : r.remindAt}
                </span>
                <span className={st.cls}>{st.text}</span>
                <span className="remrow__actions">
                  <button
                    type="button"
                    className="icon-btn"
                    title={r.isEnabled === 1 ? '停用此提醒' : '启用此提醒'}
                    aria-label={r.isEnabled === 1 ? '停用提醒' : '启用提醒'}
                    onClick={async () => {
                      try {
                        await rem.reminderSetEnabled(r.id, r.isEnabled !== 1)
                        await reload()
                      } catch (e) {
                        setError(errText(e))
                      }
                    }}
                  >
                    <Icon name={r.isEnabled === 1 ? 'pause' : 'play'} size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    title="删除提醒"
                    aria-label="删除提醒"
                    onClick={async () => {
                      try {
                        await rem.reminderDelete(r.id)
                        await reload()
                      } catch (e) {
                        setError(errText(e))
                      }
                    }}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </span>
              </li>
            )
          })}
        </ul>
      )}

      {/* 新建提醒 */}
      <div className="remnew">
        <select
          className="input input--compact"
          value={kind}
          aria-label="提醒类型"
          onChange={(e) => setKind(e.target.value as ReminderKind)}
        >
          <option value="at_due">截止时提醒</option>
          <option value="before_due">截止前提醒</option>
          <option value="at_planned">计划时间到点提醒</option>
          <option value="before_planned">计划时间前提醒</option>
          <option value="custom">自定义时间</option>
        </select>

        {(kind === 'before_due' || kind === 'before_planned') && (
          <select
            className="input input--compact"
            value={offset}
            aria-label="提前多久"
            onChange={(e) => setOffset(Number(e.target.value))}
          >
            {PRESETS.map((m) => (
              <option key={m} value={m}>
                {m >= 1440 ? `${m / 1440} 天前` : m >= 60 ? `${m / 60} 小时前` : `${m} 分钟前`}
              </option>
            ))}
          </select>
        )}

        {kind === 'custom' && (
          <input
            type="datetime-local"
            className="input input--compact"
            value={customAt}
            aria-label="提醒时间"
            onChange={(e) => setCustomAt(e.target.value)}
          />
        )}

        <button
          type="button"
          className="btn btn--ghost btn--sm"
          disabled={busy || !canCreate}
          title={disabledReason ?? '添加提醒'}
          onClick={() => void create()}
        >
          添加提醒
        </button>
      </div>

      {disabledReason && <p className="remnew__hint">{disabledReason}</p>}

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
    </div>
  )
}
