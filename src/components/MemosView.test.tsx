// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import * as memo from '../lib/memos-ipc'
import * as ai from '../lib/ai-ipc'
import * as assets from '../lib/content-assets'
import { MemosView } from './MemosView'
import { publishDataChange } from '../lib/data-change'

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
}))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  vi.useRealTimers()
  document.body.innerHTML = ''
})
async function mount(
  query = '',
  rows: memo.MemoSummary[] | Promise<memo.MemoSummary[]> = [],
) {
  vi.useFakeTimers()
  vi.spyOn(memo, 'memoList').mockImplementation(() => Promise.resolve(rows))
  window.confirm = vi.fn(() => true)
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<MemosView query={query} />))
  await act(async () => vi.advanceTimersByTimeAsync(250))
}
async function click(text: string) {
  const button = [...document.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === text,
  )!
  expect(button).toBeTruthy()
  await act(async () => button.click())
}
async function fill(label: string, value: string) {
  const el = document.querySelector(`[aria-label="${label}"]`) as
    HTMLInputElement | HTMLTextAreaElement
  const proto =
    el.tagName === 'TEXTAREA'
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
describe('独立备忘与业务流程', () => {
  it('自动保存后的字段顺序变化不算新编辑，不会每秒重复写入或切换保存按钮文案', async () => {
    const doc: memo.MemoDocument = { id: 'order', title: '发票流程', category: '', kind: 'flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null, bodyMd: '', steps: [{ id: 'one', title: '填写发票', owner: '', detail: '原说明', group: { id: 'invoice', title: '6. 发票', path: [] }, layout: { width: 605, minHeight: 289 } }] }
    vi.spyOn(memo, 'memoGet').mockResolvedValue(doc)
    let resolve!: (doc: memo.MemoDocument) => void
    const save = vi.spyOn(memo, 'memoSave').mockImplementation(input => new Promise(r => { resolve = () => r({ ...doc, revision: 2, steps: input.steps.map(s => ({ id: s.id, title: s.title, owner: s.owner, detail: s.detail, layout: s.layout, group: s.group })) }) }))
    await mount('', [doc])
    await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    await fill('第 1 步负责人', '运营')
    await act(async () => vi.advanceTimersByTimeAsync(1000))
    expect(save).toHaveBeenCalledOnce()
    const stableLabel = [...document.querySelectorAll('.memos__toolbar button')].some(b => b.textContent === '保存并查看')
    await act(async () => resolve(doc))
    await act(async () => vi.advanceTimersByTimeAsync(5000))
    expect(save).toHaveBeenCalledOnce()
    expect(stableLabel).toBe(true)
    expect(document.querySelector('.memos__toolbar')?.textContent).not.toContain('编辑停顿后自动保存')
  })
  it('打开记录及后台刷新不重复安排列表读取，不插入挤动卡片的加载提示', async () => {
    const doc: memo.MemoDocument = { id: 'stable', title: '稳定列表', category: '', kind: 'memo', revision: 1, createdAt: '', updatedAt: '', deletedAt: null, bodyMd: '已保存正文', steps: [] }
    vi.spyOn(memo, 'memoGet').mockImplementation(async () => structuredClone(doc))
    await mount('', [doc])
    await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await act(async () => vi.advanceTimersByTimeAsync(250))
    expect(memo.memoList).toHaveBeenCalledTimes(1)
    const card = document.querySelector('.memos__item')
    let resolve!: (rows: memo.MemoSummary[]) => void
    vi.mocked(memo.memoList).mockImplementationOnce(() => new Promise(r => { resolve = r }))
    await act(async () => publishDataChange(['memos']))
    expect(document.querySelector('.memos__list')?.textContent).not.toContain('读取中')
    expect(document.querySelector('.memos__item')).toBe(card)
    await act(async () => resolve([structuredClone(doc)]))
    await act(async () => vi.advanceTimersByTimeAsync(500))
    expect(memo.memoList).toHaveBeenCalledTimes(2)
    expect(document.querySelector('.memos__item')).toBe(card)
    expect(document.querySelector('.memos__reading')?.textContent).toContain('已保存正文')
  })
  it('流程名称完整放在画布左上角，记录操作与创建筛选合并在顶部工具栏', async () => {
    const doc: memo.MemoDocument = { id: 'merged', title: '合并菜单流程', category: '', kind: 'flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null, bodyMd: '', steps: [{id:'one',title:'原操作',owner:'',detail:'原说明'}] }
    vi.spyOn(memo, 'memoGet').mockResolvedValue(doc)
    await mount('', [doc])
    await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    const toolbar = document.querySelector('.memos__toolbar')!
    expect(document.querySelector('.flow-canvas__switcher [aria-label="切换流程：合并菜单流程"]')).not.toBeNull()
    expect(toolbar.querySelector('[aria-label="切换流程：合并菜单流程"]')).toBeNull()
    for (const label of ['细分流程', '历史版本', '编辑记录', '复制内容', '删除记录', '新建流程', 'AI 生成流程']) {
      expect([...toolbar.querySelectorAll('button')].some(b => b.textContent?.trim() === label), label).toBe(true)
    }
    expect(document.querySelector('.memos__document .memos__document-actions')).toBeNull()
  })
  it('单行工具栏的流程菜单放在滚动区域外，菜单内真实按下不误关，外部按下关闭', async () => {
    const first: memo.MemoDocument = {id:'first',title:'原流程',category:'',kind:'flow',revision:1,createdAt:'',updatedAt:'',deletedAt:null,bodyMd:'',steps:[{id:'one',title:'原步骤',owner:'',detail:'原文'}]}
    const second: memo.MemoDocument = {...first,id:'second',title:'另一个流程'}
    const get = vi.spyOn(memo,'memoGet').mockImplementation(async id => id===first.id?first:second)
    await mount('',[first,second]);await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await act(async () => (document.querySelector('.flow-switcher__title') as HTMLButtonElement).click())
    const menu = document.querySelector('.flow-switcher__menu')!
    expect(menu.parentElement).toBe(document.body)
    const current = menu.querySelector<HTMLButtonElement>('[aria-current="page"]')
    expect(current?.textContent).toBe('原流程')
    expect(current?.disabled).toBe(true)
    const item = [...menu.querySelectorAll('button')].find(b=>b.textContent==='另一个流程')!
    await act(async () => item.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true})))
    expect(document.querySelector('.flow-switcher__menu')).toBe(menu)
    await act(async () => item.click())
    expect(get).toHaveBeenCalledWith(second.id)
    await act(async () => (document.querySelector('.flow-switcher__title') as HTMLButtonElement).click())
    await act(async () => document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true})))
    expect(document.querySelector('.flow-switcher__menu')).toBeNull()
  })
  it('流程名称下拉切换前保存当前卡片，读取失败或版本冲突保留草稿', async () => {
    const first: memo.MemoDocument = { id: 'first', title: '流程甲', category: '', kind: 'flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null, bodyMd: '', steps: [{ id: 'step-a', title: '操作甲', owner: '', detail: '原文甲' }] }
    const second: memo.MemoDocument = { ...first, id: 'second', title: '流程乙', steps: [{ id: 'step-b', title: '操作乙', owner: '', detail: '原文乙' }] }
    const get = vi.spyOn(memo, 'memoGet').mockImplementation(async id => id === first.id ? first : second)
    const saved = vi.spyOn(memo, 'memoSave').mockRejectedValueOnce(new Error('版本冲突，草稿保留')).mockImplementation(async input => ({ ...first, ...input, id: first.id, revision: 2 }))
    await mount('', [first, second])
    await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    await fill('第 1 步标题', '未保存的新操作')
    const switcher = document.querySelector('[aria-label="切换流程：流程甲"]') as HTMLButtonElement
    expect(switcher).not.toBeNull()
    await act(async () => switcher.click())
    await click('流程乙')
    expect(saved).toHaveBeenCalledOnce()
    expect(get).not.toHaveBeenCalledWith(second.id)
    expect((document.querySelector('[aria-label="第 1 步标题"]') as HTMLInputElement).value).toBe('未保存的新操作')
    expect(document.body.textContent).toContain('版本冲突，草稿保留')
    await click('流程乙')
    expect(saved.mock.calls[1]![0].steps[0]!.title).toBe('未保存的新操作')
    expect(get).toHaveBeenCalledWith(second.id)
    expect(document.querySelector('[aria-label="切换流程：流程乙"]')).not.toBeNull()
    expect(document.querySelector('.flow-canvas__node')?.textContent).toContain('原文乙')
  })
  it('点卡片外实际保存修改，写入失败保留输入，重试成功才退出编辑', async () => {
    const doc: memo.MemoDocument = { id:'outside', title:'原流程',category:'',kind:'flow',revision:1,createdAt:'',updatedAt:'',deletedAt:null,bodyMd:'',steps:[{id:'step',title:'原步骤',owner:'',detail:'原说明'},{id:'next',title:'下一个步骤',owner:'',detail:'下一段原文'}] }
    vi.spyOn(memo, 'memoGet').mockResolvedValue(doc)
    const save = vi.spyOn(memo, 'memoSave').mockRejectedValueOnce(new Error('写入失败，未保存')).mockImplementation(async input => ({...doc,...input,id:doc.id,revision:2}))
    await mount('', [doc])
    await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick',{bubbles:true})))
    await fill('第 1 步说明', '用户修改的说明')
    await act(async () => (document.querySelector('.flow-canvas__viewport') as HTMLElement).click())
    expect(save).toHaveBeenCalledOnce()
    expect((document.querySelector('[aria-label="第 1 步说明"]') as HTMLTextAreaElement).value).toBe('用户修改的说明')
    expect(document.body.textContent).toContain('写入失败，未保存')
    await act(async () => (document.querySelector('.flow-canvas__viewport') as HTMLElement).click())
    expect(save).toHaveBeenCalledTimes(2)
    expect(save.mock.calls[1]![0].steps[0]!.detail).toBe('用户修改的说明')
    expect(document.querySelector('.flow-canvas__inline-editor')).toBeNull()
    expect(document.querySelector('.flow-canvas__node')?.textContent).toContain('用户修改的说明')
    let resolve!: (saved: memo.MemoDocument) => void
    save.mockImplementationOnce(() => new Promise(r => { resolve = r }))
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick',{bubbles:true})))
    await fill('第 1 步说明', '继续修改后换到另一步')
    const next = document.querySelectorAll<HTMLElement>('.flow-canvas__node')[1]!
    await act(async () => { next.click(); next.click(); next.dispatchEvent(new MouseEvent('dblclick',{bubbles:true})) })
    expect(save).toHaveBeenCalledTimes(3)
    await act(async () => resolve({...doc,...save.mock.calls[2]![0],id:doc.id,revision:3}))
    expect(document.querySelector('[aria-label="第 2 步标题"]')).not.toBeNull()
  })
  it('已有流程细分先预览，取消不写库，确认保留原元数据且冲突保留预览', async () => {
    const original: memo.MemoDocument = { id: 'existing', title: '原流程', category: '原分类', kind: 'flow', revision: 4, createdAt: '', updatedAt: '', deletedAt: null, bodyMd: '原始材料归档', steps: [{ id: 'old', title: '2. 原章节', owner: '', detail: '原说明甲。\n\n原说明乙。' }] }
    vi.spyOn(memo, 'memoGet').mockResolvedValue(original)
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    const generate = vi.spyOn(ai, 'aiGenerateFlow').mockResolvedValue({ ...generated, steps: [{ id: 'a', title: '原说明甲。', owner: '', detail: '原说明甲。', group: {id:'source-1',title:'2. 原章节',path:[]} }, { id: 'b', title: '原说明乙。', owner: '', detail: '原说明乙。', group: {id:'source-1',title:'2. 原章节',path:[]} }] })
    const save = vi.spyOn(memo, 'memoSave').mockRejectedValueOnce(new Error('版本冲突，草稿保留')).mockImplementation(async input => ({ ...original, ...input, id: original.id, revision: 5 }))
    await mount('', [original]); await act(async () => (document.querySelector('.memos__item') as HTMLButtonElement).click())
    await click('细分流程'); await click('生成细分预览')
    expect(generate.mock.calls[0]![1]).toContain('原说明甲。\n\n原说明乙。')
    expect(generate.mock.calls[0]![1]).not.toContain('原始材料归档')
    expect(document.querySelector('[aria-label="流程细分预览"]')).not.toBeNull()
    await act(async () => vi.advanceTimersByTimeAsync(2000)); expect(save).not.toHaveBeenCalled()
    await click('取消细分'); expect(document.querySelector('.flow-canvas__node')?.textContent).toContain('原说明甲。')
    await click('细分流程'); await click('生成细分预览'); await click('确认保存细分')
    expect(document.body.textContent).toContain('版本冲突，草稿保留')
    expect(document.querySelector('[aria-label="流程细分预览"]')).not.toBeNull()
    await click('确认保存细分')
    expect(save.mock.calls[1]![0]).toEqual(expect.objectContaining({id:'existing',expectedRevision:4,title:'原流程',category:'原分类',bodyMd:'原始材料归档',steps:expect.arrayContaining([expect.objectContaining({id:'a'}),expect.objectContaining({id:'b'})])}))
    expect(document.querySelector('[aria-label="流程细分预览"]')).toBeNull()
  })
  it('新流程默认在可编辑画布中显示，修改步骤后沿用自动保存', async () => {
    const saved = vi.spyOn(memo, 'memoSave').mockImplementation(async input => ({ ...input, id: 'canvas-flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null }))
    await mount()
    await click('新建流程')
    expect(document.querySelector('[aria-label="流程画布"]')).toBeTruthy()
    expect(document.querySelector('.memos--canvas')).toBeTruthy()
    await click('流程信息')
    await fill('备忘标题', 'ERP 发货流程')
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    await fill('第 1 步标题', '创建发货单')
    await act(async () => vi.advanceTimersByTimeAsync(1100))
    expect(saved).toHaveBeenCalledWith(expect.objectContaining({ steps: [expect.objectContaining({ title: '创建发货单' })] }))
    expect(document.querySelector('[aria-label="流程画布"]')).toBeTruthy()
    expect((document.querySelector('[aria-label="第 1 步标题"]') as HTMLInputElement).value).toBe('创建发货单')
  })
  const config: ai.ProviderConfig = { provider: 'custom', baseUrl: 'http://localhost:11434/v1', model: 'vision-test', timeoutSeconds: 30, maxOutputTokens: 2048, hasApiKey: true }
  const generated: memo.SaveMemoInput = { id: null, expectedRevision: null, title: '订单核对流程', category: '销售', kind: 'flow', bodyMd: '## 原始材料\n\n张三：先核对订单，再通知仓库。', steps: [{ id: 'step-1', title: '核对订单', owner: '待确认', detail: '核对订单数量。' }] }
  it('可把 AI 原始图片放进已有步骤，待关联数量更新，确认前不保存或重新调用 AI', async () => {
    const image: assets.ContentAsset = { id:'00000000-0000-7000-8000-000000000001',name:'发货表.png',mime:'image/png',dataBase64:'',byteSize:24,sha256:'hash',createdAt:'' }
    vi.spyOn(assets, 'assetGet').mockResolvedValue(image)
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    const body = `${generated.bodyMd}\n${assets.assetMarkdown(image)}`
    const generate = vi.spyOn(ai, 'aiGenerateFlow').mockResolvedValue({ ...generated, bodyMd: body })
    const save = vi.spyOn(memo, 'memoSave').mockImplementation(async input => ({ ...input, id:'new-flow',revision:1,createdAt:'',updatedAt:'',deletedAt:null }))
    await mount(); await click('AI 生成流程'); await fill('流程原始材料', '根据原图整理'); await click('生成流程草稿')
    await click('流程信息')
    expect(document.body.textContent).toContain('还有 1 张原图未关联步骤')
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    const picker = document.querySelector<HTMLElement>('.flow-image-picker [aria-label="选择原图"]')
    expect(picker).toBeTruthy()
    expect(document.querySelector('.flow-image-picker select')).toBeNull()
    await act(async () => picker!.click())
    await click('添加到此步骤')
    expect((document.querySelector('[aria-label="第 1 步说明"]') as HTMLTextAreaElement).value).toContain(assets.assetMarkdown(image))
    await click('流程信息')
    expect(document.body.textContent).toContain('原图均已关联步骤')
    await act(async () => vi.advanceTimersByTimeAsync(2000))
    expect(save).not.toHaveBeenCalled(); expect(generate).toHaveBeenCalledOnce()
    await click('确认保存流程')
    expect(save).toHaveBeenCalledWith(expect.objectContaining({bodyMd:body,steps:[expect.objectContaining({detail:expect.stringContaining(assets.assetMarkdown(image))})]}))
  })
  it('图片仍在导入时不能生成，避免漏发所选截图', async () => {
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    vi.spyOn(assets, 'assetImportFile').mockReturnValue(new Promise(() => {}))
    const generate = vi.spyOn(ai, 'aiGenerateFlow')
    await mount()
    await click('AI 生成流程')
    await fill('流程原始材料', '请结合即将添加的截图整理流程。')
    const input = document.querySelector('dialog input[type=file]')!
    Object.defineProperty(input, 'files', { value: [new File(['image'], '截图.png', { type: 'image/png' })] })
    await act(async () => input.dispatchEvent(new Event('change', { bubbles: true })))
    const button = [...document.querySelectorAll('button')].find(b => b.textContent === '生成流程草稿')!
    expect(button.disabled).toBe(true)
    await act(async () => button.click())
    expect(generate).not.toHaveBeenCalled()
  })
  it('确认放弃修改后取消生成，不会恢复保存已放弃的草稿', async () => {
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    const save = vi.spyOn(memo, 'memoSave')
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '这份草稿要放弃')
    await click('AI 生成流程')
    expect(window.confirm).toHaveBeenCalled()
    await click('取消生成')
    await act(async () => vi.advanceTimersByTimeAsync(2000))
    expect(save).not.toHaveBeenCalled()
    expect(document.querySelector('[aria-label="备忘标题"]')).toBeNull()
  })
  it('AI 草稿和修改都不自动写库，确认后只保存新的流程', async () => {
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    const generate = vi.spyOn(ai, 'aiGenerateFlow').mockResolvedValue({ ...generated, id: 'existing-record', expectedRevision: 8 })
    const save = vi.spyOn(memo, 'memoSave').mockImplementation(async input => ({ ...input, id: 'new-flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null }))
    await mount()
    await click('AI 生成流程')
    await fill('流程原始材料', '张三：先核对订单，再通知仓库。')
    await click('生成流程草稿')
    expect(generate).toHaveBeenCalledWith(config, '张三：先核对订单，再通知仓库。')
    await act(async () => vi.advanceTimersByTimeAsync(10000))
    await fill('第 1 步负责人', '销售')
    await act(async () => vi.advanceTimersByTimeAsync(2000))
    expect(save).not.toHaveBeenCalled()
    expect(document.body.textContent).toContain('AI 草稿待确认')
    await click('确认保存流程')
    expect(save).toHaveBeenCalledOnce()
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: null, expectedRevision: null, bodyMd: generated.bodyMd, steps: [expect.objectContaining({ owner: '销售' })] }))
    expect(document.body.textContent).toContain('已保存到本机')
    expect(document.querySelector('.memos__save-status')).toBeNull()
  })
  it('生成失败保留材料，重试生成后取消草稿仍不保存', async () => {
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(config)
    vi.spyOn(ai, 'aiGenerateFlow').mockRejectedValueOnce(new Error('当前模型不支持图片')).mockResolvedValue(generated)
    const save = vi.spyOn(memo, 'memoSave')
    await mount()
    await click('AI 生成流程')
    await fill('流程原始材料', '截图对应的聊天材料')
    await click('生成流程草稿')
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('当前模型不支持图片')
    expect((document.querySelector('[aria-label="流程原始材料"]') as HTMLTextAreaElement).value).toBe('截图对应的聊天材料')
    await click('生成流程草稿')
    await click('取消编辑')
    await act(async () => vi.advanceTimersByTimeAsync(2000))
    expect(save).not.toHaveBeenCalled()
  })
  it('AI 流程入口接受文字与截图材料，打开不会保存记录', async () => {
    vi.spyOn(ai, 'aiGetConfig').mockResolvedValue(null)
    const save = vi.spyOn(memo, 'memoSave')
    await mount()
    await click('AI 生成流程')
    expect(document.querySelector('[aria-label="流程原始材料"]')).toBeTruthy()
    expect(document.body.textContent).toContain('点击生成才会发送')
    await act(async () => vi.advanceTimersByTimeAsync(2000))
    expect(save).not.toHaveBeenCalled()
  })
  it('未填写步骤标题的流程也能自动保存，图片链接与说明保留', async () => {
    const save = vi.spyOn(memo, 'memoSave').mockImplementation(async input => ({ ...input, id: 'draft-flow', revision: 1, createdAt: '', updatedAt: '', deletedAt: null }))
    await mount()
    await click('新建流程')
    await click('流程信息')
    await fill('备忘标题', '还在整理的流程')
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    await fill('第 1 步说明', '稍后补充步骤名称，先记录操作')
    expect((Array.from(document.querySelectorAll('button')).find(b => b.textContent === '保存并查看') as HTMLButtonElement).disabled).toBe(false)
    await act(async () => vi.advanceTimersByTimeAsync(1000))
    expect(save).toHaveBeenCalledOnce()
    expect(save.mock.calls[0]![0].steps[0]).toMatchObject({ title: '', detail: '稍后补充步骤名称，先记录操作' })
  })
  it('输入停顿自动保存，写入期间的新文字保留并进入下一次保存', async () => {
    let finish!: (doc: memo.MemoDocument) => void
    const save = vi.spyOn(memo, 'memoSave').mockImplementationOnce(() => new Promise(r => { finish = r })).mockImplementation(async input => ({ ...input, id: 'saved-id', revision: 2, createdAt: '', updatedAt: '', deletedAt: null }))
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '家里流程')
    await fill('备忘内容', '第一版')
    await act(async () => vi.advanceTimersByTimeAsync(1000))
    expect(save).toHaveBeenCalledTimes(1)
    await fill('备忘内容', '第二版，不应丢失')
    await act(async () => finish({ id: 'saved-id', title: '家里流程', category: '', kind: 'memo', bodyMd: '第一版', steps: [], revision: 1, createdAt: '', updatedAt: '', deletedAt: null }))
    expect((document.querySelector('[aria-label="备忘内容"]') as HTMLTextAreaElement).value).toBe('第二版，不应丢失')
    await act(async () => vi.advanceTimersByTimeAsync(1000))
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ id: 'saved-id', expectedRevision: 1, bodyMd: '第二版，不应丢失' }))
  })
  it('自动保存接受后端去除标题首尾空白的结果，正文不丢失也不重复保存', async () => {
    const save = vi.spyOn(memo, 'memoSave').mockImplementation(async input => ({ ...input, title: input.title.trim(), category: input.category.trim(), id: 'trimmed', revision: 1, createdAt: '', updatedAt: '', deletedAt: null }))
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '  原标题  ')
    await fill('备忘内容', '原始正文')
    await act(async () => vi.advanceTimersByTimeAsync(4000))
    expect(save).toHaveBeenCalledOnce()
    expect((document.querySelector('[aria-label="备忘标题"]') as HTMLInputElement).value).toBe('原标题')
    expect((document.querySelector('[aria-label="备忘内容"]') as HTMLTextAreaElement).value).toBe('原始正文')
  })
  const row: memo.MemoSummary = {
    id: 'audit-own',
    title: '验收备忘',
    category: '学习',
    kind: 'memo',
    revision: 1,
    createdAt: '',
    updatedAt: '',
    deletedAt: null,
  }
  it('搜索无匹配时仍显示生效中的分类，用户可以清除筛选', async () => {
    await mount('', [row])
    const select = document.querySelector(
      '[aria-label="备忘分类筛选"]',
    ) as HTMLSelectElement
    await act(async () => {
      select.value = '学习'
      select.dispatchEvent(new Event('change', { bubbles: true }))
    })
    vi.mocked(memo.memoList).mockResolvedValue([])
    await act(async () => root!.render(<MemosView query="无匹配" />))
    await act(async () => vi.advanceTimersByTimeAsync(250))
    expect(select.value).toBe('学习')
    expect(select.selectedOptions[0]?.textContent).toBe('学习')
  })
  it('搜索条件改变后，防抖期间旧请求不能覆盖列表', async () => {
    let resolve!: (rows: memo.MemoSummary[]) => void
    await mount(
      '',
      new Promise((r) => {
        resolve = r
      }),
    )
    await act(async () => root!.render(<MemosView query="新搜索" />))
    await act(async () => resolve([row]))
    expect(document.querySelector('.memos__item')).toBeNull()
    vi.mocked(memo.memoList).mockResolvedValue([{ ...row, title: '新的结果' }])
    await act(async () => vi.advanceTimersByTimeAsync(250))
    expect(document.querySelector('.memos__item')?.textContent).toContain(
      '新的结果',
    )
  })
  it('列表刷新成功不会抹掉保存失败的原因', async () => {
    vi.spyOn(memo, 'memoSave').mockRejectedValue(new Error('版本冲突验收'))
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '保留草稿')
    await click('保存并查看')
    await click('刷新列表')
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      '版本冲突验收',
    )
  })
  it('添加步骤并调序，按用户顺序保存，保存后展示负责人和路线', async () => {
    const save = vi
      .spyOn(memo, 'memoSave')
      .mockImplementation(async (input) => ({
        ...input,
        id: 'own-test',
        revision: 1,
        createdAt: '2026-09-29',
        updatedAt: '2026-09-29',
        deletedAt: null,
      }))
    await mount()
    await click('新建流程')
    await click('流程信息')
    await fill('备忘标题', '订单流程验收')
    await act(async () => document.querySelector('.flow-canvas__node')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    await fill('第 1 步标题', '审核')
    await fill('第 1 步负责人', '运营')
    await click('添加步骤')
    await fill('第 2 步标题', '接收资料')
    await fill('第 2 步说明', '核对文件')
    await act(async () => (document.querySelector('[aria-label="第 2 步操作"]') as HTMLButtonElement).click())
    await click('上移')
    await click('保存并查看')
    expect(save).toHaveBeenCalledOnce()
    expect(save.mock.calls[0]![0].steps.map((s) => s.title)).toEqual([
      '接收资料',
      '审核',
    ])
    expect(
      [...document.querySelectorAll('.memos__node h3')].map(
        (el) => el.textContent,
      ),
    ).toEqual(['接收资料', '审核'])
    expect(document.body.textContent).toContain('负责人：运营')
    expect(document.body.textContent).toContain('已保存到本机')
    expect(document.querySelector('.memos__save-status')).toBeNull()
  })
  it('保存失败保留全部草稿，不呈现保存成功', async () => {
    vi.spyOn(memo, 'memoSave').mockRejectedValue(new Error('磁盘写入失败'))
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '未保存的内容')
    await fill('备忘内容', '需要保留的操作说明')
    await click('保存并查看')
    expect(document.body.textContent).toContain('磁盘写入失败')
    expect(
      (document.querySelector('[aria-label="备忘内容"]') as HTMLTextAreaElement)
        .value,
    ).toBe('需要保留的操作说明')
    expect(document.body.textContent).not.toContain('已保存到本机')
  })
  it('输入搜索走备忘接口，拒绝放弃时保留草稿', async () => {
    await mount('负责人名字')
    expect(memo.memoList).toHaveBeenCalledWith('负责人名字', false)
    await click('新建备忘')
    await fill('备忘标题', '草稿')
    vi.mocked(window.confirm).mockReturnValue(false)
    await click('新建流程')
    expect(
      (document.querySelector('[aria-label="备忘标题"]') as HTMLInputElement)
        .value,
    ).toBe('草稿')
    expect(document.querySelector('[aria-label="第 1 步标题"]')).toBeNull()
  })
  it('Markdown 预览不执行 HTML 或危险链接', async () => {
    vi.spyOn(memo, 'memoSave').mockImplementation(async (input) => ({
      ...input,
      id: 'own-test',
      revision: 1,
      createdAt: '',
      updatedAt: '',
      deletedAt: null,
    }))
    await mount()
    await click('新建备忘')
    await fill('备忘标题', '预览安全')
    await fill(
      '备忘内容',
      '<script>window.hacked=true</script>\n\n[链接](javascript:alert(1))',
    )
    await click('保存并查看')
    expect(document.querySelector('.memos__reading script')).toBeNull()
    expect(
      document.querySelector('.memos__reading a[href^="javascript:"]'),
    ).toBeNull()
  })
})
