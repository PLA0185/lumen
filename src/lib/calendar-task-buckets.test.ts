import { describe, expect, it } from 'vitest'
import { calendarTaskBuckets } from './calendar-task-buckets'
import type { Task } from './types'

const baseTask = {
  id: 'period-task',
  title: '周期任务',
  description: '',
  status: 'todo',
  plannedAt: null,
  dueAt: null,
  completedAt: null,
  createdAt: '2026-10-19T04:00:00.000Z',
  updatedAt: '2026-10-19T04:00:00.000Z',
  periodType: 'week',
} as Task

function day(year: number, month: number, date: number) {
  return new Date(year, month - 1, date, 12)
}

function key(year: number, month: number, date: number) {
  return `${year}-${String(month).padStart(2, '0')}-${String(date).padStart(2, '0')}`
}

describe('日历周期任务', () => {
  it.each([
    ['week', day(2026, 10, 19), day(2026, 10, 25), day(2026, 10, 26)],
    ['month', day(2026, 10, 1), day(2026, 10, 31), day(2026, 11, 1)],
    ['quarter', day(2026, 10, 1), day(2026, 12, 31), day(2027, 1, 1)],
    ['year', day(2026, 1, 1), day(2026, 12, 31), day(2027, 1, 1)],
  ] as const)('%s跨度任务在所属周期每天显示，周期外隐藏', (periodType, firstDay, lastDay, outsideDay) => {
    const task = {
      ...baseTask,
      periodType,
      plannedAt: new Date(2026, 9, 19, 12).toISOString(),
    } as Task
    const buckets = calendarTaskBuckets([task], [firstDay, lastDay, outsideDay])
    expect(buckets.get(key(firstDay.getFullYear(), firstDay.getMonth() + 1, firstDay.getDate()))).toHaveLength(1)
    expect(buckets.get(key(lastDay.getFullYear(), lastDay.getMonth() + 1, lastDay.getDate()))).toHaveLength(1)
    expect(buckets.has(key(outsideDay.getFullYear(), outsideDay.getMonth() + 1, outsideDay.getDate()))).toBe(false)
  })

  it('无计划和截止日期时以创建日为起点，不补到创建之前', () => {
    const task = { ...baseTask, createdAt: new Date(2026, 9, 21, 12).toISOString() }
    const buckets = calendarTaskBuckets([task], [day(2026, 10, 20), day(2026, 10, 21), day(2026, 10, 25)])
    expect(buckets.has('2026-10-20')).toBe(false)
    expect(buckets.has('2026-10-21')).toBe(true)
    expect(buckets.has('2026-10-25')).toBe(true)
  })

  it('完成任务在完成日显示已完成，之前显示待办，之后隐藏', () => {
    const task = {
      ...baseTask,
      plannedAt: new Date(2026, 9, 19, 12).toISOString(),
      status: 'done',
      completedAt: new Date(2026, 9, 21, 15).toISOString(),
    } as Task
    const buckets = calendarTaskBuckets([task], [day(2026, 10, 19), day(2026, 10, 20), day(2026, 10, 21), day(2026, 10, 22)])
    expect(buckets.get('2026-10-19')?.[0]?.status).toBe('todo')
    expect(buckets.get('2026-10-20')?.[0]?.status).toBe('todo')
    expect(buckets.get('2026-10-21')?.[0]?.status).toBe('done')
    expect(buckets.has('2026-10-22')).toBe(false)
  })

  it('普通指定日期任务仍只出现在计划日', () => {
    const task = { ...baseTask, periodType: 'none', plannedAt: new Date(2026, 9, 21, 12).toISOString() } as Task
    const buckets = calendarTaskBuckets([task], [day(2026, 10, 20), day(2026, 10, 21), day(2026, 10, 22)])
    expect([...buckets.keys()]).toEqual(['2026-10-21'])
  })
})
