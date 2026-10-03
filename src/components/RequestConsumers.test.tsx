// @vitest-environment happy-dom
import { act, StrictMode, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import * as ipc from '../lib/ipc'
import * as org from '../lib/organize-ipc'
import * as focus from '../lib/focus-ipc'
import * as stats from '../lib/stats-ipc'
import * as ai from '../lib/ai-ipc'
import type { Task } from '../lib/types'
import { FocusPanel } from './FocusPanel'
import { StatsView } from './StatsView'
import { DependencyEditor } from './DependencyEditor'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
const period: stats.PeriodStats = {
  scope: '受控统计口径', days: 30, startDate: '2026-09-05', endDate: '2026-10-04', daily: [],
  plannedTotal: 0, dueTotal: 0, completedTotal: 0, completionRate: null,
  overdueTotal: 0, overdueRate: null, estimatedMinutes: 0, actualMinutes: 0,
  estimateRatio: null, avgMinutesPerTask: null, byCategory: [], byProject: [], createdTotal: 0,
  streak: { activeStreak: 0, longestActiveStreak: 0, perfectStreak: 0, note: '无安排不计入连续天数' },
}
beforeEach(() => {
  vi.spyOn(focus, 'focusCurrent').mockResolvedValue(null)
  vi.spyOn(focus, 'focusSummary').mockResolvedValue({ todaySessions: 0, todaySeconds: 0, weekSeconds: 0, activeSession: null })
  vi.spyOn(stats, 'statsPeriod').mockResolvedValue(period)
  vi.spyOn(stats, 'statsGrowth').mockResolvedValue({ enabled: false, achievements: [], todayDone: 0, weekDone: 0, dailyGoal: 10, weeklyGoal: 50,
    level: { level: 1, title: '新手', xpInLevel: 0, xpForNext: 100, totalXp: 0, progress: 0 } })
  vi.spyOn(stats, 'growthGetConfig').mockResolvedValue({ gamificationEnabled: false, dailyGoal: 10, weeklyGoal: 50, showStreak: false })
  vi.spyOn(stats, 'goalsList').mockResolvedValue([])
  vi.spyOn(ai, 'scheduleConflicts').mockResolvedValue([])
  vi.spyOn(org, 'dependencyList').mockResolvedValue([])
  vi.spyOn(org, 'dependencyDependents').mockResolvedValue([])
})
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  vi.useRealTimers()
  document.body.innerHTML = ''
})
async function mount(node: ReactNode) {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<StrictMode>{node}</StrictMode>))
}

it('StrictMode 清理演练后专注读取成功，退出骨架并显示开始操作', async () => {
  await mount(<FocusPanel />)
  expect(focus.focusCurrent).toHaveBeenCalledTimes(2)
  expect(document.querySelector('.skeleton')).toBeNull()
  expect(document.body.textContent).toContain('开始专注')
})

it('StrictMode 清理演练后专注读取失败，退出骨架并显示真实错误', async () => {
  vi.mocked(focus.focusCurrent).mockRejectedValue(new Error('专注读取失败'))
  await mount(<FocusPanel />)
  expect(document.querySelector('.skeleton')).toBeNull()
  expect(document.body.textContent).toContain('专注读取失败')
})

it('StrictMode 清理演练后统计读取成功，渲染后台口径及区间', async () => {
  await mount(<StatsView />)
  expect(stats.statsPeriod).toHaveBeenCalledTimes(2)
  expect(document.querySelector('.skeleton')).toBeNull()
  expect(document.body.textContent).toContain('受控统计口径')
  expect(document.body.textContent).toContain('2026-09-05')
})

it('StrictMode 清理演练后统计读取失败，提供真实错误和重试', async () => {
  vi.mocked(stats.statsPeriod).mockRejectedValue(new Error('统计读取失败'))
  await mount(<StatsView />)
  expect(document.querySelector('.skeleton')).toBeNull()
  expect(document.body.textContent).toContain('统计读取失败')
  expect([...document.querySelectorAll('button')].some(button => button.textContent === '重试')).toBe(true)
})

it('StrictMode 清理演练后前置候选搜索仍应用结果、退出读取中', async () => {
  vi.useFakeTimers()
  vi.spyOn(ipc, 'listTasks').mockResolvedValue([{ id: 'other', title: '前置候选', status: 'todo' } as Task])
  vi.spyOn(ipc, 'countTasks').mockResolvedValue({ total: 1 })
  await mount(<DependencyEditor taskId="current" taskTitle="当前任务" />)
  await act(async () => [...document.querySelectorAll('button')].find(button => button.textContent?.includes('添加前置任务'))!.click())
  await act(async () => vi.advanceTimersByTimeAsync(250))
  expect(ipc.listTasks).toHaveBeenCalledOnce()
  expect(document.querySelector('.depspicker__item')?.textContent).toBe('前置候选')
  expect(document.body.textContent).not.toContain('正在搜索…')
})
