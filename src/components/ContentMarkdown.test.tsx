// @vitest-environment happy-dom
import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { ContentMarkdown } from './ContentMarkdown'
import * as assets from '../lib/content-assets'
import { save } from '@tauri-apps/plugin-dialog'
import * as annotations from '../lib/image-annotations'
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
const asset: assets.ContentAsset = { id: '00000000-0000-7000-8000-000000000001', name: 'image1.png', mime: 'image/png', byteSize: 24, dataBase64: '', sha256: '', createdAt: '' }
it('中文文件名的一个原文件引用只显示一个组件，合法的两次引用仍显示两次', async () => {
  const original = { ...asset, name: '出货SOP_标准版.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  vi.spyOn(assets, 'assetGet').mockResolvedValue(original)
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  const token = assets.assetMarkdown(original)
  await act(async () => root!.render(<ContentMarkdown>{token}</ContentMarkdown>))
  expect(host.querySelectorAll('.content-asset')).toHaveLength(1)
  await act(async () => root!.render(<ContentMarkdown>{`${token}\n\n${token}`}</ContentMarkdown>))
  expect(host.querySelectorAll('.content-asset')).toHaveLength(2)
})
let root: Root | undefined
afterEach(() => { act(() => root?.unmount()); root = undefined; document.body.innerHTML = ''; vi.restoreAllMocks(); vi.mocked(save).mockReset() })
async function mount(caption = '', writable = true) {
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  function Harness() {
    const [text, setText] = useState(`![原图](lumen-asset:${asset.id}${caption ? ` "${caption}"` : ''})`)
    return <ContentMarkdown onUpdateImage={writable ? (id, replacement) => setText(assets.replaceImageReference(text, id, replacement)) : undefined}>{text}</ContentMarkdown>
  }
  await act(async () => root!.render(<Harness />))
}
async function click(text: string) {
  const b = [...document.querySelectorAll('button')].find(b => b.textContent === text)!
  expect(b).toBeTruthy(); await act(async () => b.click())
}
it('图片不显示文件名或大小，备注显示，单击打开大图，关闭返回当前内容', async () => {
  await mount('发货单位置')
  expect(document.body.textContent).not.toContain('image1.png')
  expect(document.body.textContent).not.toContain('KB')
  expect(document.body.textContent).toContain('发货单位置')
  expect(document.body.textContent).not.toContain('另存为')
  await act(async () => document.querySelector('img')!.click())
  expect(document.querySelector('[aria-label="图片查看与批注"]')).not.toBeNull()
  for (const label of ['手绘', '箭头', '文字', '另存为', '保存到流程', '移除图片']) expect(document.body.textContent).toContain(label)
  await click('关闭图片')
  expect(document.querySelector('[aria-label="图片查看与批注"]')).toBeNull()
  expect(document.querySelector('img')).not.toBeNull()
})
it('共用断行保护跨强调边界组合中文，保留复制文字和代码，不留下自动换行孤字', async () => {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<ContentMarkdown>{'**例外情况**：若总数不一致，需回查货件与出货计划数**据**。\n\nExcel表格，ERP单。\n\n`数`'}</ContentMarkdown>))
  const paragraphs = [...host.querySelectorAll('p')]
  expect(paragraphs[0]!.textContent).toBe('例外情况：若总数不一致，需回查货件与出货计划数据。')
  const units = [...paragraphs[0]!.querySelectorAll('.no-orphan')]
  expect(units.length).toBeGreaterThan(0)
  expect(units.every(unit => [...(unit.textContent ?? '')].filter(c => /\p{Script=Han}/u.test(c)).length >= 2)).toBe(true)
  expect(units.map(unit => ({ text: unit.textContent, strong: unit.querySelector('strong')?.textContent }))).toEqual(expect.arrayContaining([expect.objectContaining({ text: expect.stringContaining('数据。'), strong: '据' })]))
  expect(paragraphs[1]!.textContent).toBe('Excel表格，ERP单。')
  expect(host.querySelector('code')?.textContent).toBe('数')
  expect(host.querySelector('code .no-orphan')).toBeNull()
})
it('备注保存只替换本图片引用，转义引号仍可显示，重新打开保留', async () => {
  await mount()
  await act(async () => document.querySelector('img')!.click())
  const input = document.querySelector('[aria-label="图片备注"]')!
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '点击“发货”'); input.dispatchEvent(new Event('input', { bubbles: true })) })
  await click('保存备注')
  await click('关闭图片')
  expect(document.body.textContent).toContain('点击“发货”')
  await act(async () => document.querySelector('img')!.click())
  expect((document.querySelector('[aria-label="图片备注"]') as HTMLInputElement).value).toBe('点击“发货”')
})
it('只读大图可另存原图；取消不写文件，导出失败保留窗口并明确错误', async () => {
  const exportAsset = vi.spyOn(assets, 'assetExport').mockRejectedValue(new Error('写入失败'))
  await mount('', false)
  await act(async () => document.querySelector('img')!.click())
  expect([...document.querySelectorAll('button')].some(b => b.textContent === '保存到流程')).toBe(false)
  vi.mocked(save).mockResolvedValue(null)
  await click('另存为'); expect(exportAsset).not.toHaveBeenCalled()
  vi.mocked(save).mockResolvedValue('D:/test.png')
  await click('另存为')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('写入失败')
  expect(document.querySelector('[aria-label="图片查看与批注"]')).not.toBeNull()
})
it('箭头真实记录位置，撤销重做及收起后草稿保留；保存失败不替换原引用', async () => {
  await mount()
  await act(async () => document.querySelector('img')!.click())
  const image = document.querySelector<HTMLImageElement>('.image-viewer__image img')!
  Object.defineProperties(image, { naturalWidth: { value: 100 }, naturalHeight: { value: 100 } })
  await act(async () => image.dispatchEvent(new Event('load')))
  const svg = document.querySelector<SVGSVGElement>('.image-viewer__overlay')!
  vi.spyOn(svg, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 100, height: 100, right: 100, bottom: 100, x: 0, y: 0, toJSON() {} })
  svg.setPointerCapture = vi.fn(); svg.releasePointerCapture = vi.fn()
  await click('箭头')
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerdown', { button: 0, pointerId: 1, clientX: 20, clientY: 30, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientX: 80, clientY: 70, bubbles: true })))
  await act(async () => svg.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, bubbles: true })))
  expect(svg.querySelector('polyline')?.getAttribute('points')).toMatch(/^20,30 /)
  expect(svg.querySelector('polygon')?.getAttribute('points')).toMatch(/^80,70 /)
  expect(svg.querySelector('polygon')).not.toBeNull()
  await click('撤销'); expect(svg.querySelector('polyline')).toBeNull()
  await click('重做'); expect(svg.querySelector('polyline')).not.toBeNull()
  await click('关闭图片')
  await act(async () => document.querySelector('img')!.click())
  const reopened = document.querySelector<HTMLImageElement>('.image-viewer__image img')!
  Object.defineProperties(reopened, { naturalWidth: { value: 100 }, naturalHeight: { value: 100 } })
  await act(async () => reopened.dispatchEvent(new Event('load')))
  expect(document.querySelector('.image-viewer__overlay polyline')).not.toBeNull()
  vi.spyOn(annotations, 'annotatedImage').mockResolvedValue(new File(['png'], '批注图片.png', { type: 'image/png' }))
  vi.spyOn(assets, 'assetImportFile').mockRejectedValue(new Error('批注保存失败'))
  await click('保存到流程')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('批注保存失败')
  expect(document.querySelector('.image-viewer__overlay polyline')).not.toBeNull()
  expect(document.querySelector('[aria-label="图片查看与批注"]')).not.toBeNull()
})

it('流程原文换行让图片及图注按上下顺序显示，代码和表格不改写', async () => {
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  const source = '点击原入口。\n![原图](lumen-asset:' + asset.id + ')\n图1 原入口\n\n```text\n甲\n乙\n```\n\n| A | B |\n| - | - |\n| 甲 | 乙 |'
  await act(async () => root!.render(<ContentMarkdown preserveLines>{source}</ContentMarkdown>))
  const p = host.querySelector('p')!
  expect(p.querySelectorAll('br')).toHaveLength(2)
  expect(p.textContent).toContain('点击原入口。')
  expect(p.textContent).toContain('图1 原入口')
  expect(host.querySelector('pre code')!.textContent).toBe('甲\n乙\n')
  expect(host.querySelector('table')).not.toBeNull()
})
