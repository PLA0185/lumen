import { describe, expect, it } from 'vitest'
import { creationDefaults } from './task-creation-context'
import { buildQuery } from './store'
import { combineDateTime } from './datetime'
import type { ViewId } from './types'

describe('新建任务保留当前视图归属', () => {
  const now = new Date(2026, 8, 28, 12)
  it('日期视图默认日期落在对应列表筛选区间内', () => {
    for (const view of ['today', 'tomorrow', 'week'] as ViewId[]) {
      // buildQuery 使用真实当前日期；创建默认值也使用相同时间。
      const defaults = creationDefaults(view, '2026-10-08')
      const query = buildQuery({view, search:'', statusFilter:[], sortBy:'manual', sortDesc:false, overdueOnly:false})
      const date = combineDateTime(defaults.plannedDate!, '')!.utc
      expect(date >= query.plannedFrom! && date <= query.plannedTo!).toBe(true)
    }
    expect(creationDefaults('calendar', '2026-10-08', now).plannedDate).toBe('2026-10-08')
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
