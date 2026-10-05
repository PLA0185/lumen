// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { KnowledgeBase } from './KnowledgeBase'
import * as api from '../lib/knowledge-base-ipc'
import * as assets from '../lib/content-assets'
import { save } from '@tauri-apps/plugin-dialog'

const dataChanges = vi.hoisted(() => ({ listeners: new Map<string, Set<() => void>>() }))
vi.mock('../lib/data-change', () => ({
  onDataChanged: (domains: string[], callback: () => void) => {
    for (const domain of domains) {
      const listeners = dataChanges.listeners.get(domain) ?? new Set<() => void>()
      listeners.add(callback)
      dataChanges.listeners.set(domain, listeners)
    }
    return () => { for (const listeners of dataChanges.listeners.values()) listeners.delete(callback) }
  },
}))
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

async function mount(onOpenFlow = vi.fn(), initialSources: api.KnowledgeSourceSummary[] = []) {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  vi.mocked(api.knowledgeList).mockResolvedValue(initialSources)
  await act(async () => root!.render(<KnowledgeBase onOpenFlow={onOpenFlow} />))
  return onOpenFlow
}

it('在提问前明确说明最近对话也会发送给 AI 服务商', async () => {
  await mount()
  expect(document.body.textContent).toContain('当前问题、最近必要对话和少量相关摘录会发送')
  expect(document.body.textContent).toContain('选择“全部业务数据”后，二者会加密同步到其他电脑')
  expect(document.body.textContent).toContain('已保存流程按“备忘与流程”范围同步并自动纳入检索')
  expect(document.body.textContent).toContain('导入图片或扫描页时，也会逐张发送给已配置的多模态 AI 识别')
  expect(document.body.textContent).toContain('失败后改用本机 PaddleOCR，不会上传整份文件')
})

async function enterQuestion(value: string) {
  const textarea = document.querySelector('textarea')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, value)
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

async function publishDataChange(domain: string) {
  await act(async () => { for (const listener of dataChanges.listeners.get(domain) ?? []) listener() })
}

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  host?.remove(); host = undefined
  vi.restoreAllMocks()
  vi.mocked(save).mockReset()
  vi.clearAllMocks()
  dataChanges.listeners.clear()
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
  expect(document.querySelector<HTMLDetailsElement>('.knowledge-answer__notice')?.open).toBe(false)
  expect(document.querySelector<HTMLDetailsElement>('.knowledge-answer__evidence')?.open).toBe(false)
  expect(document.querySelector<HTMLDetailsElement>('.knowledge-evidence__excerpt')?.open).toBe(false)
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
    question: '發票怎么做', history: [], selectedFlowId: 'flow-1', selectedSourceIds: [],
  })
  expect(document.body.textContent).toContain('按发票流程第 3 步核对抬头。')
})

it('默认检索全部来源，也支持限定一个或多个上传文件', async () => {
  const sourceA: api.KnowledgeSourceSummary = {
    id: 'source-a', assetId: 'asset-a', title: '美国发票.pdf', mime: 'application/pdf', byteSize: 100,
    sha256: 'hash-a', status: 'ready', warnings: [], error: null, createdAt: '', updatedAt: '',
  }
  const sourceB: api.KnowledgeSourceSummary = {
    ...sourceA, id: 'source-b', assetId: 'asset-b', title: '英国发票.pdf', sha256: 'hash-b',
  }
  await mount(vi.fn(), [sourceA, sourceB])
  vi.mocked(api.knowledgeAsk).mockResolvedValue({
    status: 'answered', answer: '先核对发票信息。', searchTerms: ['发票'],
    citations: [documentCitation], flowCandidates: [], message: null, providerNotice: '',
  })
  vi.mocked(api.knowledgeSearch).mockResolvedValue({ citations: [] })

  const scope = document.querySelector<HTMLSelectElement>('#knowledge-scope')!
  expect(scope.value).toBe('all')
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(scope, 'selected')
    scope.dispatchEvent(new Event('change', { bubbles: true }))
  })
  const checkSource = async (id: string) => {
    const checkbox = document.querySelector<HTMLInputElement>(`input[name="knowledge-source"][value="${id}"]`)!
    await act(async () => checkbox.click())
  }
  await checkSource('source-a')
  await checkSource('source-b')
  expect([...document.querySelectorAll<HTMLInputElement>('input[name="knowledge-source"]')].map(input => input.checked))
    .toEqual([true, true])
  await enterQuestion('发票下一步怎么做？')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())

  expect(api.knowledgeAsk).toHaveBeenCalledWith({
    question: '发票下一步怎么做？', history: [], selectedFlowId: null,
    selectedSourceIds: ['source-a', 'source-b'],
  })

  const directSearch = document.querySelector<HTMLInputElement>('#knowledge-search')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(directSearch, '税号')
    directSearch.dispatchEvent(new Event('input', { bubbles: true }))
    await vi.waitFor(() => expect(api.knowledgeSearch).toHaveBeenCalledWith('税号', ['source-a', 'source-b']))
  })
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

