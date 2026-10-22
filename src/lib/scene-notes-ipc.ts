import { invokeData } from './data-change'
import { IpcError } from './ipc'
import type { BackendError } from './types'

export type SceneMatchMode = 'image' | 'ocr' | 'image_and_ocr'

export interface TemplateConfig {
  x: number; y: number; width: number; height: number
  sourceWidth: number; sourceHeight: number; threshold: number
}

export interface SceneRule {
  id: string; name: string; enabled: boolean; priority: number
  programAllowlist: string[]; windowTitleContains: string[]
  matchMode: SceneMatchMode; templateId: string | null; templateConfig: TemplateConfig | null
  requiredTerms: string[]; excludedTerms: string[]; headline: string; bodyMd: string
  memoId: string | null; stepId: string | null; stableHits: number; stableLeaveMs: number
  defaultSnoozeMinutes: number; revision: number; createdAt: string; updatedAt: string; deletedAt: string | null
}

export interface SceneRuleInput {
  id: string | null; expectedRevision: number | null; name: string; enabled: boolean; priority: number
  programAllowlist: string[]; windowTitleContains: string[]; matchMode: SceneMatchMode
  templateConfig: TemplateConfig | null; templateImageBase64: string | null; existingTemplateId: string | null
  requiredTerms: string[]; excludedTerms: string[]; headline: string; bodyMd: string
  memoId: string | null; stepId: string | null; stableHits: number; stableLeaveMs: number
  defaultSnoozeMinutes: number
}

export interface SceneMonitorStatus {
  platformSupported: boolean; enabled: boolean; paused: boolean; captureArmed: boolean
  allowedRuleCount: number; reason: string | null
}

export interface CaptureFramePayload {
  captureId: string; pngBase64: string; processName: string; windowTitle: string; width: number; height: number
}

export interface SceneTestResult {
  ruleId: string | null; passed: boolean; windowAllowed: boolean; imageScore: number | null
  imageThreshold: number | null; ocrConditionsMet: boolean | null; reasonCode: string; explanation: string; elapsedMs: number
}

export interface SceneNoteItem {
  ruleId: string; headline: string; bodyMd: string; memoId: string | null; stepId: string | null; snoozedUntil: string | null
}

export interface SceneEvent {
  id: string; ruleId: string | null; occurredAt: string; eventType: string; resultCode: string
  imageScore: number | null; elapsedMs: number
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  try { return await invokeData<T>(command, args) }
  catch (error) {
    if (error && typeof error === 'object' && 'code' in error && 'message' in error) {
      const backend = error as BackendError
      throw new IpcError(backend.code, backend.message, backend.hint ?? null, error)
    }
    throw new IpcError('internal', typeof error === 'string' ? error : '智能场景操作失败，请重试', null, error)
  }
}

export const sceneRulesList = () => call<SceneRule[]>('scene_rules_list')
export const sceneRuleSave = (input: SceneRuleInput) => call<SceneRule>('scene_rule_save', { input })
export const sceneRuleSetEnabled = (id: string, enabled: boolean) => call<SceneRule>('scene_rule_set_enabled', { id, enabled })
export const sceneRuleDelete = (id: string) => call<void>('scene_rule_delete', { id })
export const sceneMonitorStatus = () => call<SceneMonitorStatus>('scene_monitor_status')
export const sceneMonitorSetEnabled = (enabled: boolean, consent: boolean) => call<SceneMonitorStatus>('scene_monitor_set_enabled', { enabled, consent })
export const sceneMonitorSetPaused = (paused: boolean) => call<SceneMonitorStatus>('scene_monitor_set_paused', { paused })
export const sceneEventsList = () => call<SceneEvent[]>('scene_events_list', { limit: 50 })
export const sceneCaptureArm = () => call<void>('scene_capture_arm')
export const sceneCaptureCancel = () => call<void>('scene_capture_cancel')
export const sceneTestArm = (ruleId: string) => call<void>('scene_test_arm', { ruleId })
export const sceneNoteCurrent = () => call<SceneNoteItem[]>('scene_note_current')
export const sceneNoteAck = (id: string) => call<void>('scene_note_ack', { id })
export const sceneNoteSnooze = (id: string, minutes?: number) => call<void>('scene_note_snooze', { id, minutes })
export const sceneNoteOpenFlow = (id: string) => call<void>('scene_note_open_flow', { id })
