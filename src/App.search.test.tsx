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
const originalUserAgent = navigator.userAgent
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
  Object.defineProperty(navigator, 'userAgent', { configurable: true, value: originalUserAgent })
})
async function mountApp() {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<App />))
  return host
}
async function mount() {
  await mountApp()
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

it('Android 当前页面属于更多导航时，底部的更多入口也保持选中状态', async () => {
  Object.defineProperty(navigator, 'userAgent', { configurable: true, value: 'Mozilla/5.0 (Linux; Android 15)' })
  useApp.setState({ view: 'knowledge' })
  const host = await mountApp()
  const more = host.querySelector<HTMLButtonElement>('[aria-label="更多导航"]')

  expect(more).not.toBeNull()
  expect(more?.getAttribute('aria-current')).toBe('page')
  expect(more?.classList.contains('is-active')).toBe(true)
})
