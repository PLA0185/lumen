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
async function mount() {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  function Editor() { const [value, setValue] = useState('操作说明'); return <ContentEditor aria-label="内容" value={value} onChange={(e) => setValue(e.target.value)} /> }
  await act(async () => root!.render(<Editor />))
  const field = host.querySelector('textarea')!; field.focus(); field.setSelectionRange(4,4)
  return field
}
const asset: assets.ContentAsset = { id:'00000000-0000-7000-8000-000000000001', name:'业务截图.png', mime:'image/png',dataBase64:'',byteSize:24,sha256:'hash',createdAt:'2026-09-30' }
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
