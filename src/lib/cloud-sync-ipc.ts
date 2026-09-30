import { callBackend as call } from './ipc'
import type { MemoDocument } from './memos-ipc'
export interface CloudConfig {
  server: string; account: string; folder: string; connectionId: string
  workspaceId: string; enabled: boolean; inheritAll: boolean
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
export const cloudNow = () => call<CloudStatus>('cloud_sync_now')
export const cloudRecoveryCode = () => call<string>('cloud_sync_recovery_code')
export const cloudHistory = (id: string) => call<CloudHistory>('cloud_sync_history', { id })
export const cloudRestore = (id: string, eventId: string, expectedHeads: string[], keepBoth: boolean) =>
  call<MemoDocument>('cloud_sync_restore', { id, eventId, expectedHeads, keepBoth })
export const cloudBusinessConflicts = () => call<BusinessConflict[]>('cloud_sync_business_conflicts')
export const cloudBusinessResolve = (id: string, eventId: string, expectedHeads: string[]) =>
  call<void>('cloud_sync_business_resolve', { id, eventId, expectedHeads })
