import { describe, expect, it } from 'vitest'
import { parseChatGPTContentImport } from './chatgpt-content-import'
import type { SaveMemoInput } from './memos-ipc'

const empty: SaveMemoInput = { id: null, expectedRevision: null, title: '', category: '', kind: 'memo', bodyMd: '', steps: [] }

describe('ChatGPT 内容导入预览解析', () => {
  it('解析 JSON 新建流程，但只返回草稿', () => {
    const result = parseChatGPTContentImport(JSON.stringify({ kind: 'flow', title: '发货流程', category: '物流', bodyMd: '核对订单。', steps: [{ title: '核对订单', owner: '运营', detail: '确认型号和数量。' }] }), { ...empty, kind: 'flow' }, true)
    expect(result).toMatchObject({ id: null, expectedRevision: null, kind: 'flow', title: '发货流程', category: '物流', bodyMd: '核对订单。' })
    expect(result.steps).toHaveLength(1)
    expect(result.steps[0]).toMatchObject({ title: '核对订单', owner: '运营', detail: '确认型号和数量。' })
  })

  it('新建时允许 GPT 根据 JSON kind 自动选择备忘或流程', () => {
    const memoResult = parseChatGPTContentImport(JSON.stringify({ kind: 'memo', title: '物流术语', bodyMd: '追踪编号说明。', steps: [] }), { ...empty, kind: 'flow' }, true)
    const flowResult = parseChatGPTContentImport(JSON.stringify({ kind: 'flow', title: '出货流程', steps: [{ title: '核对订单', detail: '核对数量。' }] }), empty, true)

    expect(memoResult).toMatchObject({ kind: 'memo', title: '物流术语', bodyMd: '追踪编号说明。', steps: [] })
    expect(flowResult).toMatchObject({ kind: 'flow', title: '出货流程' })
    expect(flowResult.steps).toHaveLength(1)
  })

  it('纯 Markdown 更新只替换正文，并沿用目标记录身份与标题', () => {
    const existing: SaveMemoInput = { id: 'memo-1', expectedRevision: 7, kind: 'memo', title: '术语表', category: '业务', bodyMd: '旧内容', steps: [] }
    const result = parseChatGPTContentImport('新整理后的内容\n\n- 第一项', existing, false)
    expect(result).toMatchObject({ id: 'memo-1', expectedRevision: 7, title: '术语表', category: '业务', bodyMd: '新整理后的内容\n\n- 第一项' })
  })

  it('Markdown 标题用于新建标题，坏 JSON 明确报错', () => {
    expect(parseChatGPTContentImport('# SOP 标题\n\n正文', empty, true)).toMatchObject({ title: 'SOP 标题', bodyMd: '正文' })
    expect(() => parseChatGPTContentImport('{bad json}', empty, true)).toThrow('JSON 格式不正确')
  })

  it('流程 Markdown 把步骤标题下的图片保留在对应步骤', () => {
    const image = '![订单截图](lumen-asset:11111111-1111-4111-8111-111111111111)'
    const result = parseChatGPTContentImport(`# 发货 SOP\n\n适用范围\n\n## 第 1 步：核对订单\n\n核对型号和数量。\n\n${image}\n\n## 第 2 步：通知仓库\n\n发送备货要求。`, { ...empty, kind: 'flow' }, true)
    expect(result).toMatchObject({ title: '发货 SOP', bodyMd: '适用范围' })
    expect(result.steps.map(step => step.title)).toEqual(['核对订单', '通知仓库'])
    expect(result.steps[0]!.detail).toContain(image)
    expect(result.steps[1]!.detail).not.toContain('lumen-asset:')
  })

  it('更新已有流程时按步骤标题保留原步骤图片和说明区图片', () => {
    const orderImage = '![订单](lumen-asset:11111111-1111-4111-8111-111111111111)'
    const packingImage = '![包装](lumen-asset:22222222-2222-4222-8222-222222222222)'
    const introImage = '![示意图](lumen-asset:33333333-3333-4333-8333-333333333333)'
    const existing: SaveMemoInput = {
      id: 'flow-1', expectedRevision: 4, kind: 'flow', title: '出货流程', category: '物流',
      bodyMd: `旧背景\n\n${introImage}`,
      steps: [
        { id: 'step-order', title: '核对订单', owner: '运营', detail: `核对订单信息。\n\n${orderImage}` },
        { id: 'step-packing', title: '包装商品', owner: '仓库', detail: `检查包装。\n\n${packingImage}` },
      ],
    }
    const result = parseChatGPTContentImport('# 出货 SOP\n\n新背景\n\n## 包装商品\n\n更新包装要求。\n\n## 核对订单\n\n更新核对规则。', existing, false)
    expect(result.bodyMd).toContain(introImage)
    expect(result.steps.map(step => step.id)).toEqual(['step-packing', 'step-order'])
    expect(result.steps[0]!.detail).toContain(packingImage)
    expect(result.steps[1]!.detail).toContain(orderImage)
  })

  it('已有流程步骤无法匹配时不丢图片，而是留在待分配内容中', () => {
    const image = '![检查示意](lumen-asset:44444444-4444-4444-8444-444444444444)'
    const existing: SaveMemoInput = {
      id: 'flow-2', expectedRevision: 2, kind: 'flow', title: '检查流程', category: '', bodyMd: '',
      steps: [{ id: 'step-inspect', title: '检查样品', owner: '', detail: `观察外观。\n\n${image}` }],
    }
    const result = parseChatGPTContentImport('# 检查流程\n\n## 复核样品\n\n再次观察外观。', existing, false)
    expect(result.steps[0]!.detail).not.toContain(image)
    expect(result.bodyMd).toContain(image)
  })

  it('JSON 更新已有步骤时保留该步骤原图片', () => {
    const image = '![装箱图](lumen-asset:55555555-5555-4555-8555-555555555555)'
    const existing: SaveMemoInput = {
      id: 'flow-3', expectedRevision: 6, kind: 'flow', title: '装箱流程', category: '', bodyMd: '',
      steps: [{ id: 'step-pack', title: '装箱', owner: '', detail: image }],
    }
    const result = parseChatGPTContentImport(JSON.stringify({ steps: [{ id: 'step-pack', title: '装箱', detail: '更新装箱规范。' }] }), existing, false)
    expect(result.steps[0]!.detail).toContain(image)
  })
})
