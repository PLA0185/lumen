/**
 * 提醒相关 IPC 封装（任务书 §4.3）。
 */

import { invoke } from '@tauri-apps/api/core'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 提醒类型 */
export type ReminderKind =
  | 'at_due'
  | 'before_due'
  | 'at_planned'
  | 'before_planned'
  | 'custom'

/** 提醒 */
export interface Reminder {
  id: string
  taskId: string
  kind: string
  offsetMinutes: number | null
  remindAt: string
  isEnabled: number
  /** 正常情况下是触发时刻；'expired' 表示因超出补发窗口而作废 */
  firedAt: string | null
  snoozedUntil: string | null
  createdAt: string
  updatedAt: string
}

/** 待触发提醒（含任务标题，设置页展示用） */
export interface PendingReminder {
  id: string
  taskId: string
  taskTitle: string
  kind: string
  remindAt: string
}

/** 调度器状态 */
export interface SchedulerStatus {
  running: boolean
  paused: boolean
  /** 补发窗口（分钟）；0 表示不补发关闭期间错过的提醒 */
  missedGraceMinutes: number
  pendingCount: number
  expiredCount: number
}

export const REM_CMD = {
  create: 'reminder_create',
  list: 'reminder_list',
  setEnabled: 'reminder_set_enabled',
  delete: 'reminder_delete',
  snooze: 'reminder_snooze',
  status: 'reminder_scheduler_status',
  setGrace: 'reminder_set_grace',
  checkMissed: 'reminder_check_missed',
  listPending: 'reminder_list_pending',
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

/** 创建提醒。
 *
 * `remindAt` 用于 kind='custom'；`offsetMinutes` 用于相对型提醒。
 * 相对型提醒的绝对时刻由**后端**根据任务当前时间推导，
 * 前端不重复计算，避免两处逻辑不一致。
 */
export const reminderCreate = (input: {
  taskId: string
  kind: ReminderKind
  offsetMinutes?: number
  remindAt?: string
}): Promise<Reminder> => call(REM_CMD.create, { input })

export const reminderList = (taskId: string): Promise<Reminder[]> =>
  call(REM_CMD.list, { taskId })

export const reminderSetEnabled = (id: string, enabled: boolean): Promise<Reminder> =>
  call(REM_CMD.setEnabled, { id, enabled })

export const reminderDelete = (id: string): Promise<number> => call(REM_CMD.delete, { id })

/** 稍后提醒 */
export const reminderSnooze = (id: string, minutes: number): Promise<Reminder> =>
  call(REM_CMD.snooze, { id, minutes })

export const reminderSchedulerStatus = (): Promise<SchedulerStatus> => call(REM_CMD.status)

export const reminderSetGrace = (minutes: number): Promise<number> =>
  call(REM_CMD.setGrace, { minutes })

/** 立即把超出补发窗口的提醒标记为过期，返回处理数量 */
export const reminderCheckMissed = (): Promise<number> => call(REM_CMD.checkMissed)

export const reminderListPending = (limit?: number): Promise<PendingReminder[]> =>
  call(REM_CMD.listPending, { limit: limit ?? null })
