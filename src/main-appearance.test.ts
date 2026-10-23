// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'

const native = vi.hoisted(() => ({
  zoom: vi.fn().mockResolvedValue(undefined),
  resize: undefined as ((event: { payload: { width: number; height: number } }) => void) | undefined,
  dpi: undefined as ((event: { payload: { scaleFactor: number; size: { width: number; height: number } } }) => void) | undefined,
}))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom: native.zoom }) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({
  label: 'main',
  innerSize: async () => ({ width: 1400, height: 900 }),
  scaleFactor: async () => 1.25,
  onResized: async (callback: typeof native.resize) => { native.resize = callback; return () => {} },
  onScaleChanged: async (callback: typeof native.dpi) => { native.dpi = callback; return () => {} },
}) }))
vi.mock('react-dom/client', () => ({ default: { createRoot: () => ({ render: vi.fn() }) } }))
vi.mock('./App', () => ({ default: () => null }))
vi.mock('./components/FloatingToday', () => ({ FloatingToday: () => null, QuickAddWindow: () => null }))
vi.mock('./lib/fresh-paste', () => ({ installFreshPaste: () => () => {} }))
const listeners = vi.spyOn(window, 'addEventListener')
afterEach(() => {
  for (const [type, listener] of listeners.mock.calls) window.removeEventListener(type, listener)
  document.body.innerHTML = ''
  localStorage.clear()
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
})

it('启动应用只应用用户缩放，窗口尺寸和系统 DPI 由响应式布局处理', async () => {
  document.body.innerHTML = '<div id="root"></div>'
  Object.assign(window, { __TAURI_INTERNALS__: { metadata: { currentWebview: { label: 'main' } } } })
  localStorage.setItem('lumen.uiScale', '0.8')
  localStorage.setItem('lumen.fontSize', '20px')
  await import('./main')
  await vi.waitFor(() => expect(native.zoom).toHaveBeenLastCalledWith(0.8))
  expect(document.documentElement.style.getPropertyValue('--font-size-base')).toBe('20px')
  expect(native.resize).toBeUndefined()
  expect(native.dpi).toBeUndefined()
  window.dispatchEvent(new Event('resize'))
  await new Promise(resolve => setTimeout(resolve, 0))
  await vi.waitFor(() => expect(native.zoom).toHaveBeenLastCalledWith(0.8))
  localStorage.setItem('lumen.uiScale', '1.5')
  window.dispatchEvent(new StorageEvent('storage', { key: 'lumen.uiScale' }))
  await vi.waitFor(() => expect(native.zoom).toHaveBeenLastCalledWith(1.5))
})
