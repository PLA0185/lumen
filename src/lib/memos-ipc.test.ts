import { expect, it } from 'vitest'
import { memoMarkdown, type SaveMemoInput } from './memos-ipc'

it('复制流程按当前顺序编号一次且不修改标题、正文及图片引用', () => {
  const doc: SaveMemoInput = {
    id: null, expectedRevision: null, title: '出货', category: '', kind: 'flow', bodyMd: '原说明',
    steps: [
      { id: 'one', title: '6. 发票', owner: '', detail: '6. 操作原文\n![原图](lumen-asset:example)' },
      { id: 'two', title: '6.2 版模板', owner: '', detail: '原文' },
      { id: 'three', title: '100kg 货物', owner: '', detail: '' },
      { id: 'four', title: '4．核对数量', owner: '', detail: '' },
      { id: 'five', title: '5、通知仓库', owner: '', detail: '' },
    ],
  }
  const before = structuredClone(doc)
  const md = memoMarkdown(doc)
  expect(md).toContain('### 1. 发票\n')
  expect(md).toContain('### 2. 6.2 版模板\n')
  expect(md).toContain('### 3. 100kg 货物\n')
  expect(md).toContain('### 4. 核对数量\n')
  expect(md).toContain('### 5. 通知仓库\n')
  expect(md).toContain(doc.steps[0]!.detail)
  expect(doc).toEqual(before)
})


it('复制分组流程保留章节及阶段，原文图片仍在各自步骤中', () => {
  const group = { id: 'chapter-2', title: '2. 发货', path: ['2.2 填写'] }
  const doc: SaveMemoInput = { id: null, expectedRevision: null, title: '原流程', category: '', kind: 'flow', bodyMd: '', steps: [
    { id: 'a', title: '核对', owner: '', detail: '甲说明\n![甲](lumen-asset:甲)', group },
    { id: 'b', title: '保存', owner: '', detail: '乙说明\n![乙](lumen-asset:乙)', group },
  ] }
  const before = structuredClone(doc)
  const md = memoMarkdown(doc)
  expect(md).toContain('### 2. 发货\n')
  expect(md).toContain('#### 2.2 填写\n')
  expect(md.split('### 2. 发货\n')).toHaveLength(2)
  expect(md).toContain('##### 1. 核对\n')
  expect(md.indexOf(doc.steps[0]!.detail)).toBeLessThan(md.indexOf(doc.steps[1]!.detail))
  expect(doc).toEqual(before)
})
