/**
 * 备份、导出与恢复的 IPC 封装（任务书 §9）。
 */

import { invokeData as invoke } from './data-change'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 各类数据的条目数 */
export interface BackupStats {
  tasks: number
  projects: number
  categories: number
  tags: number
  taskTags: number
  subtasks: number
  dependencies: number
  reminders: number
  attachments: number
  series: number
  segments: number
  settings: number
}

/** 导出结果 */
export interface ExportResult {
  path: string
  bytes: number
  checksum: string
  stats: BackupStats
  /** 附件未被包含时的提醒（若有附件） */
  attachmentWarning: string | null
}

/** 导入预览 */
export interface ImportPreview {
  path: string
  formatVersion: number
  appVersion: string
  createdAt: string
  checksumOk: boolean
  checksumError: string | null
  stats: BackupStats
  /** 当前库中的条目数，供对比 */
  current: BackupStats
  /** 将被覆盖的任务数 */
  willReplaceTasks: number
  attachmentsNote: string
  /** 阻断性问题；非空则不允许导入 */
  blockingIssues: string[]
}

/** 恢复结果 */
export interface RestoreResult {
  /** 恢复前自动生成的当前库快照路径（可退回） */
  safetyBackup: string | null
  imported: BackupStats
}

/** 备份文件条目 */
export interface BackupEntry {
  path: string
  fileName: string
  bytes: number
  modifiedAt: string
  /** 自动备份 / 手动备份 / 恢复前快照 / 迁移前快照 */
  kind: string
}

export const BACKUP_CMD = {
  export: 'backup_export',
  preview: 'backup_preview',
  restore: 'backup_restore',
  list: 'backup_list',
  delete: 'backup_delete',
  auto: 'backup_auto',
  exportCsv: 'export_csv',
  exportMarkdown: 'export_markdown',
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

/** 导出完整 JSON 备份。不传 path 时自动落到数据目录的 backups 子目录。 */
export const backupExport = (path?: string): Promise<ExportResult> =>
  call(BACKUP_CMD.export, { path: path ?? null })

/** 预览导入内容（不改动任何数据） */
export const backupPreview = (path: string): Promise<ImportPreview> =>
  call(BACKUP_CMD.preview, { path })

/** 从备份恢复（会先自动生成当前库快照） */
export const backupRestore = (path: string): Promise<RestoreResult> =>
  call(BACKUP_CMD.restore, { path })

export const backupList = (): Promise<BackupEntry[]> => call(BACKUP_CMD.list)

export const backupDelete = (path: string): Promise<boolean> => call(BACKUP_CMD.delete, { path })

/** 立即生成一次自动备份，并按保留份数清理旧的 */
export const backupAuto = (keep?: number): Promise<ExportResult> =>
  call(BACKUP_CMD.auto, { keep: keep ?? null })

export const exportCsv = (path: string): Promise<string> =>
  call(BACKUP_CMD.exportCsv, { path })

export const exportMarkdown = (path: string): Promise<string> =>
  call(BACKUP_CMD.exportMarkdown, { path })

/** 把字节数格式化为可读文本 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}
