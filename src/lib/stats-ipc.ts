/**
 * 统计与成长的 IPC 封装（任务书 §7）。
 */

import { invokeData as invoke } from './data-change'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 单日统计 */
export interface DayStat {
  date: string
  planned: number
  completed: number
  plannedDone: number
  due: number
  overdue: number
  actualMinutes: number
  estimatedMinutes: number
}

/** 分类占比 */
export interface CategoryShare {
  categoryId: string | null
  name: string
  color: string | null
  count: number
}

/** 项目分布 */
export interface ProjectShare {
  projectId: string | null
  name: string
  color: string | null
  count: number
}

/** 连续天数 */
export interface StreakInfo {
  activeStreak: number
  longestActiveStreak: number
  perfectStreak: number
  /** 说明文字，必须展示给用户以免误读 */
  note: string
}

/** 周期统计 */
export interface PeriodStats {
  /** 口径说明（分母、时间范围、重复实例计数方式） */
  scope: string
  days: number
  startDate: string
  endDate: string
  daily: DayStat[]
  plannedTotal: number
  dueTotal: number
  completedTotal: number
  /** 分母为 0 时为 null，而不是 0% */
  completionRate: number | null
  overdueTotal: number
  overdueRate: number | null
  estimatedMinutes: number
  actualMinutes: number
  /** 实际/预计；样本不足时为 null */
  estimateRatio: number | null
  avgMinutesPerTask: number | null
  byCategory: CategoryShare[]
  byProject: ProjectShare[]
  streak: StreakInfo
  createdTotal: number
}

/** 等级信息 */
export interface LevelInfo {
  level: number
  title: string
  xpInLevel: number
  xpForNext: number
  totalXp: number
  progress: number
}

/** 成就 */
export interface Achievement {
  id: string
  name: string
  description: string
  achieved: boolean
  progress: number
  target: number
}

/** 成长总览 */
export interface GrowthOverview {
  enabled: boolean
  level: LevelInfo
  achievements: Achievement[]
  todayDone: number
  dailyGoal: number
  weekDone: number
  weeklyGoal: number
}

/** 成长设置 */
export interface GrowthConfig {
  gamificationEnabled: boolean
  dailyGoal: number
  weeklyGoal: number
  showStreak: boolean
}

/** 个人目标 */
export interface Goal {
  id: string
  title: string
  targetCount: number
  dueDate: string | null
  createdAt: string
}

/** 目标进度 */
export interface GoalProgress extends Goal {
  doneCount: number
  percent: number
  achieved: boolean
}

export const STATS_CMD = {
  period: 'stats_period',
  growth: 'stats_growth',
  growthGet: 'growth_get_config',
  growthSet: 'growth_set_config',
  goalsList: 'goals_list',
  goalCreate: 'goal_create',
  goalDelete: 'goal_delete',
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

export const statsPeriod = (days: number): Promise<PeriodStats> =>
  call(STATS_CMD.period, { days })

export const statsGrowth = (): Promise<GrowthOverview> => call(STATS_CMD.growth)

export const growthGetConfig = (): Promise<GrowthConfig> => call(STATS_CMD.growthGet)

export const growthSetConfig = (config: GrowthConfig): Promise<GrowthConfig> =>
  call(STATS_CMD.growthSet, { config })

export const goalsList = (): Promise<GoalProgress[]> => call(STATS_CMD.goalsList)

export const goalCreate = (
  title: string,
  targetCount: number,
  dueDate?: string | null,
): Promise<Goal> => call(STATS_CMD.goalCreate, { title, targetCount, dueDate: dueDate ?? null })

export const goalDelete = (id: string): Promise<boolean> => call(STATS_CMD.goalDelete, { id })

// =============================================================================
// 展示辅助（纯函数，便于单测）
// =============================================================================

/** 分钟 → 可读文本 */
export function formatMinutes(m: number | null | undefined): string {
  if (m == null) return '—'
  if (m < 60) return `${Math.round(m)} 分钟`
  const h = Math.floor(m / 60)
  const rest = Math.round(m % 60)
  return rest === 0 ? `${h} 小时` : `${h} 小时 ${rest} 分`
}

/**
 * 百分比展示。
 *
 * `null` 必须显示为"无数据"而不是 0%——"没有安排任务"与
 * "安排了但一项没完成"是两件完全不同的事（§7 要求口径清楚）。
 */
export function formatPercent(v: number | null | undefined): string {
  if (v == null) return '无数据'
  return `${v.toFixed(0)}%`
}

/** 估算偏差的可读描述 */
export function describeEstimateRatio(r: number | null | undefined): {
  text: string
  hint: string
} {
  if (r == null) {
    return { text: '样本不足', hint: '需要至少 3 个「有预计耗时且已完成」的任务才有参考意义' }
  }
  if (r > 1.15) {
    return {
      text: `实际是预计的 ${r.toFixed(2)} 倍`,
      hint: '你倾向于低估耗时，可以考虑把预计时间上调一些',
    }
  }
  if (r < 0.85) {
    return {
      text: `实际是预计的 ${r.toFixed(2)} 倍`,
      hint: '你倾向于高估耗时，预计时间可以更紧一些',
    }
  }
  return { text: `实际与预计接近（${r.toFixed(2)} 倍）`, hint: '估算比较准确' }
}

/** 日期短标签 `MM-DD` */
export function shortDate(iso: string): string {
  const parts = iso.split('-')
  return parts.length === 3 ? `${parts[1]}-${parts[2]}` : iso
}
