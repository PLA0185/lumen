/**
 * IPC 层：前端与 Rust 后端之间的唯一通道。
 *
 * 设计要点（任务书 §3 / §10）：
 * - 所有后端错误被规范化为 `IpcError`，携带 `code`/`message`/`hint`，
 *   UI 据此展示可读提示与可恢复操作，而不是把原始异常抛给用户。
 * - 显式区分"运行在 Tauri 中"与"在纯浏览器中预览"两种环境：
 *   后者会给出明确错误，而不是静默返回假数据（§1「不得用假数据冒充已实现功能」）。
 * - 命令名集中定义，避免散落各处拼写不一致。
 */

import { invoke } from '@tauri-apps/api/core'
import type {
  AppInfo,
  BackendError,
  BulkActionInput,
  CreateTaskInput,
  DataPaths,
  ErrorCode,
  PurgeResult,
  SoftDeleteResult,
  Task,
  TaskQuery,
  TodayOverview,
  UpdateTaskInput,
} from './types'

/** 命令名常量表。与 Rust `invoke_handler` 中注册的名称必须完全一致。 */
export const CMD = {
  ping: 'ping',
  appInfo: 'app_info',
  appDataPaths: 'app_data_paths',
  taskCreate: 'task_create',
  taskUpdate: 'task_update',
  taskGet: 'task_get',
  taskList: 'task_list',
  taskToggleDone: 'task_toggle_done',
  taskSoftDelete: 'task_soft_delete',
  taskRestore: 'task_restore',
  taskPurge: 'task_purge',
  taskPurgeAllDeleted: 'task_purge_all_deleted',
  taskBulk: 'task_bulk',
  taskDuplicate: 'task_duplicate',
  taskReorder: 'task_reorder',
  taskReport: 'task_report',
  taskReportAll: 'task_report_all',
  exportPdf: 'export_pdf',
  todayOverview: 'today_overview',
  setRemindersPaused: 'set_reminders_paused',
  tasksInRange: 'tasks_in_range',
  taskReschedule: 'task_reschedule',
} as const

/**
 * 规范化后的 IPC 错误。
 *
 * UI 层只需要读 `userMessage()` 就能得到可直接展示的中文文案，
 * 需要分支处理时再读 `code`。
 */
export class IpcError extends Error {
  readonly code: ErrorCode
  readonly hint: string | null
  /** 原始错误，仅用于开发期诊断，不展示给用户 */
  readonly raw: unknown

  constructor(code: ErrorCode, message: string, hint: string | null, raw: unknown) {
    super(message)
    this.name = 'IpcError'
    this.code = code
    this.hint = hint
    this.raw = raw
  }

  /** 面向用户的完整文案（含恢复建议） */
  userMessage(): string {
    return this.hint ? `${this.message}\n${this.hint}` : this.message
  }

  /** 是否属于"用户可以重试"的临时性错误 */
  isRetryable(): boolean {
    return this.code === 'network' || this.code === 'timeout' || this.code === 'rate_limited'
  }
}

/** 判断当前是否运行在 Tauri 宿主中 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 把任意后端返回的错误值转成 IpcError */
function toIpcError(raw: unknown): IpcError {
  if (raw instanceof IpcError) return raw

  // Rust 的 AppError 序列化为 { code, message, hint }
  if (raw && typeof raw === 'object' && 'message' in raw && 'code' in raw) {
    const e = raw as BackendError
    const code = (typeof e.code === 'string' ? e.code : 'internal') as ErrorCode
    return new IpcError(code, String(e.message ?? '未知错误'), e.hint ?? null, raw)
  }

  if (typeof raw === 'string') {
    return new IpcError('internal', raw, null, raw)
  }

  return new IpcError('internal', '发生了未预期的错误', '请查看日志目录获取详细信息', raw)
}

/** 统一调用封装：所有后端调用都必须经此函数 */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new IpcError(
      'internal',
      '当前不在 Lumen 桌面程序内运行',
      '请通过桌面应用打开，而不是在浏览器中访问页面',
      null,
    )
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    const err = toIpcError(e)
    // 开发期打印原始错误便于定位；生产日志不包含任务正文（§10）
    if (import.meta.env.DEV) {
      console.error(`[ipc] ${cmd} 失败`, err.raw)
    }
    throw err
  }
}

// =============================================================================
// 应用级
// =============================================================================

/** 就绪探针：确认后端与数据库可用 */
export const ping = (): Promise<string> => call<string>(CMD.ping)

/** 应用健康信息 */
export const getAppInfo = (): Promise<AppInfo> => call<AppInfo>(CMD.appInfo)

/** 数据目录信息 */
export const getDataPaths = (): Promise<DataPaths> => call<DataPaths>(CMD.appDataPaths)

/** 设置提醒暂停状态 */
export const setRemindersPaused = (paused: boolean): Promise<boolean> =>
  call<boolean>(CMD.setRemindersPaused, { paused })

// =============================================================================
// 任务
// =============================================================================

