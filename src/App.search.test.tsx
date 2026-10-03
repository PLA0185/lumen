// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from './App'
import { useApp } from './lib/store'

vi.mock('./components/AiAssistant', () => ({ AiAssistant: () => null }))
vi.mock('./lib/data-change', () => ({ onDataChanged: () => () => {} }))
vi.mock('./lib/event-listener', () => ({ listenEvent: () => () => {} }))
vi.mock('./lib/window-ipc', () => ({ windowFloatingState: async () => ({ enabled: false }) }))
vi.mock('./lib/recurrence-ipc', () => ({ recurringEnsureRange: async () => {} }))

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
const reload = vi.fn(async () => {})
beforeEach(() => {
  vi.clearAllMocks()
  useApp.setState({ view: 'all', search: '输入中的文字', tasks: [], progressMap: {},
    loadState: 'ready', totalCount: 0, hasMore: false, loadingMore: false, toasts: [],
    init: async () => {}, reload })
})
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  document.body.innerHTML = ''
})
async function mount() {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<App />))
  return document.querySelector<HTMLInputElement>('[aria-label="搜索任务"]')!
}

it.each([{ isComposing: true }, { keyCode: 229 }])('搜索输入法确认候选 Enter 不查询旧文字：%j', async options => {
  const search = await mount()
  await act(async () => search.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'Enter', bubbles: true, cancelable: true, ...options,
  })))
  expect(reload).not.toHaveBeenCalled()
  expect(search.value).toBe('输入中的文字')
  await act(async () => search.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'Enter', bubbles: true, cancelable: true,
  })))
  expect(reload).toHaveBeenCalledOnce()
})
