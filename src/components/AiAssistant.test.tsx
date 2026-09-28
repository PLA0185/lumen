// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import * as ai from '../lib/ai-ipc'
import { AiAssistant } from './AiAssistant'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})
async function mount(hasApiKey = true) {
  vi.spyOn(ai, 'aiGetConfig').mockResolvedValue({ provider: 'custom', baseUrl: 'http://localhost:1', model: 'test-model', hasApiKey, timeoutSeconds: 10, maxOutputTokens: 500 })
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<AiAssistant />))
}
async function click(text: string) {
  const button = Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text))!
  expect(button).toBeTruthy()
  await act(async () => button.click())
}
async function fill(selector: string, value: string) {
  const el = document.querySelector(selector) as HTMLInputElement
  const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value)
    el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }))
  })
}
describe('AI 助手完整交互（模拟模型，真实 React 状态）', () => {
  it('生成不自动保存，允许清空与改标题，只应用选中条目，并选择自然月年总结', async () => {
    const preview: ai.DiffPreview = { previewId: 'test-preview', capability: '整理任务', summary: '2 条', acceptable: true, raw: '{}', issues: [], usage: null, dataScopeNote: '测试范围', items: ['候选一','候选二'].map((title) => ({ action: 'create', taskId: null, title, changes: [], note: null })) }
    const organize = vi.spyOn(ai, 'aiOrganize').mockResolvedValue(preview)
    const edit = vi.spyOn(ai, 'aiEditPreview').mockResolvedValue(preview.items)
    const apply = vi.spyOn(ai, 'aiApply').mockResolvedValue({ created: 1, updated: 0, skipped: 1 })
    const review = vi.spyOn(ai, 'aiReview').mockResolvedValue({ text: '测试总结', summary: '测试月份', stats: {}, usage: null, dataScopeNote: '测试范围' })
    await mount()
    await fill('[aria-label="发送给 AI 的文本"]', '明天整理资料，周五交付计划')
    await click('发送并生成')
    expect(organize).toHaveBeenCalledOnce()
    expect(apply).not.toHaveBeenCalled()
    await fill('[aria-label="第 1 条任务标题"]', '')
    expect((document.querySelector('[aria-label="第 1 条任务标题"]') as HTMLInputElement).value).toBe('')
    await fill('[aria-label="第 1 条任务标题"]', '用户修改的标题')
    await act(async () => (document.querySelector('[aria-label="接受第 2 条：候选二"]') as HTMLInputElement).click())
    await click('写入')
    expect(edit).toHaveBeenCalledWith('test-preview', [{ index: 0, field: 'title', value: '用户修改的标题' }])
    expect(apply).toHaveBeenCalledWith('test-preview', [0])
    await click('总结与复盘')
    await fill('[aria-label="AI 时间范围"]', 'monthly')
    await fill('[aria-label="总结所在日期"]', '2024-02-29')
    await click('生成总结')
    expect(review).toHaveBeenLastCalledWith(expect.anything(), 'monthly', '2024-02-29', '明天整理资料，周五交付计划')
    await fill('[aria-label="AI 时间范围"]', 'yearly')
    await click('生成总结')
    expect(review).toHaveBeenLastCalledWith(expect.anything(), 'yearly', '2024-02-29', expect.any(String))
  })
  it('未配置密钥时说明原因且不调用模型', async () => {
    const organize = vi.spyOn(ai, 'aiOrganize')
    await mount(false)
    await fill('[aria-label="发送给 AI 的文本"]', '生成待办')
    expect(document.body.textContent).toContain('尚未配置 AI 密钥')
    const button = Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes('发送并生成'))!
    expect(button.disabled).toBe(true)
    expect(organize).not.toHaveBeenCalled()
  })
})