/** 创建任务 */
export const createTask = (input: CreateTaskInput): Promise<Task> =>
  call<Task>(CMD.taskCreate, { input })

/** 更新任务（部分更新） */
export const updateTask = (id: string, input: UpdateTaskInput): Promise<Task> =>
  call<Task>(CMD.taskUpdate, { id, input })

/** 读取单个任务 */
export const getTask = (id: string): Promise<Task> => call<Task>(CMD.taskGet, { id })

/** 查询任务列表 */
export const listTasks = (query: TaskQuery = {}): Promise<Task[]> =>
  call<Task[]>(CMD.taskList, { query })

/** 完成 / 撤销完成（只影响本次，不涉及重复范围，§5） */
export const toggleTaskDone = (id: string, done: boolean): Promise<Task> =>
  call<Task>(CMD.taskToggleDone, { id, done })

/** 软删除到回收站 */
export const softDeleteTask = (id: string): Promise<SoftDeleteResult> =>
  call<SoftDeleteResult>(CMD.taskSoftDelete, { id })

/** 从回收站恢复 */
export const restoreTask = (id: string): Promise<Task> => call<Task>(CMD.taskRestore, { id })

/** 永久删除（仅限回收站内） */
export const purgeTask = (id: string): Promise<PurgeResult> =>
  call<PurgeResult>(CMD.taskPurge, { id })

/** 清空回收站 */
export const purgeAllDeleted = (): Promise<PurgeResult> =>
  call<PurgeResult>(CMD.taskPurgeAllDeleted)

/** 批量操作 */
export const bulkTasks = (input: BulkActionInput): Promise<number> =>
  call<number>(CMD.taskBulk, { input })

/** 复制任务的结果 */
export interface DuplicateResult {
  newTaskId: string
  title: string
  copiedTags: number
  copiedSubtasks: number
  copiedReminders: number
  /** 未一起复制的附件数量（>0 时界面应提示） */
  skippedAttachments: number
  note: string
}

/**
 * 复制任务。
 *
 * 副本状态为未完成、完成时间清空（复制是为了"再做一遍"）；
 * 标签、子任务、提醒会复制，但**附件不会**——界面据 `skippedAttachments`
 * 提示用户，而不是静默丢掉。
 */
export const duplicateTask = (id: string): Promise<DuplicateResult> =>
  call<DuplicateResult>(CMD.taskDuplicate, { id })

/**
 * 手动排序：把任务移到 `beforeId` 之前；`beforeId` 为 null 表示移到末尾。
 *
 * 后端用中点插入法，只更新被移动的那一行，因此上千条任务也不会卡。
 */
export const reorderTask = (movedId: string, beforeId?: string | null): Promise<number> =>
  call<number>(CMD.taskReorder, { input: { movedId, beforeId: beforeId ?? null } })

/** 打印 / PDF 报告的一行（归属名称已在后端解析好，避免前端 N+1） */
export interface TaskReportRow {
  task: Task
  projectName: string | null
  categoryName: string | null
  tagNames: string[]
}

/** 取报告数据：与列表同一套筛选语义，额外带项目/分类/标签名称 */
export const taskReport = (query: TaskQuery): Promise<TaskReportRow[]> =>
  call<TaskReportRow[]>(CMD.taskReport, { query })

/**
 * 取**完整**报告数据（分页读取，不受单页上限限制）。
 *
 * 整改任务书 §7：报告/归档类导出不能静默截断。后端按 500 条一页连续读取
 * 直到取完，`truncated` 为真表示碰到了总量安全上限（界面必须如实告知）。
 */
export interface ReportPage {
  rows: TaskReportRow[]
  total: number
  truncated: boolean
}

export const taskReportAll = (query: TaskQuery): Promise<ReportPage> =>
  call<ReportPage>(CMD.taskReportAll, { query })

/**
 * 把主窗口当前页面导出为 PDF。
 *
 * 调用前界面必须已经切到打印报告（`#print-report` 可见），
 * 因为 WebView2 打印的是**当前页面**。
 */
export const exportPdf = (path: string): Promise<void> => call<void>(CMD.exportPdf, { path })
/** 今日概览 */
export const getTodayOverview = (
  dayStartUtc: string,
  dayEndUtc: string,
  nowUtc?: string,
): Promise<TodayOverview> =>
  call<TodayOverview>(CMD.todayOverview, { dayStartUtc, dayEndUtc, nowUtc: nowUtc ?? null })

/** 日历视图：查询落在指定 UTC 范围内的任务 */
export const tasksInRange = (startUtc: string, endUtc: string): Promise<Task[]> =>
  call<Task[]>(CMD.tasksInRange, { startUtc, endUtc })

/** 拖拽改期：把任务的计划时间移到新日期（保留原时刻，不影响截止时间） */
export const rescheduleTask = (id: string, newDateUtc: string): Promise<Task> =>
  call<Task>(CMD.taskReschedule, { id, newDateUtc })
