// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as ipc from '../lib/ipc'
import { TodayDateSelector } from './TodayDateSelector'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => { act(() => root?.unmount()); root = undefined; vi.restoreAllMocks(); document.body.innerHTML = '' })

it('显示所选日期所在周并可切换到之前一周', async () => {
  vi.spyOn(ipc, 'countTasks').mockResolvedValue({ total: 0 } as Awaited<ReturnType<typeof ipc.countTasks>>)
  const onChange = vi.fn()
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<TodayDateSelector selectedDate="2026-09-30" onChange={onChange} />))
  expect(document.querySelector('[aria-pressed="true"]')?.getAttribute('aria-label')).toContain('周三')
  const previous = document.querySelector('[aria-label="上一周"]') as HTMLButtonElement
  await act(async () => previous.click())
  expect(onChange).toHaveBeenCalledWith('2026-09-23')
  expect(document.body.textContent).toContain('回到今天')
})
