// @vitest-environment happy-dom
import { expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { rescheduleTask } from './ipc'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn().mockResolvedValue(undefined) }))

it('日历改期同时传递本地日期的 UTC 和当前 IANA 时区', async () => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
  vi.mocked(invoke).mockResolvedValue({ id: 'task' })
  await rescheduleTask('task', '2026-10-04T16:00:00.000Z')
  expect(invoke).toHaveBeenCalledWith('task_reschedule', {
    id: 'task',
    newDateUtc: '2026-10-04T16:00:00.000Z',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  })
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__')
})
