// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { setCanvasInputActive } from './canvas-input'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
afterEach(() => { Reflect.deleteProperty(window, '__TAURI_INTERNALS__'); vi.clearAllMocks() })

it('串行同步画布键盘范围，清理的 false 不会被先前的 true 反向覆盖', async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  let release!: () => void
  vi.mocked(invoke).mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve }))
  vi.mocked(invoke).mockResolvedValueOnce(undefined)
  const entered = setCanvasInputActive(true)
  const left = setCanvasInputActive(false)
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))
  expect(invoke).toHaveBeenNthCalledWith(1, 'canvas_input_set_active', { active: true })
  release()
  await Promise.all([entered, left])
  expect(invoke).toHaveBeenNthCalledWith(2, 'canvas_input_set_active', { active: false })
})

it('范围同步失败向调用者返回错误，仍允许下一次退出范围', async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  vi.mocked(invoke).mockRejectedValueOnce(new Error('native guard failed')).mockResolvedValueOnce(undefined)
  await expect(setCanvasInputActive(true)).rejects.toThrow('native guard failed')
  await expect(setCanvasInputActive(false)).resolves.toBeUndefined()
  expect(invoke).toHaveBeenLastCalledWith('canvas_input_set_active', { active: false })
})
