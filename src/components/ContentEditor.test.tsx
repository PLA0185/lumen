// @vitest-environment happy-dom
import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { ContentEditor } from './ContentEditor'
import * as assets from '../lib/content-assets'
import { readImage, readText } from '@tauri-apps/plugin-clipboard-manager'
import { invoke } from '@tauri-apps/api/core'

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readImage: vi.fn(), readText: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
beforeEach(() => vi.mocked(invoke).mockRejectedValue(new Error('测试未配置此 IPC')))
afterEach(() => { act(() => root?.unmount()); root = undefined; vi.restoreAllMocks(); vi.mocked(invoke).mockReset(); document.body.innerHTML = '' })
async function mount(initial = '操作说明', readOnly = false, extractFiles = false, maxLength?: number, disabled = false) {
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  function Editor() { const [value, setValue] = useState(initial); return <ContentEditor aria-label="内容" value={value} maxLength={maxLength} readOnly={readOnly} disabled={disabled} extractFiles={extractFiles} onChange={(e) => setValue(e.target.value)} /> }
  await act(async () => root!.render(<Editor />))
  const field = host.querySelector('textarea')!; field.focus(); field.setSelectionRange(4,4)
  return field
}
const asset: assets.ContentAsset = { id:'00000000-0000-7000-8000-000000000001', name:'业务截图.png', mime:'image/png',dataBase64:'',byteSize:24,sha256:'hash',createdAt:'2026-09-30' }
it('输入增加时说明自动增高，手动收回后相同内容不强制撑开', async () => {
  const field = await mount('原文')
  let scroll = 360
  Object.defineProperty(field, 'offsetHeight', {get:() => Number.parseFloat(field.style.height) || 120})
  Object.defineProperty(field, 'clientHeight', {get:() => field.offsetHeight})
  Object.defineProperty(field, 'scrollHeight', {get:() => scroll})
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(field,'原文\n新增说明')
    field.dispatchEvent(new Event('input',{bubbles:true}))
  })
  expect(field.style.height).toBe('360px')
  const edge = document.querySelector('[aria-label="调整内容高度"]')!
  await act(async () => edge.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true,cancelable:true})))
  expect(field.style.height).toBe('344px')
  await act(async () => field.dispatchEvent(new Event('input',{bubbles:true})))
  expect(field.style.height).toBe('344px')
  scroll = 500
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(field,'原文\n新增说明\n继续增加')
    field.dispatchEvent(new Event('input',{bubbles:true}))
  })
  expect(field.style.height).toBe('500px')
  expect(field.style.width).toBe('')
})
it('说明输入框整条底边调整高度，宽度不随拖动改变', async () => {
  const field = await mount()
  Object.defineProperty(field, 'offsetHeight', { value: 200 })
  vi.spyOn(field, 'getBoundingClientRect').mockReturnValue({ height: 200 } as DOMRect)
  const edge = document.querySelector<HTMLElement>('[aria-label="调整内容高度"]')
  expect(edge).not.toBeNull()
  edge!.setPointerCapture = vi.fn(); edge!.releasePointerCapture = vi.fn()
  await act(async () => edge!.dispatchEvent(new PointerEvent('pointerdown', { button: 0, pointerId: 1, clientX: 10, clientY: 200, bubbles: true })))
  await act(async () => edge!.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientX: 150, clientY: 280, bubbles: true })))
  await act(async () => edge!.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, bubbles: true })))
  expect(field.style.height).toBe('280px')
  expect(field.style.width).toBe('')
})
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
async function recognizeAsset(name: string) {
  const button = [...document.querySelectorAll('button')].find(b => b.getAttribute('aria-label') === `识别内容：${name}`)
  expect(button, `原资源 ${name} 应提供显式识别按钮`).toBeTruthy()
  await act(async () => { button!.focus(); button!.click() })
}
it.each(['paste', 'drop', 'pick'] as const)('%s 文件只插入原文件卡，点击识别才读取正文', async (method) => {
  const original = { ...asset, name: '原始订单.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'content_asset_import' || command === 'content_asset_get') return original
    if (command === 'content_asset_extract' && (args as { id: string }).id === original.id) return { text: '核对订单数量', images: [], warnings: [] }
    throw new Error(`意外的 IPC：${command}`)
  })
  const field = await mount('保留原说明', false, true)
  const file = new File(['word'], '原始订单.docx')
  await act(async () => {
    if (method === 'pick') {
      const picker = document.querySelector<HTMLInputElement>('input[type="file"]')!
      Object.defineProperty(picker, 'files', { value: [file] })
      picker.dispatchEvent(new Event('change', { bubbles: true }))
    } else field.dispatchEvent(filesEvent(method, [file]))
    await new Promise(resolve => setTimeout(resolve, 20))
  })
  expect(field.value).toBe('保留原说\n[原始订单.docx](lumen-asset:00000000-0000-7000-8000-000000000001)\n明')
  expect(document.querySelector('.content-asset')?.textContent).toContain('原始订单.docx')
  expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'content_asset_extract')).toBe(false)
  expect(document.querySelector('input[type="checkbox"]')).toBeNull()
  field.focus(); field.setSelectionRange(0, 4)
  await recognizeAsset(original.name)
  expect(field.value).toContain('核对订单数量')
  expect(field.value).toContain('保留原说')
  expect(field.value).toContain('[原始订单.docx](lumen-asset:00000000-0000-7000-8000-000000000001)')
  expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'content_asset_extract')).toEqual([['content_asset_extract', { id: original.id }]])
})
it('识别导入文件后把正文和内嵌原图放入编辑器，保留原文件与已有文字', async () => {
  const fileAsset = { ...asset, name: '发货流程.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  const image = { ...asset, id: '00000000-0000-7000-8000-000000000002' }
  vi.spyOn(assets, 'assetImportFile').mockResolvedValue(fileAsset)
  vi.spyOn(assets, 'assetGet').mockImplementation(async id => id === fileAsset.id ? fileAsset : image)
  const extract = vi.mocked(invoke).mockResolvedValue({ text: '先核对发货表，再通知仓库。', images: [image], warnings: [] })
  const field = await mount('保留说明', false, true)
  await act(async () => field.dispatchEvent(filesEvent('drop', [new File(['word'], '发货流程.docx')])))
  await recognizeAsset(fileAsset.name)
  expect(extract).toHaveBeenCalledExactlyOnceWith('content_asset_extract', { id: fileAsset.id })
  expect(field.value).toContain('保留说明')
  expect(field.value).toContain(assets.assetMarkdown(fileAsset))
  expect(field.value).toContain('先核对发货表，再通知仓库。')
  expect(field.value).toContain(assets.assetMarkdown(image))
  expect(field.value).toContain(`<!-- lumen-extracted:${fileAsset.id} -->`)
})
it('Word 识别正文已有原位置图片时不再次堆到正文末尾', async () => {
  const original = { ...asset, name: 'SOP.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  const image = { ...asset, id: '00000000-0000-7000-8000-000000000002' }
  vi.spyOn(assets, 'assetGet').mockResolvedValue(original)
  const text = `## 1. 下载\n取得表格。\n${assets.assetMarkdown(image)}\n\n## 2. 创建\n生成单据。`
  vi.mocked(invoke).mockResolvedValue({ text, images: [image], warnings: [] })
  const field = await mount(assets.assetMarkdown(original), false, true)
  await recognizeAsset(original.name)
  expect(field.value.split(assets.assetMarkdown(image))).toHaveLength(2)
  expect(field.value.indexOf(assets.assetMarkdown(image))).toBeLessThan(field.value.indexOf('## 2. 创建'))
})
it('识别失败明确提示并保留已导入文件链接，不伪装成识别成功', async () => {
  vi.spyOn(assets, 'assetImportFile').mockResolvedValue(asset)
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  vi.mocked(invoke).mockRejectedValue(new Error('本机没有可用 OCR 语言包'))
  const field = await mount('原说明', false, true)
  await act(async () => field.dispatchEvent(filesEvent('paste', [new File(['png'], '业务截图.png')])))
  await recognizeAsset(asset.name)
  expect(field.value).toContain('原说明')
  expect(field.value).toContain(assets.assetMarkdown(asset))
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('本机没有可用 OCR 语言包')
})
it('附件模式不调用识别，识别警告与正文均不被静默丢弃', async () => {
  vi.spyOn(assets, 'assetImportFile').mockResolvedValue(asset)
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const extract = vi.mocked(invoke).mockResolvedValue({ text: '识别出的内容', images: [], warnings: ['请核对表格列顺序'] })
  let field = await mount('原说明')
  await act(async () => field.dispatchEvent(filesEvent('drop', [new File(['x'], 'image.png')])))
  expect(extract).not.toHaveBeenCalled()
  await act(async () => root?.unmount()); root = undefined; document.body.innerHTML = ''
  field = await mount('原说明', false, true)
  await act(async () => field.dispatchEvent(filesEvent('drop', [new File(['x'], 'image.png')])))
  await recognizeAsset(asset.name)
  expect(field.value).toContain('识别出的内容')
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('请核对表格列顺序')
})
it('识别正文超过步骤剩余额度时保留原文件，完整识别结果存成可访问文件', async () => {
  const original = { ...asset, name: '发货流程.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  const complete = { ...asset, id: '00000000-0000-7000-8000-000000000002', name: '发货流程.docx-识别内容.md', mime: 'text/plain' }
  const imported = vi.spyOn(assets, 'assetImportFile').mockResolvedValueOnce(original).mockResolvedValueOnce(complete)
  vi.spyOn(assets, 'assetGet').mockImplementation(async id => id === original.id ? original : complete)
  vi.mocked(invoke).mockResolvedValue({ text: '文'.repeat(5001), images: [], warnings: [] })
  const field = await mount('原说明', false, true, 5000)
  await act(async () => field.dispatchEvent(filesEvent('drop', [new File(['x'], '发货流程.docx')])))
  await recognizeAsset(original.name)
  expect(field.value).toContain('原说明')
  expect(field.value).toContain(assets.assetMarkdown(original))
  expect(field.value).toContain(assets.assetMarkdown(complete))
  expect(await imported.mock.calls[1]![0].text()).toContain('文'.repeat(5001))
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('完整识别内容已保存为文件')
})
it('剩余额度只能容纳原文件链接时，完整结果仍能从导入结果区访问', async () => {
  const original = { ...asset, name: `${'文'.repeat(250)}.docx`, mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' }
  const complete = { ...asset, id: '00000000-0000-7000-8000-000000000002', name: `${'文'.repeat(180)}-识别内容.md`, mime: 'text/plain' }
  vi.spyOn(assets, 'assetImportFile').mockResolvedValueOnce(original).mockResolvedValueOnce(complete)
  vi.spyOn(assets, 'assetGet').mockImplementation(async id => id === original.id ? original : complete)
  vi.mocked(invoke).mockResolvedValue({ text: '识别正文'.repeat(1500), images: [], warnings: [] })
  const field = await mount('原'.repeat(4500), false, true, 5000)
  await act(async () => field.dispatchEvent(filesEvent('drop', [new File(['x'], original.name)])))
  await recognizeAsset(original.name)
  expect(field.value).toContain(assets.assetMarkdown(original))
  expect(field.value.length).toBeLessThanOrEqual(5000)
  expect(document.querySelector('[aria-label="本次导入结果"]')?.textContent).toContain(complete.name)
})
it.each(['readOnly', 'disabled', 'attachmentOnly'] as const)('%s 资源不允许手动识别', async (mode) => {
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  const field = await mount(assets.assetMarkdown(asset), mode === 'readOnly', mode !== 'attachmentOnly', undefined, mode === 'disabled')
  expect([...document.querySelectorAll('button')].filter(b => b.textContent === '识别内容' && !b.disabled)).toHaveLength(0)
  expect(field.value).toBe(assets.assetMarkdown(asset))
  expect(invoke).not.toHaveBeenCalled()
})
it('手动识别插入光标所在资源之后，不拆开原引用；异步期间新输入不被覆盖', async () => {
  vi.spyOn(assets, 'assetGet').mockResolvedValue(asset)
  let resolve!: (result: { text: string; images: assets.ContentAsset[]; warnings: string[] }) => void
  vi.mocked(invoke).mockReturnValueOnce(new Promise(r => { resolve = r })).mockResolvedValueOnce({ text: '核对数量', images: [], warnings: [] })
  const source = `原说明\n${assets.assetMarkdown(asset)}\n末尾说明`
  const field = await mount(source, false, true)
  field.setSelectionRange(12, 12)
  const button = [...document.querySelectorAll('button')].find(b => b.textContent === '识别内容')
  expect(button).toBeTruthy()
  await act(async () => button!.click())
  expect([...document.querySelectorAll('button')].filter(b => b.textContent === '识别内容' && !b.disabled)).toHaveLength(0)
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, `${source}\n新输入`)
    field.dispatchEvent(new Event('input', { bubbles: true }))
    resolve({ text: '过期结果', images: [], warnings: [] })
  })
  expect(field.value).toBe(`${source}\n新输入`)
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('编辑位置发生变化')
  field.setSelectionRange(12, 12)
  await recognizeAsset(asset.name)
  expect(field.value).toContain(`${assets.assetMarkdown(asset)}\n\n### 业务截图.png · 识别内容\n\n核对数量\n\n末尾说明`)
  expect(field.value).toContain('新输入')
})
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
