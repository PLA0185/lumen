import { callBackend as call } from './ipc'
import type { MemoDocument } from './memos-ipc'
export interface SyncOptions {
  scope: 'all' | 'tasks' | 'memos'
  direction: 'both' | 'upload' | 'download'
}
export const SYNC_SCOPES = { all: '全部业务数据', tasks: '仅任务及关联数据', memos: '仅备忘与流程（含图片及文件）' } as const
export const SYNC_DIRECTIONS = { both: '双向同步', upload: '仅上传到云端', download: '仅从云端下载' } as const
export function defaultSync(config: CloudConfig | null): SyncOptions {
  return config?.defaultSync ?? { scope: config?.inheritAll === false ? 'memos' : 'all', direction: 'both' }
}
export interface CloudConfig {
  server: string; account: string; folder: string; connectionId: string
  workspaceId: string; enabled: boolean; inheritAll: boolean
  defaultSync?: SyncOptions | null
}
export interface CloudStatus {
  config: CloudConfig | null; pending: number; conflicts: string[]
  lastScan: string | null; lastUpload: string | null; lastError: string | null
  retryUntil: number
}
export interface MemoRevision { version: number; id: string; parents: string[]; document: MemoDocument }
export interface CloudHistory { heads: string[]; versions: MemoRevision[] }
export interface BusinessRevision {
  id: string; parents: string[]; table: string; recordId: string
  key: Record<string, unknown>; row: Record<string, unknown> | null; createdAt: string
}
export interface BusinessConflict { id: string; table: string; heads: string[]; versions: BusinessRevision[] }
export const cloudStatus = () => call<CloudStatus>('cloud_sync_status')
export const cloudConnect = (input: { server: string; account: string; folder: string; password: string; recoveryCode: string; inheritAll: boolean }) =>
  call<CloudStatus>('cloud_sync_connect', { input })
export const cloudEnable = (enabled: boolean) => call<CloudStatus>('cloud_sync_set_enabled', { enabled })
export const cloudInheritance = (inheritAll: boolean) => call<CloudStatus>('cloud_sync_set_inheritance', { inheritAll })
export const cloudNow = (options?: SyncOptions) => call<CloudStatus>('cloud_sync_now', { options: options ?? null })
export const cloudSetDefaults = (options: SyncOptions) => call<CloudStatus>('cloud_sync_set_defaults', { options })
export const cloudRecoveryCode = () => call<string>('cloud_sync_recovery_code')
export const cloudHistory = (id: string) => call<CloudHistory>('cloud_sync_history', { id })
export const cloudRestore = (id: string, eventId: string, expectedHeads: string[], keepBoth: boolean) =>
  call<MemoDocument>('cloud_sync_restore', { id, eventId, expectedHeads, keepBoth })
export const cloudBusinessConflicts = () => call<BusinessConflict[]>('cloud_sync_business_conflicts')
export const cloudBusinessResolve = (id: string, eventId: string, expectedHeads: string[]) =>
  call<void>('cloud_sync_business_resolve', { id, eventId, expectedHeads })
