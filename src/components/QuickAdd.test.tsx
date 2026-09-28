import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { QuickAdd } from './QuickAdd'

describe('统一新建任务入口', () => {
  it('在日期视图新建时预填计划日期，避免保存后被当前视图隐藏', () => {
    const html = renderToStaticMarkup(<QuickAdd onCreated={() => {}} {...{defaultPlannedDate: '2026-09-28'}} />)
    expect(html).toContain('aria-label="计划执行日期"')
    expect(html).toContain('value="2026-09-28"')
  })
  it('包含重复选项，默认不重复', () => {
    const html = renderToStaticMarkup(<QuickAdd onCreated={() => {}} />)
    expect(html).toContain('aria-label="任务重复"')
    expect(html).toContain('不重复')
  })
})
