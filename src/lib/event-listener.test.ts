import { expect, it, vi } from 'vitest'
import { listen, type Event, type EventCallback } from '@tauri-apps/api/event'
import { listenEvent } from './event-listener'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
it('未完成注册的旧监听不再派发事件，迟到的注册立即释放', async () => {
  let deliver!: EventCallback<unknown>
  let resolve!: (off: () => void) => void
  vi.mocked(listen).mockImplementation((_name, callback) => {
    deliver = callback
    return new Promise(r => { resolve = r })
  })
  const callback = vi.fn()
  const off = vi.fn()
  const stop = listenEvent('audit-event', callback)
  deliver({ payload: 'current' } as Event<unknown>)
  expect(callback).toHaveBeenCalledOnce()
  stop()
  deliver({ payload: 'obsolete' } as Event<unknown>)
  resolve(off)
  await Promise.resolve()
  expect(callback).toHaveBeenCalledOnce()
  expect(off).toHaveBeenCalledOnce()
})
it('监听注册失败向在场消费者报告，离开后不再更新错误', async () => {
  vi.mocked(listen).mockRejectedValue(new Error('事件注册失败'))
  const report = vi.fn()
  listenEvent('active', () => {}, report)
  const stop = listenEvent('disposed', () => {}, report)
  stop()
  await Promise.resolve()
  await Promise.resolve()
  expect(report).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ message: '事件注册失败' }))
})
