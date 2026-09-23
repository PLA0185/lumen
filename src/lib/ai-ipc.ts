/**
 * AI 功能的 IPC 封装（任务书 §6）。
 *
 * 注意 apply 的语义：必须带上 generate 时返回的 `previewId`，
 * 且**可以只接受部分条目**（acceptIndices）。这是"用户选择接受、
 * 修改或放弃"里"接受一部分"的落点。
 */

import { invoke } from '@tauri-apps/api/core'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 提供商 */
export type AiProvider = 'deep_seek' | 'open_ai' | 'claude' | 'custom'

/** 提供商配置（**不含密钥**） */
export interface ProviderConfig {
  provider: AiProvider
  baseUrl: string
  model: string
  timeoutSeconds: number
  maxOutputTokens: number
  /** 只表示"是否已保存密钥"，密钥本身在系统凭据管理器里 */
  hasApiKey: boolean
}

/** token 用量 */
export interface TokenUsage {
  inputTokens: number | null
  outputTokens: number | null
  cacheHitTokens: number | null
  cacheMissTokens: number | null
}

/** 差异项的动作 */
export type DiffAction = 'create' | 'update' | 'reschedule'

/** 字段改动 */
export interface FieldChange {
  field: string
  label: string
  before: string | null
  after: string | null
}

/** 差异项 */
export interface DiffItem {
  action: DiffAction
  taskId: string | null
  title: string
  changes: FieldChange[]
  note: string | null
}

/** 校验问题 */
export interface ValidationIssue {
  level: 'error' | 'warning'
  /** 0 表示整体问题，否则是从 1 开始的下标 */
  index: number
  message: string
}

/** 差异预览 */
export interface DiffPreview {
  /** 确认写入时必须回传 */
  previewId: string
  capability: string
  summary: string
  items: DiffItem[]
  issues: ValidationIssue[]
  /** 有 error 级问题时为 false，界面应禁用"接受" */
  acceptable: boolean
  raw: string
  usage: TokenUsage | null
  /** 本次向模型发送了什么（§6 要求说明数据范围） */
  dataScopeNote: string
}

/** 应用结果 */
export interface ApplyResult {
  created: number
  updated: number
  skipped: number
}

/** 复盘结果（纯只读） */
export interface ReviewResult {
  text: string
  summary: string
  stats: unknown
  usage: TokenUsage | null
  dataScopeNote: string
}

/** AI 状态 */
export interface AiStatus {
  configured: boolean
  provider: AiProvider | null
  model: string | null
  note: string
}

/** 模型列表查询结果 */
export interface ModelListResult {
  ok: boolean
  models: string[]
  reason?: string
  hint?: string | null
  fallback?: string
}

