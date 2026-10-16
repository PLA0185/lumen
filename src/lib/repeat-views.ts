import type { ViewId } from './types'

export const REPEAT_VIEWS = [
  { id: 'recurring-daily', label: '每日', title: '每日重复' },
  { id: 'recurring-weekly', label: '每周', title: '每周重复' },
  { id: 'recurring-monthly', label: '每月', title: '每月重复' },
  { id: 'recurring-quarterly', label: '每季度', title: '每季度重复' },
  { id: 'recurring-yearly', label: '每年', title: '每年重复' },
] as const satisfies readonly { id: ViewId; label: string; title: string }[]

export type RepeatViewId = (typeof REPEAT_VIEWS)[number]['id']

export function isRepeatView(view: ViewId): view is RepeatViewId {
  return REPEAT_VIEWS.some((item) => item.id === view)
}
