import { describe, expect, it } from 'vitest'
import { buildRrule } from '../lib/recurrence-ipc'
import { parseRrule } from './RuleEditor'

describe('editing an existing recurrence rule', () => {
  it('keeps COUNT and the monthly last-day rule on an unchanged save', () => {
    const parsed = parseRrule('FREQ=MONTHLY;INTERVAL=2;BYMONTHDAY=-1;COUNT=8')
    expect(parsed.end).toEqual({ kind: 'count', count: 8 })
    expect(parsed.byMonthday).toEqual([-1])
    expect(buildRrule(parsed)).toBe('FREQ=MONTHLY;INTERVAL=2;BYMONTHDAY=-1;COUNT=8')
  })

  it('keeps UNTIL and a yearly month/day on an unchanged save', () => {
    const parsed = parseRrule('FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=31;UNTIL=20301231')
    expect(parsed.end).toEqual({ kind: 'until', date: '2030-12-31' })
    expect(parsed.byMonth).toEqual([3])
    expect(buildRrule(parsed)).toBe('FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=31;UNTIL=20301231')
  })
})
