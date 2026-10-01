// @vitest-environment happy-dom
import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { ContentEditor } from './ContentEditor'
import * as assets from '../lib/content-assets'
import { readImage, readText } from '@tauri-apps/plugin-clipboard-manager'

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readImage: vi.fn(), readText: vi.fn() }))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => { act(() => root?.unmount()); root = undefined; vi.restoreAllMocks(); document.body.innerHTML = '' })
async function mount(initial = '操作说明', readOnly = false) {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  function Editor() { const [value, setValue] = useState(initial); return <ContentEditor aria-label="内容" value={value} readOnly={readOnly} onChange={(e) => setValue(e.target.value)} /> }
  await act(async () => root!.render(<Editor />))
  const field = host.querySelector('textarea')!; field.focus(); field.setSelectionRange(4,4)
  return field
}
const asset: assets.ContentAsset = { id:'00000000-0000-7000-8000-000000000001', name:'业务截图.png', mime:'image/png',dataBase64:'',byteSize:24,sha256:'hash',createdAt:'2026-09-30' }
it('完整预览中在图片前新增段落，复用已读取资源和图片 URL', async () => {
  const read = vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const field = await mount(assets.assetMarkdown(asset))
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '预览内容')!.click())
  const url = document.querySelector<HTMLImageElement>('.content-asset img')!.src
  read.mockClear()
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, `新增段落\n\n${assets.assetMarkdown(asset)}`)
    field.dispatchEvent(new Event('input', { bubbles: true }))
  })
  expect(document.querySelector<HTMLImageElement>('.content-asset img')!.src).toBe(url)
  expect(read).not.toHaveBeenCalled()
})
it('图片引用换为未加载资源时，不展示或导出上一张图片；移除后释放 URL', async () => {
  const revoke = vi.spyOn(URL, 'revokeObjectURL')
  vi.spyOn(assets, 'assetGet').mockImplementation(id => id === asset.id ? Promise.resolve(asset) : new Promise(() => {}))
  const field = await mount(assets.assetMarkdown(asset))
  const url = document.querySelector<HTMLImageElement>('.content-asset img')!.src
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, assets.assetMarkdown({ ...asset, id: '00000000-0000-7000-8000-000000000002' }))
    field.dispatchEvent(new Event('input', { bubbles: true }))
  })
  expect(document.querySelector('.content-asset img')).toBeNull()
  expect([...document.querySelectorAll('button')].some(b => b.textContent?.includes('业务截图.png ·'))).toBe(false)
  expect(revoke).toHaveBeenCalledWith(url)
})
it.each([false, true])('输入文字时保留已加载图片，不重新读取或替换图片节点（完整预览：%s）', async (preview) => {
  const read = vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const field = await mount(`原始说明\n${assets.assetMarkdown(asset)}`)
  if (preview) await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '预览内容')!.click())
  read.mockClear()
  const image = document.querySelector('.content-asset img')
  expect(image).toBeTruthy()
  for (const text of ['新增文字', '新增文字，继续输入']) {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, `原始说明\n${assets.assetMarkdown(asset)}\n${text}`)
      field.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(document.querySelector('.content-asset img')).toBe(image)
  }
  expect(read).not.toHaveBeenCalled()
})
it('移除插图仅去掉当前材料中该图引用，保留文字和其它图片', async () => {
  const second = { ...asset, id: '00000000-0000-7000-8000-000000000002', name: '另一张图.png' }
  vi.spyOn(assets, 'assetGet').mockImplementation(async id => id === asset.id ? asset : second)
  const initial = `订单说明\n${assets.assetMarkdown(asset)}\n改到12月8号\n${assets.assetMarkdown(second)}\n保留这段文字`
  const field = await mount(initial)
  const remove = [...document.querySelectorAll('button')].find(b => b.getAttribute('aria-label') === '移除图片：业务截图.png')
  expect(remove).toBeTruthy()
  await act(async () => remove!.click())
  expect(field.value).toBe(initial.replace(assets.assetMarkdown(asset), ''))
  expect(document.querySelectorAll('.content-asset')).toHaveLength(1)
})
it('原图读取失败时仍可移除失效引用', async () => {
  vi.spyOn(assets, 'assetGet').mockRejectedValue(new Error('图片不存在'))
  const field = await mount(`原说明\n${assets.assetMarkdown(asset)}`)
  const remove = [...document.querySelectorAll('button')].find(b => b.textContent === '移除图片')
  expect(remove).toBeTruthy()
  await act(async () => remove!.click())
  expect(field.value).toBe('原说明\n')
})
it('只读预览不出现移除按钮', async () => {
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const source = assets.assetMarkdown(asset)
  const field = await mount(source, true)
  expect([...document.querySelectorAll('button')].some(b => b.textContent === '移除图片')).toBe(false)
  expect(field.value).toBe(source)
})
it('连续插入图片或文件时不能从中间拆坏现有资源引用', () => {
  const token = assets.assetMarkdown(asset)
  expect(assets.contentInsertionRange(token, 10, 10)).toEqual({start:token.length,end:token.length})
  expect(assets.contentInsertionRange(`前${token}后`, 2, 12)).toEqual({start:1,end:1+token.length})
  expect(assets.contentInsertionRange('纯文字',1,1)).toEqual({start:1,end:1})
})
function filesEvent(type: 'paste' | 'drop', files: File[]) {
  const event = new Event(type, { bubbles:true, cancelable:true })
  Object.defineProperty(event, type === 'paste' ? 'clipboardData' : 'dataTransfer', {value:{ files,types:['Files'] }})
  return event
}
it('粘贴图片写入本地资源并在光标处插入引用，原文字保留', async () => {
  const field = await mount()
  const save = vi.spyOn(assets,'assetImportFile').mockResolvedValue(asset)
  const file = new File(['png bytes'], 'business.png', {type:'image/png'})
  await act(async () => field.dispatchEvent(filesEvent('paste',[file])))
  expect(save).toHaveBeenCalledExactlyOnceWith(file)
  expect(field.value).toBe(`操作说明\n![业务截图.png](lumen-asset:${asset.id})\n`)
})
it('文件拖入失败会显示错误并保留原输入', async () => {
  const field = await mount()
  vi.spyOn(assets,'assetImportFile').mockRejectedValue(new Error('磁盘写入失败'))
  await act(async () => field.dispatchEvent(filesEvent('drop',[new File(['x'],'manual.pdf')])))
  expect(field.value).toBe('操作说明')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('磁盘写入失败')
})
it('Ctrl+Shift+V 每次读取当前剪贴板，保留与图片粘贴同一输入路径', async () => {
  const field = await mount()
  vi.mocked(readImage).mockRejectedValue(new Error('no image'))
  vi.mocked(readText).mockResolvedValueOnce('第一份').mockResolvedValueOnce('第二份')
  for (let i=0;i<2;i++) {
    await act(async () => field.dispatchEvent(new KeyboardEvent('keydown',{key:'v',ctrlKey:true,shiftKey:true,bubbles:true,cancelable:true})))
    field.setSelectionRange(field.value.length,field.value.length)
  }
  expect(field.value).toBe('操作说明第一份第二份')
  expect(readText).toHaveBeenCalledTimes(2)
})
it('导入未完成时继续编辑，不得用旧文本覆盖新输入', async () => {
  const field = await mount()
  let resolve!: (a: assets.ContentAsset) => void
  vi.spyOn(assets,'assetImportFile').mockReturnValue(new Promise((r) => { resolve=r }))
  await act(async () => field.dispatchEvent(filesEvent('paste',[new File(['x'],'x.png')])))
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(field,'新的业务说明')
    field.dispatchEvent(new Event('input',{bubbles:true}))
    resolve(asset)
  })
  expect(field.value).toBe('新的业务说明')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('编辑位置发生变化')
})
