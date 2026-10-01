// @vitest-environment happy-dom
import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { AppearanceSlider } from './AppearanceSlider'

it('拖动时不改变界面几何，松手应用最后一个数值；键盘调整仍立即应用', async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host), commit = vi.fn()
  try {
    function Slider() { const [value, setValue] = useState('1'); return <AppearanceSlider min="0.8" max="1.5" step="0.05" value={value} onPreview={setValue} onCommit={commit} /> }
    await act(async () => root.render(<Slider />))
    const slider = host.querySelector('input')!
    await act(async () => slider.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, pointerId: 1 })))
    for (const value of ['1.1', '1.3', '1.5']) await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(slider, value)
      slider.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(commit).not.toHaveBeenCalled()
    await act(async () => slider.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1 })))
    expect(commit).toHaveBeenCalledExactlyOnceWith('1.5')
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(slider, '1.45')
      slider.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(commit).toHaveBeenLastCalledWith('1.45')
  } finally { await act(async () => root.unmount()); host.remove() }
})
