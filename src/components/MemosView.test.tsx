// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import * as memo from '../lib/memos-ipc'
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