export const AI_CMD = {
  getConfig: 'ai_get_config',
  setConfig: 'ai_set_config',
  test: 'ai_test_connection',
  models: 'ai_list_models',
  clearKey: 'ai_clear_key',
  status: 'ai_status',
  organize: 'ai_organize',
  breakdown: 'ai_breakdown',
  plan: 'ai_plan',
  review: 'ai_review',
  apply: 'ai_apply',
  discard: 'ai_discard',
  conflicts: 'schedule_conflicts',
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

export const aiGetConfig = (): Promise<ProviderConfig | null> => call(AI_CMD.getConfig)

/** 保存配置。`apiKey` 为 null 表示不改动已保存的密钥，空字符串表示清除。 */
export const aiSetConfig = (
  config: ProviderConfig,
  apiKey?: string | null,
): Promise<ProviderConfig> =>
  call(AI_CMD.setConfig, { config, apiKey: apiKey ?? null })

export const aiTestConnection = (config: ProviderConfig): Promise<string> =>
  call(AI_CMD.test, { config })

export const aiListModels = (config: ProviderConfig): Promise<ModelListResult> =>
  call(AI_CMD.models, { config })

export const aiClearKey = (provider: AiProvider): Promise<boolean> =>
  call(AI_CMD.clearKey, { provider })

export const aiStatus = (): Promise<AiStatus> => call(AI_CMD.status)

// ------------------------- 四项能力（都只返回预览） -------------------------

export const aiOrganize = (
  config: ProviderConfig,
  text: string,
  sendNotes = false,
): Promise<DiffPreview> => call(AI_CMD.organize, { config, input: { text, sendNotes } })

export const aiBreakdown = (
  config: ProviderConfig,
  taskId: string,
  asSubtasks = true,
): Promise<DiffPreview> =>
  call(AI_CMD.breakdown, { config, input: { taskId, asSubtasks } })

export const aiPlan = (
  config: ProviderConfig,
  horizon: 'daily' | 'weekly',
  minutesPerDay: number,
  taskIds?: string[],
  sendNotes = false,
): Promise<DiffPreview> =>
  call(AI_CMD.plan, {
    config,
    input: { horizon, minutesPerDay, taskIds: taskIds ?? null, sendNotes },
  })

export const aiReview = (
  config: ProviderConfig,
  horizon: 'daily' | 'weekly',
): Promise<ReviewResult> => call(AI_CMD.review, { config, input: { horizon } })

/**
 * 确认写入。
 *
 * `acceptIndices` 为 null 表示全部接受；否则只写入被勾选的条目。
 * `previewId` 必须来自对应的 generate 调用——预览是一次性的，
 * 重复调用会因为找不到预览而失败（这是刻意的防重复写入设计）。
 */
export const aiApply = (
  previewId: string,
  acceptIndices?: number[] | null,
): Promise<ApplyResult> => call(AI_CMD.apply, { previewId, acceptIndices: acceptIndices ?? null })

/** 放弃预览 */
export const aiDiscard = (previewId: string): Promise<boolean> =>
  call(AI_CMD.discard, { previewId })

/** 排程冲突（纯规则计算，不依赖 AI） */
export const scheduleConflicts = (): Promise<
  Array<{ day: string; kind: string; message: string }>
> => call(AI_CMD.conflicts)

// =============================================================================
// 展示辅助（纯函数）
// =============================================================================

/** 提供商显示名 */
export const PROVIDER_LABELS: Record<AiProvider, string> = {
  deep_seek: 'DeepSeek',
  open_ai: 'OpenAI',
  claude: 'Anthropic Claude',
  custom: '自定义兼容服务',
}

/** 默认 Base URL（与后端保持一致） */
export const PROVIDER_DEFAULT_BASE: Record<AiProvider, string> = {
  deep_seek: 'https://api.deepseek.com',
  open_ai: 'https://api.openai.com/v1',
  claude: 'https://api.anthropic.com',
  custom: '',
}

/** 默认模型（调研时官方文档列出的 ID） */
export const PROVIDER_DEFAULT_MODEL: Record<AiProvider, string> = {
  deep_seek: 'deepseek-flash',
  open_ai: 'gpt-6-astra',
  claude: 'claude-sonnet-5',
  custom: '',
}

/** 动作的显示文案 */
export const ACTION_LABELS: Record<DiffAction, string> = {
  create: '新建',
  update: '修改',
  reschedule: '改期',
}

/** 统计错误与警告数量 */
export function countIssues(issues: ValidationIssue[]): { errors: number; warnings: number } {
  let errors = 0
  let warnings = 0
  for (const i of issues) {
    if (i.level === 'error') errors += 1
    else warnings += 1
  }
  return { errors, warnings }
}

/**
 * 判断能否接受。
 *
 * 双重条件：预览自身声明可接受，且当前没有被勾选的条目也不能提交。
 * 后端已给出 acceptable，这里再结合用户的勾选状态判断。
 */
export function canApply(
  preview: DiffPreview | null,
  selectedCount: number,
): { ok: boolean; reason?: string } {
  if (!preview) return { ok: false, reason: '没有预览内容' }
  if (!preview.acceptable) {
    const { errors } = countIssues(preview.issues)
    return {
      ok: false,
      reason:
        errors > 0
          ? `有 ${errors} 处校验未通过，无法写入。可以重新生成或换用其它模型`
          : '没有可写入的内容',
    }
  }
  if (selectedCount === 0) return { ok: false, reason: '请至少勾选一条要接受的改动' }
  return { ok: true }
}