it('导入成功提示不复述资料卡中的解析告警，并合并重复的原件保留提示', async () => {
  const warning = '内嵌图片 image6.png 的 OCR 未完成：Windows OCR 未识别到文字。原文件已保留；原件已保留。'
  const source: api.KnowledgeSourceSummary = {
    id: 'source-3', assetId: 'asset-3', title: 'TK-WM出货SOP.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    byteSize: 2048, sha256: 'abc', status: 'ready', warnings: [warning], error: null,
    createdAt: '2026-10-04', updatedAt: '2026-10-04',
  }
  vi.mocked(api.knowledgeImportFile).mockResolvedValue({ source, duplicate: false })
  await mount()
  vi.mocked(api.knowledgeList).mockResolvedValue([source])
  const input = document.querySelector<HTMLInputElement>('input[type="file"]')!
  Object.defineProperty(input, 'files', { configurable: true, value: [new File(['SOP'], source.title)] })
  await act(async () => {
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await vi.waitFor(() => expect(api.knowledgeList).toHaveBeenCalledTimes(2))
  })

  expect(document.querySelector('.knowledge-message')?.textContent).toBe(`已解析「${source.title}」并加入检索。`)
  expect(document.querySelector('.knowledge-sources__warning')?.textContent)
    .toBe('内嵌图片 image6.png 的 OCR 未完成：Windows OCR 未识别到文字。原文件已保留。')
  expect(document.body.textContent?.match(/原件已保留|原文件已保留/g)).toHaveLength(1)
})

it('导入期间实时显示解析阶段和图片识别计数', async () => {
  await mount()
  const source: api.KnowledgeSourceSummary = {
    id: 'progress-source', assetId: 'progress-asset', title: '出货流程.docx',
    mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    byteSize: 2048, sha256: 'progress-hash', status: 'ready', warnings: [], error: null,
    createdAt: '', updatedAt: '',
  }
  let finishImport!: (result: { source: api.KnowledgeSourceSummary; duplicate: boolean }) => void
  let reportProgress!: (progress: api.KnowledgeImportProgress) => void
  vi.mocked(api.knowledgeImportFile).mockImplementationOnce((_file, onProgress) => new Promise(resolve => {
    finishImport = resolve
    reportProgress = onProgress!
  }))
  const input = document.querySelector<HTMLInputElement>('input[type="file"]')!
  Object.defineProperty(input, 'files', { configurable: true, value: [new File(['SOP'], source.title)] })
  await act(async () => {
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await vi.waitFor(() => expect(api.knowledgeImportFile).toHaveBeenCalledTimes(1))
  })

  await act(async () => reportProgress({
    phase: 'ocr', message: '正在识别内嵌图片 image29.png（多模态 AI）', current: 29, total: 30,
  }))
  expect(document.body.textContent).toContain('正在识别内嵌图片 image29.png（多模态 AI）')
  expect(document.body.textContent).toContain('29/30')

  await act(async () => finishImport({ source, duplicate: false }))
})

