// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { ImageViewer } from './ImageViewer'
import type { ContentAsset } from '../lib/content-assets'
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => { act(() => root?.unmount()); root = undefined; document.body.innerHTML = ''; vi.restoreAllMocks() })
it('文字点击位置后就地输入，完成可拖动、等比缩放，双击重新编辑且撤销恢复', async () => {
  const asset: ContentAsset = { id: crypto.randomUUID(), name: 'image.png', mime: 'image/png', byteSize: 0, dataBase64: '', sha256: '', createdAt: '' }
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<ImageViewer asset={asset} url="blob:test" caption="" onClose={() => {}} />))
  const img = document.querySelector<HTMLImageElement>('.image-viewer__image img')!
  Object.defineProperties(img, { naturalWidth: { value: 500 }, naturalHeight: { value: 300 } })
  await act(async () => img.dispatchEvent(new Event('load')))
  const svg = document.querySelector<SVGSVGElement>('svg')!
  vi.spyOn(svg, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 500, height: 300, right: 500, bottom: 300, x: 0, y: 0, toJSON() {} })
  svg.setPointerCapture = vi.fn(); svg.releasePointerCapture = vi.fn()
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '文字')!.click())
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerdown', { button: 0, pointerId: 1, clientX: 20, clientY: 30, bubbles: true })))
  const editor = document.querySelector<HTMLTextAreaElement>('[aria-label="就地编辑批注文字"]')
  expect(editor).not.toBeNull()
  expect(editor!.closest('foreignObject')?.getAttribute('x')).toBe('20')
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(editor, '发货位置'); editor!.dispatchEvent(new Event('input', { bubbles: true })) })
  await act(async () => editor!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true })))
  expect(svg.querySelector('text')?.textContent).toBe('发货位置')
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerdown', { button: 0, pointerId: 2, clientX: 25, clientY: 35, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointermove', { pointerId: 2, clientX: 65, clientY: 75, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerup', { pointerId: 2, bubbles: true })))
  expect(svg.querySelector('text')?.getAttribute('x')).toBe('60')
  const handle = document.querySelector<SVGRectElement>('[aria-label="等比缩放文字"]')
  expect(handle).not.toBeNull()
  handle!.setPointerCapture = vi.fn(); handle!.releasePointerCapture = vi.fn()
  const before = Number(svg.querySelector('text')!.getAttribute('font-size'))
  await act(async () => handle!.dispatchEvent(new PointerEvent('pointerdown', { button: 0, pointerId: 3, clientX: 160, clientY: 110, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointermove', { pointerId: 3, clientX: 260, clientY: 160, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerup', { pointerId: 3, bubbles: true })))
  expect(Number(svg.querySelector('text')!.getAttribute('font-size'))).toBeGreaterThan(before)
  await act(async () => svg.dispatchEvent(new MouseEvent('dblclick', { clientX: 65, clientY: 75, bubbles: true })))
  expect((document.querySelector('[aria-label="就地编辑批注文字"]') as HTMLTextAreaElement).value).toBe('发货位置')
  await act(async () => document.querySelector('[aria-label="就地编辑批注文字"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '撤销')!.click())
  expect(Number(svg.querySelector('text')!.getAttribute('font-size'))).toBe(before)
  expect(svg.querySelector('text')!.getAttribute('x')).toBe('60')
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '撤销')!.click())
  expect(svg.querySelector('text')!.getAttribute('x')).toBe('20')
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '撤销')!.click())
  expect(svg.querySelector('text')).toBeNull()
})
