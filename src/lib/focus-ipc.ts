/**
 * 专注模式 IPC 封装（任务书 §4.4）。
 */

import { invokeData as invoke } from './data-change'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 会话状态 */
export type FocusState = 'idle' | 'running' | 'paused' | 'finished' | 'interrupted'

/** 计时类型 */
export type FocusKind = 'pomodoro' | 'stopwatch'

/** 专注会话 */
export interface FocusSession {
  id: string
  taskId: string | null
  kind: FocusKind
  state: FocusState
  plannedSeconds: number
  elapsedSeconds: number
  startedAt: string | null
  lastResumedAt: string | null
  endedAt: string | null
  createdAt: string
  updatedAt: string
}

/** 会话的实时视图 */
export interface FocusView extends FocusSession {
  /** 当前已进行秒数（运行中含从上次恢复到现在的时间） */
  currentSeconds: number
  /** 剩余秒数；正计时为 null */
  remainingSeconds: number | null
  progress: number
  isDue: boolean
  taskTitle: string | null
}

/** 汇总 */
export interface FocusSummary {
  todaySessions: number
  todaySeconds: number
  weekSeconds: number
  activeSession: FocusView | null
}

export const FOCUS_CMD = {
  start: 'focus_start',
  current: 'focus_current',
  pause: 'focus_pause',
  resume: 'focus_resume',
  end: 'focus_end',
  cancel: 'focus_cancel',
  summary: 'focus_summary',
} as const

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    throw new IpcError('internal', '当前不在 Lumen 桌面程序内运行', null, null)
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'message' in e && 'code' in e) {
      const err = e as { code: string; message: string; hint: string | null }
      throw new IpcError(err.code as ErrorCode, err.message, err.hint ?? null, e)
    }
    if (typeof e === 'string') throw new IpcError('internal', e, null, e)
    throw new IpcError('internal', '发生了未预期的错误', null, e)
  }
}

export const focusStart = (args: {
  taskId?: string | null
  kind?: FocusKind
  minutes?: number
}): Promise<FocusView> =>
  call(FOCUS_CMD.start, {
    input: {
      taskId: args.taskId ?? null,
      kind: args.kind ?? 'pomodoro',
      minutes: args.minutes ?? null,
    },
  })

/**
 * 读取当前会话。
 *
 * 后端会在这里处理"程序被杀掉后残留的 running 会话"：
 * 若倒计时已远超预期时长，会标记为 interrupted 并返回更新后的状态。
 */
export const focusCurrent = (): Promise<FocusView | null> => call(FOCUS_CMD.current)

export const focusPause = (sessionId: string): Promise<FocusView> =>
  call(FOCUS_CMD.pause, { sessionId })

export const focusResume = (sessionId: string): Promise<FocusView> =>
  call(FOCUS_CMD.resume, { sessionId })

/** 结束并记录时长（会累加到任务的 actual_minutes） */
export const focusEnd = (
  sessionId: string,
  recordToTask = true,
): Promise<FocusView> => call(FOCUS_CMD.end, { input: { sessionId, recordToTask } })

/** 放弃（不记录时长） */
export const focusCancel = (sessionId: string): Promise<boolean> =>
  call(FOCUS_CMD.cancel, { sessionId })

export const focusSummary = (): Promise<FocusSummary> => call(FOCUS_CMD.summary)

// =============================================================================
// 展示辅助（纯函数）
// =============================================================================

/** 秒 → `MM:SS` / `HH:MM:SS`（与后端 format_hms 规则一致） */
export function formatHms(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = s % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`
}

/** 会话状态的中文标签 */
export const FOCUS_STATE_LABELS: Record<FocusState, string> = {
  idle: '空闲',
  running: '进行中',
  paused: '已暂停',
  finished: '已结束',
  interrupted: '已中断',
}

/**
 * 常用的番茄钟时长（分钟）。
 *
 * 除标准 25 分钟外，给出几个常见变体；任务书未规定具体时长，
 * 因此这里只是提供便捷选项，用户也可以自行输入。
 */
export const POMODORO_PRESETS = [15, 20, 25, 45, 60]

/**
 * 根据当前会话状态决定可用的操作。
 *
 * 抽成纯函数是为了可测：界面按钮的可用性如果靠散落的 if 判断，
 * 很容易出现"暂停按钮在已结束时仍可点"这类问题。
 */
export function availableActions(state: FocusState | null): {
  canPause: boolean
  canResume: boolean
  canEnd: boolean
  canCancel: boolean
} {
  switch (state) {
    case 'running':
      // 运行中：可暂停、可结束、可放弃
      return { canPause: true, canResume: false, canEnd: true, canCancel: true }
    case 'paused':
      // 暂停中：可恢复、可结束（把已积累的时间记下来）、可放弃
      return { canPause: false, canResume: true, canEnd: true, canCancel: true }
    case 'interrupted':
      // 中断后仍允许恢复——用户可能只是去开了个会
      return { canPause: false, canResume: true, canEnd: true, canCancel: true }
    default:
      // idle / finished：没有任何进行中的操作
      return { canPause: false, canResume: false, canEnd: false, canCancel: false }
  }
}

/**
 * 判断会话是否"已到时间但还在跑"。
 *
 * 用于界面提示"时间到了，可以结束并记录"，
 * 而不是自动结束——用户可能想多做一会儿。
 */
export function isOverrun(v: FocusView | null): boolean {
  if (!v) return false
  return v.kind === 'pomodoro' && v.state === 'running' && v.currentSeconds >= v.plannedSeconds
}
