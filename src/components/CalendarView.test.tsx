// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { CalendarView } from './CalendarView'

vi.mock('../lib/ipc', () => ({
  IpcError: class extends Error { userMessage() { return this.message } },
  tasksInRange: vi.fn().mockResolvedValue({ rows: [], total: 0, truncated: false }),
  rescheduleTask: vi.fn(),
}))
vi.mock('../lib/recurrence-ipc', () => ({ recurringEnsureRange: vi.fn().mockResolvedValue(undefined) }))
vi.mock('../lib/data-change', () => ({ onDataChanged: vi.fn(() => () => {}) }))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

let root: Root | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

it('月历任意日期都能直接新建，点击加号不切换日期视图', async () => {
  const onCreateTask = vi.fn()
  const onDateChange = vi.fn()
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  const now = new Date()
  const expectedDate = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
  const label = `在 ${now.getMonth() + 1} 月 ${now.getDate()} 日新建任务`

  await act(async () => {
    root!.render(<CalendarView onCreateTask={onCreateTask} onDateChange={onDateChange} />)
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
  onDateChange.mockClear()
  const button = host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)
  expect(button).not.toBeNull()
  await act(async () => button!.click())

  expect(onCreateTask).toHaveBeenCalledWith(expectedDate)
  expect(onDateChange).not.toHaveBeenCalled()
})
