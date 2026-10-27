import { describe, expect, it } from 'vitest'
import { creationDefaults } from './task-creation-context'
import { buildQuery } from './store'
import { combineDateTime } from './datetime'
import { dateRange } from './datetime'
import type { ViewId } from './types'

describe('新建任务保留当前视图归属', () => {
  const now = new Date(2026, 8, 28, 12)
  it('日期视图默认日期落在对应列表筛选区间内', () => {
    const today = new Date()
    const selectedDate = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`
    for (const view of ['today', 'tomorrow', 'week'] as ViewId[]) {
      const defaults = creationDefaults(view, selectedDate)
      const query = buildQuery({view, todayDate:selectedDate, search:'', statusFilter:[], sortBy:'manual', sortDesc:false, overdueOnly:false})
      const date = combineDateTime(defaults.plannedDate!, '')!.utc
      expect(date >= query.plannedFrom! && date <= query.plannedTo!).toBe(true)
    }
    expect(creationDefaults('calendar', '2026-10-08', now).plannedDate).toBe('2026-10-08')
  })
  it('今天页选中历史日期后，查询和新建任务都使用所选日期', () => {
    const selectedDate = '2026-09-30'
    const query = buildQuery({view:'today', todayDate:selectedDate, search:'', statusFilter:[], sortBy:'manual', sortDesc:false, overdueOnly:false})
    expect(query.plannedFrom).toBe(dateRange(selectedDate).start)
    expect(query.plannedTo).toBe(dateRange(selectedDate).end)
    expect(creationDefaults('today', selectedDate).plannedDate).toBe(selectedDate)
  })
  it('选中周二时查询包含该周的周期任务范围', () => {
    const query = buildQuery({view:'today', todayDate:'2026-10-27', search:'', statusFilter:[], sortBy:'manual', sortDesc:false, overdueOnly:false}) as ReturnType<typeof buildQuery> & { periodDay?: { weekFrom: string; weekTo: string } }
    expect(query.periodDay?.weekFrom).toBe(dateRange('2026-10-26').start)
    expect(query.periodDay?.weekTo).toBe(dateRange('2026-11-01').end)
  })
  it('周期归属与当前视图的查询条件一致，收件箱不强加日期', () => {
    for (const view of ['period-week', 'period-month', 'period-quarter', 'period-year'] as ViewId[]) {
      const defaults = creationDefaults(view, '', now)
      const query = buildQuery({view, search:'', statusFilter:[], sortBy:'manual', sortDesc:false, overdueOnly:false})
      expect(query.periodTypes).toContain(defaults.periodType)
      expect(defaults.plannedDate).toBeUndefined()
    }
    expect(creationDefaults('inbox', '', now)).toEqual({plannedDate:undefined, periodType:'none'})
  })
})
