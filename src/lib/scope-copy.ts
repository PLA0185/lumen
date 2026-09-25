import type { ScopeInfo } from './recurrence-ipc'

/** User-facing recurrence summary that separates the plan limit from materialized rows. */
export function recurrenceSeriesSummary(info: ScopeInfo): string {
  const generated = info.totalInstances ?? 0
  if (info.recurrenceEndKind === 'count' && info.recurrenceCount) {
    return `计划共 ${info.recurrenceCount} 次 · 已生成 ${generated} 次`
  }
  if (info.recurrenceEndKind === 'until' && info.recurrenceUntil) {
    return `重复至 ${info.recurrenceUntil.slice(0, 10)} · 已生成 ${generated} 次`
  }
  return `无限重复 · 当前已生成 ${generated} 次`
}
