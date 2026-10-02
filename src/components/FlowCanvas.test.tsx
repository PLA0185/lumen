// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { FlowCanvas } from './FlowCanvas'
import type { FlowStep } from '../lib/memos-ipc'
import * as assets from '../lib/content-assets'
import * as canvasInput from '../lib/canvas-input'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
const steps: FlowStep[] = [
  { id: 'one', title: '核对 ERP 发货单', owner: '', detail: '进入发货模块核对数量' },
  { id: 'two', title: '通知仓库', owner: '运营', detail: '发货单确认后通知仓库' },
]
afterEach(() => { act(() => root?.unmount()); root = undefined; document.body.innerHTML = ''; vi.restoreAllMocks() })
async function mount() {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<FlowCanvas steps={steps} source="" onChange={() => {}} />))
  return document.querySelector('[aria-label="流程画布"]') as HTMLElement
}
async function input(label: string, text: string) {
  const el = document.querySelector(`[aria-label="${label}"]`) as HTMLInputElement
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, text); el.dispatchEvent(new Event('input', { bubbles: true })) })
}
it('超过三步仍从左到右排列，打开时保持可阅读的原始比例', async () => {
  await mount()
  await act(async () => root!.render(<FlowCanvas steps={Array.from({ length: 5 }, (_, i) => ({ ...steps[0]!, id: String(i) }))} source="" onChange={() => {}} />))
  const nodes = [...document.querySelectorAll<HTMLElement>('.flow-canvas__node')]
  expect(nodes.every(node => node.style.top === nodes[0]!.style.top)).toBe(true)
  expect(nodes.every((node, i) => i === 0 || Number.parseFloat(node.style.left) > Number.parseFloat(nodes[i - 1]!.style.left))).toBe(true)
  expect(document.querySelector('[aria-label="画布缩放比例"]')!.textContent).toBe('100%')
})
it('顶部导航显示节点标题，点击直接定位且不强制展开编辑面板', async () => {
  await mount()
  const navigation = document.querySelector('[aria-label="流程节点导航"]')!
  const destination = navigation?.querySelector<HTMLButtonElement>('[aria-label="定位第 2 步：通知仓库"]')
  expect(destination).toBeTruthy()
  await act(async () => destination!.click())
  expect(document.querySelector('.flow-canvas__node[aria-pressed="true"]')?.textContent).toContain('通知仓库')
  expect(destination!.getAttribute('aria-current')).toBe('step')
  expect(document.querySelector('.flow-canvas__inspector')).toBeNull()
})
it('卡片直接显示完整 Markdown 和原图，不靠打开详情才能查看', async () => {
  const image: assets.ContentAsset = { id: '00000000-0000-7000-8000-000000000001', name: '操作.png', mime: 'image/png', byteSize: 3, dataBase64: 'YWJj', sha256: '', createdAt: '' }
  vi.spyOn(assets, 'assetGet').mockResolvedValue(image)
  await mount()
  await act(async () => root!.render(<FlowCanvas steps={[{ ...steps[0]!, detail: `第一段完整说明\n\n**最后一段不能省略**\n\n${assets.assetMarkdown(image)}` }]} source="" onChange={() => {}} />))
  const node = document.querySelector<HTMLElement>('.flow-canvas__node')!
  expect(node.querySelector('strong')?.textContent).toBe('最后一段不能省略')
  expect(node.querySelector('img')?.getAttribute('src')).toMatch(/^blob:/)
  expect(node.querySelector('img')?.getAttribute('alt')).toBe('操作.png')
  expect(node.style.height).toBe('')
  expect(node.querySelector('button button')).toBeNull()
  expect(document.querySelector('.flow-canvas__inspector')).toBeNull()
})
it('纵向顺排在同一列从上往下，卡片之间留间隔，不左右折返', async () => {
  await mount()
  await act(async () => root!.render(<FlowCanvas steps={Array.from({ length: 5 }, (_, i) => ({ ...steps[0]!, id: String(i) }))} source="" onChange={() => {}} />))
  const direction = document.querySelector<HTMLSelectElement>('[aria-label="流程排列方向"]')!
  await act(async () => { direction.value = 'vertical'; direction.dispatchEvent(new Event('change', { bubbles: true })) })
  const nodes = [...document.querySelectorAll<HTMLElement>('.flow-canvas__node')]
  expect(nodes.every(node => node.style.left === '0px')).toBe(true)
  expect(nodes.every((node, i) => i === 0 || Number.parseFloat(node.style.top) - Number.parseFloat(nodes[i - 1]!.style.top) >= 148 + 96)).toBe(true)
  await act(async () => (document.querySelector('.flow-canvas__nav-step:nth-child(4)') as HTMLButtonElement).click())
  const [, , y, scale] = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec((document.querySelector('.flow-canvas__scene') as HTMLElement).style.transform)!
  const top = Number(y) + Number.parseFloat(nodes[3]!.style.top) * Number(scale)
  expect(top).toBeGreaterThanOrEqual(32)
  expect(top + 148 * Number(scale)).toBeLessThanOrEqual(488)
})
it('鼠标中键直接拖动画布，释放和失焦后停止', async () => {
  const viewport = await mount()
  viewport.setPointerCapture = vi.fn()
  viewport.releasePointerCapture = vi.fn()
  const scene = document.querySelector<HTMLElement>('.flow-canvas__scene')!
  const before = scene.style.transform
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerdown', { button: 1, pointerId: 7, clientX: 100, clientY: 100, bubbles: true, cancelable: true })))
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointermove', { button: 1, buttons: 4, pointerId: 7, clientX: 180, clientY: 150, bubbles: true })))
  expect(scene.style.transform).not.toBe(before)
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerup', { button: 1, pointerId: 7, bubbles: true })))
  const released = scene.style.transform
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointermove', { pointerId: 7, clientX: 220, clientY: 170, bubbles: true })))
  expect(scene.style.transform).toBe(released)
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(false)
})
it('查看步骤只显示所选步骤，不重复整份流程的背景与材料', async () => {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<FlowCanvas steps={steps} source="" metadata={<h2>整份流程背景与材料</h2>} onChange={() => {}} />))
  await act(async () => (document.querySelectorAll('.flow-canvas__node')[1] as HTMLButtonElement).click())
  const inspector = document.querySelector('[aria-label="第 2 步详情"]')!
  expect(inspector.textContent).not.toContain('整份流程背景与材料')
  expect(inspector.querySelector('[aria-label="第 2 步标题"]')).not.toBeNull()
  expect(inspector.querySelector('[aria-label="第 1 步标题"]')).toBeNull()
})
it('流程信息入口单独展示整体说明，切回步骤后整体说明隐藏', async () => {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<FlowCanvas steps={steps} source="" metadata={<h2>整份流程背景与材料</h2>} onChange={() => {}} />))
  await act(async () => (document.querySelector('.flow-canvas__node') as HTMLButtonElement).click())
  const info = Array.from(document.querySelectorAll('button')).find(button => button.textContent === '流程信息')!
  await act(async () => info.click())
  expect(document.querySelector('[aria-label="流程信息"]')?.textContent).toContain('整份流程背景与材料')
  expect(document.querySelector('.flow-canvas__inspector [aria-label="第 1 步标题"]')).toBeNull()
  await act(async () => (document.querySelectorAll('.flow-canvas__node')[1] as HTMLButtonElement).click())
  expect(document.querySelector('.flow-canvas__inspector')?.textContent).not.toContain('整份流程背景与材料')
  expect(document.querySelector('[aria-label="第 2 步标题"]')).not.toBeNull()
})
it('精准搜索保持连续文本匹配，模糊搜索允许分散关键词，结果定位步骤', async () => {
  await mount(); await input('搜索流程步骤', 'ERP 数量')
  expect(document.querySelectorAll('.flow-canvas__node--match')).toHaveLength(1)
  const select = document.querySelector('[aria-label="搜索方式"]') as HTMLSelectElement
  await act(async () => { select.value = 'exact'; select.dispatchEvent(new Event('change', { bubbles: true })) })
  expect(document.querySelectorAll('.flow-canvas__node--match')).toHaveLength(0)
  await input('搜索流程步骤', '通知仓库')
  await act(async () => (document.querySelector('[aria-label="下一个搜索结果"]') as HTMLButtonElement).click())
  expect(document.querySelector('.flow-canvas__node[aria-pressed="true"]')?.textContent).toContain('通知仓库')
})
it('普通滚轮直接缩放并保持鼠标指向的位置，详情输入滚轮不缩放画布', async () => {
  const viewport = await mount()
  vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 800, height: 600, right: 800, bottom: 600, x: 0, y: 0, toJSON() {} })
  const scene = () => document.querySelector('.flow-canvas__scene') as HTMLElement
  const before = scene().style.transform
  const event = new WheelEvent('wheel', { deltaY: -120, clientX: 200, clientY: 150, bubbles: true, cancelable: true })
  await act(async () => viewport.dispatchEvent(event))
  expect(scene().style.transform).not.toBe(before)
  expect(event.defaultPrevented).toBe(true)
  expect(document.querySelector('[aria-label="画布缩放比例"]')?.textContent).not.toBe('100%')
  await act(async () => (document.querySelector('.flow-canvas__node') as HTMLElement).click())
  const edited = scene().style.transform
  await act(async () => document.querySelector('[aria-label="第 1 步说明"]')!.dispatchEvent(new WheelEvent('wheel', { deltaY: -120, bubbles: true, cancelable: true })))
  expect(scene().style.transform).toBe(edited)
})
it('空格在编辑输入中正常打字，画布拖动模式在松键与失焦后停止', async () => {
  const viewport = await mount()
  await act(async () => (document.querySelector('.flow-canvas__node') as HTMLButtonElement).click())
  const input = document.querySelector('[aria-label="第 1 步标题"]') as HTMLInputElement
  const typed = new KeyboardEvent('keydown', { code: 'Space', key: ' ', bubbles: true, cancelable: true })
  await act(async () => { input.focus(); input.dispatchEvent(typed) })
  expect(typed.defaultPrevented).toBe(false)
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(false)
  await act(async () => { viewport.focus(); viewport.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', key: ' ', bubbles: true, cancelable: true })) })
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(true)
  await act(async () => window.dispatchEvent(new Event('blur')))
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(false)
})
it('切换步骤后延迟到达的附件不会插入另一步的空白说明', async () => {
  let finish!: (asset: assets.ContentAsset) => void
  vi.spyOn(assets, 'assetImportFile').mockImplementation(() => new Promise(resolve => { finish = resolve }))
  vi.spyOn(assets, 'assetGet').mockResolvedValue({ id: '00000000-0000-7000-8000-000000000001', name: '订单.txt', mime: 'text/plain', byteSize: 3, dataBase64: '', sha256: '', createdAt: '' })
  const change = vi.fn()
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<FlowCanvas steps={steps.map(s => ({ ...s, detail: '' }))} source="" onChange={change} initialEdit />))
  const field = document.querySelector('[aria-label="第 1 步说明"]') as HTMLTextAreaElement
  field.focus(); field.setSelectionRange(0, 0)
  const file = document.querySelector('.flow-canvas__inspector input[type=file]')!
  Object.defineProperty(file, 'files', { value: [new File(['abc'], '订单.txt')] })
  await act(async () => file.dispatchEvent(new Event('change', { bubbles: true })))
  await act(async () => (document.querySelectorAll('.flow-canvas__node')[1] as HTMLButtonElement).click())
  const next = document.querySelector('[aria-label="第 2 步说明"]') as HTMLTextAreaElement
  next.focus(); next.setSelectionRange(0, 0)
  await act(async () => finish({ id: '00000000-0000-7000-8000-000000000001', name: '订单.txt', mime: 'text/plain', byteSize: 3, dataBase64: '', sha256: '', createdAt: '' }))
  expect(change).not.toHaveBeenCalled()
  expect(next.value).toBe('')
})
it('一百步的查看全图包含首尾卡片', async () => {
  await mount()
  const viewport = document.querySelector('[aria-label="流程画布"]') as HTMLElement
  vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 800, height: 500, right: 800, bottom: 500, x: 0, y: 0, toJSON() {} })
  await act(async () => root!.render(<FlowCanvas steps={Array.from({ length: 100 }, (_, i) => ({ ...steps[0]!, id: String(i) }))} source="" onChange={() => {}} />))
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '查看全图')!.click())
  const transform = (document.querySelector('.flow-canvas__scene') as HTMLElement).style.transform
  const [, x, y, scale] = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec(transform)!
  expect(Number(y)).toBeGreaterThanOrEqual(0)
  expect(Number(y) + 148 * Number(scale)).toBeLessThanOrEqual(500)
  const last = document.querySelectorAll<HTMLElement>('.flow-canvas__node')[99]!
  expect(Number(x) + (Number.parseFloat(last.style.left) + Number.parseFloat(last.style.width)) * Number(scale)).toBeLessThanOrEqual(768)
})
it('全屏查看全图避开顶栏、底栏和打开但未选步骤的信息面板', async () => {
  const viewport = await mount()
  const host = document.querySelector('.flow-canvas')!.parentElement!
  host.classList.add('memos', 'memos--canvas')
  const actions = document.createElement('div'); actions.className = 'memos__document-actions'; host.append(actions)
  const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON() {} })
  vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue(rect(0, 0, 800, 500))
  vi.spyOn(actions, 'getBoundingClientRect').mockReturnValue(rect(16, 12, 700, 84))
  vi.spyOn(document.querySelector('.flow-canvas__toolbar')!, 'getBoundingClientRect').mockReturnValue(rect(16, 400, 768, 84))
  vi.spyOn(document.querySelector('.flow-canvas__hint')!, 'getBoundingClientRect').mockReturnValue(rect(20, 380, 600, 20))
  await act(async () => root!.render(<FlowCanvas steps={Array.from({ length: 100 }, (_, i) => ({ ...steps[0]!, id: String(i) }))} source="" onChange={() => {}} />))
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '流程信息')!.click())
  vi.spyOn(document.querySelector('.flow-canvas__inspector')!, 'getBoundingClientRect').mockReturnValue(rect(444, 110, 340, 240))
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '查看全图')!.click())
  const transform = (document.querySelector('.flow-canvas__scene') as HTMLElement).style.transform
  const [, x, y, scale] = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec(transform)!
  expect(Number(y)).toBeGreaterThanOrEqual(112)
  expect(Number(y) + 148 * Number(scale)).toBeLessThanOrEqual(364)
  const last = document.querySelectorAll<HTMLElement>('.flow-canvas__node')[99]!
  expect(Number(x) + (Number.parseFloat(last.style.left) + Number.parseFloat(last.style.width)) * Number(scale)).toBeLessThanOrEqual(420)
})
it('鼠标回到画布后，空格不激活仍有焦点的工具栏按钮', async () => {
  const viewport = await mount()
  const button = [...document.querySelectorAll('button')].find(b => b.textContent === '查看全图')!
  button.focus()
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })))
  const press = new KeyboardEvent('keydown', { code: 'Space', key: ' ', bubbles: true, cancelable: true })
  await act(async () => button.dispatchEvent(press))
  expect(press.defaultPrevented).toBe(true)
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(true)
  const release = new KeyboardEvent('keyup', { code: 'Space', key: ' ', bubbles: true, cancelable: true })
  await act(async () => button.dispatchEvent(release))
  expect(release.defaultPrevented).toBe(true)
  expect(viewport.classList.contains('flow-canvas__viewport--pan')).toBe(false)
})
it('原生菜单拦截仅在画布拥有输入时启用，编辑、失焦和卸载均关闭', async () => {
  const guard = vi.spyOn(canvasInput, 'setCanvasInputActive').mockResolvedValue()
  const viewport = await mount()
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })))
  expect(guard).toHaveBeenLastCalledWith(true)
  const beforeRefocus = guard.mock.calls.length
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(guard.mock.calls.length).toBe(beforeRefocus + 1)
  expect(guard).toHaveBeenLastCalledWith(true)
  await act(async () => (document.querySelector('.flow-canvas__node') as HTMLButtonElement).click())
  await act(async () => (document.querySelector('[aria-label="第 1 步标题"]') as HTMLInputElement).focus())
  expect(guard).toHaveBeenLastCalledWith(false)
  await act(async () => viewport.focus())
  expect(guard).toHaveBeenLastCalledWith(true)
  await act(async () => window.dispatchEvent(new Event('blur')))
  expect(guard).toHaveBeenLastCalledWith(false)
  await act(async () => root!.unmount()); root = undefined
  expect(guard).toHaveBeenLastCalledWith(false)
})
it('键盘聚焦画布或节点时无需鼠标进入，离开指针仍保护，聚焦外部或编辑区域则关闭', async () => {
  const guard = vi.spyOn(canvasInput, 'setCanvasInputActive').mockResolvedValue()
  const viewport = await mount()
  await act(async () => viewport.focus())
  expect(guard).toHaveBeenLastCalledWith(true)
  await act(async () => {
    viewport.dispatchEvent(new FocusEvent('focusout', { bubbles: true }))
    window.dispatchEvent(new Event('blur'))
  })
  expect(guard).toHaveBeenLastCalledWith(false)
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(guard).toHaveBeenLastCalledWith(true)
  const press = new KeyboardEvent('keydown', { code: 'Space', key: ' ', bubbles: true, cancelable: true })
  await act(async () => viewport.dispatchEvent(press))
  expect(press.defaultPrevented).toBe(true)
  await act(async () => viewport.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space', bubbles: true })))
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })))
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerout', { bubbles: true, relatedTarget: document.body })))
  expect(guard).toHaveBeenLastCalledWith(true)
  await act(async () => viewport.blur())
  expect(document.activeElement).toBe(document.body)
  expect(guard).toHaveBeenLastCalledWith(false)
  await act(async () => viewport.focus())
  expect(guard).toHaveBeenLastCalledWith(true)
  const outside = document.createElement('button'); document.body.append(outside)
  await act(async () => outside.focus())
  expect(guard).toHaveBeenLastCalledWith(false)
  const outsidePress = new KeyboardEvent('keydown', { code: 'Space', key: ' ', bubbles: true, cancelable: true })
  await act(async () => outside.dispatchEvent(outsidePress))
  expect(outsidePress.defaultPrevented).toBe(false)
  const node = document.querySelector('.flow-canvas__node') as HTMLButtonElement
  await act(async () => node.focus())
  expect(guard).toHaveBeenLastCalledWith(true)
  await act(async () => node.click())
  const inspectorButton = document.querySelector('.flow-canvas__inspector button') as HTMLButtonElement
  await act(async () => inspectorButton.focus())
  expect(guard).toHaveBeenLastCalledWith(false)
  const field = document.querySelector('[aria-label="第 1 步标题"]') as HTMLInputElement
  await act(async () => field.focus())
  expect(guard).toHaveBeenLastCalledWith(false)
  await act(async () => viewport.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })))
  expect(guard).toHaveBeenLastCalledWith(false)
})
