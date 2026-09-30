// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { listen } from '@tauri-apps/api/event'
import * as win from '../lib/window-ipc'
import { WindowSettings } from './WindowSettings'

vi.mock('@tauri-apps/plugin-autostart', () => ({
  isEnabled: vi.fn().mockResolvedValue(false),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})
const config: win.WindowConfig = {
  mainAlwaysOnTop: false,
  mainShowInTaskbar: true,
  closeAction: 'tray',
  floatingEnabled: false,
  floatingAlwaysOnTop: true,
  floatingClickThrough: false,
  floatingOpacity: 1,
  floatingShowInTaskbar: false,
  floatingX: null,
  floatingY: null,
  floatingWidth: 320,
  floatingHeight: 460,
  trayEnabled: true,
  shortcutEnabled: true,
  shortcutToggle: 'Ctrl+Alt+L',
  shortcutQuickAdd: 'Ctrl+Alt+N',
  shortcutToday: 'Ctrl+Alt+T',
  shortcutFloating: 'CmdOrCtrl+Alt+Q',
}
async function mount() {
  vi.spyOn(win, 'windowGetConfig').mockResolvedValue({
    config,
    opacityMin: 0.2,
    hasRecoveryPath: true,
    defaultFloatingSize: { width: 480, height: 620 },
  } as win.WindowConfigState)
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<WindowSettings />))
}
it('恢复默认大小使用后端默认值，并且只写一次窗口尺寸', async () => {
  const size = vi
    .spyOn(win, 'windowSetFloatingSize')
    .mockResolvedValue({ width: 480, height: 620 })
  const setConfig = vi.spyOn(win, 'windowSetConfig').mockResolvedValue(config)
  await mount()
  const button = [...document.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === '恢复默认大小',
  )!
  await act(async () => button.click())
  expect(size).toHaveBeenCalledExactlyOnceWith(480, 620)
  expect(setConfig).not.toHaveBeenCalled()
  expect(document.body.textContent).toContain('当前 480 × 620')
})
it('页面在异步监听注册完成前离开，也释放迟到的监听', async () => {
  let resolve!: (off: () => void) => void
  const off = vi.fn()
  vi.mocked(listen).mockReturnValueOnce(
    new Promise((r) => {
      resolve = r
    }),
  )
  await mount()
  act(() => root!.unmount())
  root = undefined
  await act(async () => resolve(off))
  expect(off).toHaveBeenCalledOnce()
})
it('窗口设置提供独立的呼出悬浮窗快捷键并保存更改', async () => {
  const setConfig = vi.spyOn(win, 'windowSetConfig').mockImplementation(async (value) => value)
  await mount()
  const select = document.querySelector<HTMLSelectElement>('[aria-label="显示 / 隐藏悬浮窗快捷键"]')
  expect(select).not.toBeNull()
  await act(async () => {
    select!.value = 'Alt+Shift+A'
    select!.dispatchEvent(new Event('change', { bubbles: true }))
  })
  expect(setConfig).toHaveBeenCalledExactlyOnceWith({ ...config, shortcutFloating: 'Alt+Shift+A' })
})