it('资料清单默认使用紧凑行，MIME 和长解析告警收纳在折叠详情内', async () => {
  const source: api.KnowledgeSourceSummary = {
    id: 'compact-source', assetId: 'compact-asset', title: 'TK-WM出货SOP.docx',
    mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    byteSize: 3_800_000, sha256: 'compact-hash', status: 'ready',
    warnings: ['已提取正文、表格和内嵌图片；图片文字识别完成：多模态 AI 45 张，本机 PaddleOCR 兜底 4 张。请核对识别结果。'],
    error: null, createdAt: '', updatedAt: '',
  }
  await mount()
  vi.mocked(api.knowledgeList).mockResolvedValue([source])
  await publishDataChange('knowledge')

  const row = document.querySelector('.knowledge-sources__item')
  expect(row).not.toBeNull()
  expect(row?.querySelector('.knowledge-sources__actions')).not.toBeNull()
  const details = row?.querySelector<HTMLDetailsElement>('details')
  expect(details?.open).toBe(false)
  expect(details?.querySelector('.knowledge-sources__warning')?.textContent).toContain('45 张')
  expect(row?.querySelector('.knowledge-sources__mime')?.textContent).toContain('application/vnd.')
})

it('流程内容变化只刷新流程检索，不反复重载资料列表', async () => {
  await mount()
  expect(api.knowledgeList).toHaveBeenCalledTimes(1)

  await publishDataChange('memos')

  expect(api.knowledgeList).toHaveBeenCalledTimes(1)
  expect(document.body.textContent).not.toContain('正在读取资料列表…')
})

it('刷新资料清单时保留已显示内容，不闪回初始读取状态', async () => {
  await mount()
  let finishReload!: (rows: api.KnowledgeSourceSummary[]) => void
  vi.mocked(api.knowledgeList).mockImplementationOnce(() => new Promise(resolve => { finishReload = resolve }))

  await publishDataChange('knowledge')

  expect(document.body.textContent).not.toContain('正在读取资料列表…')
  const source: api.KnowledgeSourceSummary = {
    id: 'source-2', assetId: 'asset-2', title: '出货标签指南.pdf', mime: 'application/pdf',
    byteSize: 2048, sha256: 'abc', status: 'ready', warnings: [], error: null,
    createdAt: '2026-10-04', updatedAt: '2026-10-04',
  }
  await act(async () => finishReload([source]))
  expect(document.body.textContent).toContain('出货标签指南.pdf')
})

it('提问等待期间显示状态，AI 调用失败时明确展示错误', async () => {
  await mount()
  let failAsk!: (reason: Error) => void
  vi.mocked(api.knowledgeAsk).mockImplementationOnce(() => new Promise((_, reject) => { failAsk = reject }))
  await enterQuestion('工厂刚发来重量和体积，下一步做什么？')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())
  expect(api.knowledgeAsk).toHaveBeenCalledTimes(1)
  expect(document.body.textContent).toContain('正在检索并核对…')

  await act(async () => failAsk(new Error('AI 请求超时')))
  expect(document.body.textContent).toContain('AI 请求超时')
})

it('资料变化事件不会静默取消正在进行的问答', async () => {
  await mount()
  let finishAsk!: (result: api.KnowledgeAskResult) => void
  vi.mocked(api.knowledgeAsk).mockImplementationOnce(() => new Promise(resolve => { finishAsk = resolve }))
  await enterQuestion('工厂发来重量体积和运费，接下来做什么？')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())

  await publishDataChange('memos')
  expect(document.body.textContent).toContain('正在检索并核对…')

  await act(async () => finishAsk({
    status: 'answered', answer: '按流程先核对货件资料。', searchTerms: ['货件资料'],
    citations: [flowCitation], flowCandidates: [], message: null, providerNotice: '',
  }))
  expect(document.body.textContent).toContain('按流程先核对货件资料。')
})

it('回答展示后，资料或流程变化不会自动清空回答', async () => {
  await mount()
  vi.mocked(api.knowledgeAsk).mockResolvedValue({
    status: 'answered', answer: '先打印 FBA 标签，再核对货件。', searchTerms: ['FBA 标签'],
    citations: [flowCitation], flowCandidates: [], message: null, providerNotice: '',
  })
  await enterQuestion('FBA 标贴去哪里打印？')
  await act(async () => document.querySelector<HTMLButtonElement>('form button[type="submit"]')!.click())
  expect(document.body.textContent).toContain('先打印 FBA 标签，再核对货件。')

  await publishDataChange('knowledge')
  expect(document.body.textContent).toContain('先打印 FBA 标签，再核对货件。')
  await publishDataChange('memos')
  expect(document.body.textContent).toContain('先打印 FBA 标签，再核对货件。')
})
