// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { FitToolbar } from './FitToolbar'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
it('按原始菜单宽度等比缩放，窗口变窄仍完整保留单行菜单且测量不会受到缩放影响', async () => {
  let width = 800
  let resize!: () => void
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: () => void) { resize = callback }
    observe() {}
    disconnect() {}
  })
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (this: HTMLElement) { return this.classList.contains('memos__toolbar') ? width : 0 })
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) { return this.classList.contains('memos__toolbar-row') ? 1600 : 0 })
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(48)
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host)
  try {
    await act(async () => root.render(<FitToolbar><button>新建流程</button><button>备忘回收站</button><button>刷新列表</button></FitToolbar>))
    const row = host.querySelector<HTMLElement>('.memos__toolbar-row')!
    expect(row.style.transform).toBe('scale(0.5)')
    expect(host.querySelector<HTMLElement>('.memos__toolbar')!.style.height).toBe('40px')
    await act(async () => { resize(); resize() })
    expect(row.style.transform).toBe('scale(0.5)')
    await act(async () => { width = 400; resize() })
    expect(row.style.transform).toBe('scale(0.25)')
    expect([...row.querySelectorAll('button')].map(button => button.textContent)).toEqual(['新建流程', '备忘回收站', '刷新列表'])
  } finally {
    act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals()
  }
})
