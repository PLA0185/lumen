// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { IpcError } from '../lib/ipc'
import * as rec from '../lib/recurrence-ipc'
import { SeriesRuleDialog } from './SeriesRuleDialog'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

let root: Root | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

it('读取重复规则失败后退出加载状态并显示原因', async () => {
  const error = '起始年份没有已公布的节假日日历'
  vi.spyOn(rec, 'recurringGet').mockRejectedValue(
    new IpcError('validation', error, '已内置 2025–2026 年。', null),
  )
  vi.spyOn(rec, 'recurringScopeInfo').mockResolvedValue({
    isRecurring: true,
    totalInstances: 1,
    completed: 0,
    completedBefore: 0,
  } as rec.ScopeInfo)

  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => {
    root!.render(
      <SeriesRuleDialog
        taskId="task-1"
        seriesId="series-1"
        occurrenceKey="2026-11-04T00:00:00.000Z"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    await Promise.resolve()
    await Promise.resolve()
  })

  expect(host.querySelector('[role="alert"]')?.textContent).toContain(error)
  expect(host.textContent).not.toContain('正在读取重复规则…')
  expect(host.textContent).toContain('重试读取')
})
