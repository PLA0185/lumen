// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import * as memo from '../lib/memos-ipc'
import * as ai from '../lib/ai-ipc'
import { MemosView } from './MemosView'

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
  const config: ai.ProviderConfig = { provider: 'custom', baseUrl: 'http://localhost:11434/v1', model: 'vision-test', timeoutSeconds: 30, maxOutputTokens: 2048, hasApiKey: true }
  const generated: memo.SaveMemoInput = { id: null, expectedRevision: null, title: '订单核对流程', category: '销售', kind: 'flow', bodyMd: '## 原始材料\n\n张三：先核对订单，再通知仓库。', steps: [{ id: 'step-1', title: '核对订单', owner: '待确认', detail: '核对订单数量。' }] }
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
    await fill('备忘标题', '还在整理的流程')
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
    await fill('备忘标题', '订单流程验收')
    await fill('第 1 步标题', '审核')
    await fill('第 1 步负责人', '运营')
    await click('添加步骤')
    await fill('第 2 步标题', '接收资料')
    await fill('第 2 步说明', '核对文件')
    await act(async () =>
      (
        document.querySelector(
          '[aria-label="第 2 步上移"]',
        ) as HTMLButtonElement
      ).click(),
    )
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
