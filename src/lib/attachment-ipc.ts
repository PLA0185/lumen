/**
 * 附件 IPC 封装（任务书 §4.1）。
 */

import { invoke } from '@tauri-apps/api/core'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 附件 */
export interface Attachment {
  id: string
  taskId: string
  fileName: string
  mimeType: string | null
  byteSize: number | null
  sha256: string | null
  /** reference = 仅记录原文件；copied = 已复制到受控目录 */
  storageMode: string
  externalPath: string | null
  storedPath: string | null
  createdAt: string
}

/** 附件文件存在性检查结果 */
export interface AttachmentCheck {
  id: string
  fileName: string
  exists: boolean
  mode: string
  path: string
}

/** 孤儿附件清理结果（第二轮整改任务书 §6.5） */
export interface OrphanCleanupResult {
  scanned: number
  kept: number
  removed: number
  skipped: number
  removedFiles: string[]
}

export const ATT_CMD = {
  add: 'attachment_add',
  list: 'attachment_list',
  remove: 'attachment_remove',
  reveal: 'attachment_reveal',
  check: 'attachment_check',
  cleanupOrphans: 'attachment_cleanup_orphans',
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

/**
 * 添加附件。
 *
 * `mode`：
 * - `reference` 只在数据库里记录原文件路径，不复制、不占用额外空间；
 * - `copied` 把文件复制到 Lumen 数据目录，备份时能找到它，但会占双份空间。
 */
export const attachmentAdd = (
  taskId: string,
  sourcePath: string,
  mode: 'reference' | 'copied' = 'reference',
): Promise<Attachment> => call(ATT_CMD.add, { taskId, sourcePath, mode })

export const attachmentList = (taskId: string): Promise<Attachment[]> =>
  call(ATT_CMD.list, { taskId })

/** 移除附件记录；返回是否同时删除了受控副本（原文件始终不动） */
export const attachmentRemove = (id: string): Promise<boolean> =>
  call(ATT_CMD.remove, { id })

/** 取附件的可打开路径（用于在资源管理器中定位） */
export const attachmentReveal = (id: string): Promise<string> => call(ATT_CMD.reveal, { id })

export const attachmentCheck = (taskId: string): Promise<AttachmentCheck[]> =>
  call(ATT_CMD.check, { taskId })

/**
 * 扫描并清理受控附件目录里数据库已无引用的副本文件。
 *
 * 只会删除"位于受控目录内、命名是 Lumen 生成的 UUID 形式、数据库无引用"的文件；
 * 引用模式的原文件与用户自己放进目录的内容都不会被触碰。
 */
export const attachmentCleanupOrphans = (): Promise<OrphanCleanupResult> =>
  call(ATT_CMD.cleanupOrphans)
