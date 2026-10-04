// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { KnowledgeBase } from './KnowledgeBase'
import * as api from '../lib/knowledge-base-ipc'
import * as assets from '../lib/content-assets'
import { save } from '@tauri-apps/plugin-dialog'

vi.mock('../lib/data-change', () => ({ onDataChanged: () => () => {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }))
vi.mock('../lib/knowledge-base-ipc', () => ({
  knowledgeAsk: vi.fn(),
  knowledgeDelete: vi.fn(),
  knowledgeImportFile: vi.fn(),
  knowledgeList: vi.fn(),
  knowledgeSearch: vi.fn(),
}))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

const flowCitation: api.KnowledgeCitation = {
  id: 'E1', sourceKind: 'flow', sourceId: 'flow-1', title: '发票流程', category: '财务',
  assetId: null, flowId: 'flow-1', stepId: 'step-3', revision: 4, contentHash: null,
  locator: '第 3 步 · 美国发票', excerpt: '核对抬头后提交税务系统。', startOffset: null,
}
const documentCitation: api.KnowledgeCitation = {
  ...flowCitation, id: 'doc:1', sourceKind: 'document', sourceId: 'source-1', title: '美国发票指南.pdf',
  category: null, assetId: 'asset-1', flowId: null, stepId: null, revision: null,
  contentHash: 'abc123', locator: '第 2 页；发票处理', excerpt: '核对抬头与税号。', startOffset: 120,
}

let root: Root | undefined
let host: HTMLDivElement | undefined

async function mount(onOpenFlow = vi.fn()) {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  vi.mocked(api.knowledgeList).mockResolvedValue([])
  await act(async () => root!.render(<KnowledgeBase onOpenFlow={onOpenFlow} />))
  return onOpenFlow
}

it('在提问前明确说明最近对话也会发送给 AI 服务商', async () => {
  await mount()
  expect(document.body.textContent).toContain('当前问题、最近必要对话和少量相关摘录会发送')
  expect(document.body.textContent).toContain('导入时不会上传整份文件')
})

async function enterQuestion(value: string) {
  const textarea = document.querySelector('textarea')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, value)
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  host?.remove(); host = undefined
  vi.restoreAllMocks()
  vi.mocked(save).mockReset()
  vi.clearAllMocks()
})

it('回答展示真实引用，并能从流程引用跳转到具体节点', async () => {
  const openFlow = await mount()
  vi.mocked(api.knowledgeAsk).mockResolvedValue({
    status: 'answered', answer: '先核对抬头，再提交。', searchTerms: ['美国发票'],
    citations: [flowCitation], flowCandidates: [], message: null, providerNotice: '问题和摘录将发送给 AI 服务商。',
  })
  await enterQuestion('美国发票下一步怎么做')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())

  expect(document.body.textContent).toContain('先核对抬头，再提交。')
  expect(document.body.textContent).toContain('核对抬头后提交税务系统。')
  await act(async () => [...document.querySelectorAll('button')].find(button => button.textContent === '打开并定位到流程节点')!.click())
  expect(openFlow).toHaveBeenCalledWith('flow-1', 'step-3')
})

it('命中多个流程时要求选择，选择后把明确的流程编号交给后端', async () => {
  await mount()
  vi.mocked(api.knowledgeAsk)
    .mockResolvedValueOnce({
      status: 'clarify', answer: null, searchTerms: ['发票'], citations: [flowCitation],
      flowCandidates: [{ flowId: 'flow-1', title: '发票流程', category: '财务', evidence: '步骤 3：核对抬头' }],
      message: '命中了多个流程。请选择。', providerNotice: '',
    })
    .mockResolvedValueOnce({
      status: 'answered', answer: '按发票流程第 3 步核对抬头。', searchTerms: ['发票'],
      citations: [flowCitation], flowCandidates: [], message: null, providerNotice: '',
    })
  await enterQuestion('發票怎么做')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())
  expect(document.body.textContent).toContain('请选择要查询的流程')
  await act(async () => [...document.querySelectorAll('button')].find(button => button.textContent?.includes('发票流程'))!.click())
  expect(api.knowledgeAsk).toHaveBeenNthCalledWith(2, {
    question: '發票怎么做', history: [], selectedFlowId: 'flow-1',
  })
  expect(document.body.textContent).toContain('按发票流程第 3 步核对抬头。')
})

it('资料引用显示解析位置并允许导出原件核对', async () => {
  await mount()
  vi.mocked(api.knowledgeAsk).mockResolvedValue({
    status: 'answered', answer: '先核对发票抬头。', searchTerms: ['发票'],
    citations: [documentCitation], flowCandidates: [], message: null, providerNotice: '',
  })
  vi.mocked(save).mockResolvedValue('C:/Temp/美国发票指南.pdf')
  const exportAsset = vi.spyOn(assets, 'assetExport').mockResolvedValue()
  await enterQuestion('核对发票什么内容')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())
  expect(document.body.textContent).toContain('第 2 页；发票处理')
  await act(async () => [...document.querySelectorAll('button')].find(button => button.textContent === '导出引用原件')!.click())
  expect(exportAsset).toHaveBeenCalledWith('asset-1', 'C:/Temp/美国发票指南.pdf')
})
