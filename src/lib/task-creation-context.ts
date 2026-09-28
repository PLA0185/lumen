import { addDays, format } from 'date-fns'
import type { PeriodType, ViewId } from './types'

export interface TaskCreationContext {
  label: string
  projectId?: string
  categoryId?: string
  tagIds?: string[]
}

export function creationDefaults(view: ViewId, calendarDate: string, now = new Date()): {
  plannedDate?: string
  periodType: PeriodType
} {
  const periods: Partial<Record<ViewId, PeriodType>> = {
    'period-week': 'week', 'period-month': 'month', 'period-quarter': 'quarter', 'period-year': 'year',
  }
  return {
    plannedDate: view === 'tomorrow' ? format(addDays(now, 1), 'yyyy-MM-dd')
      : view === 'today' || view === 'week' ? format(now, 'yyyy-MM-dd')
        : view === 'calendar' ? calendarDate : undefined,
    periodType: periods[view] ?? 'none',
  }
}
