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
  it('从每季度重复入口新建时预选季度规则', () => {
    const html = renderToStaticMarkup(<QuickAdd onCreated={() => {}} defaultRepeat="quarterly" />)
    expect(html).toContain('value="quarterly" selected')
    expect(html).toContain('value="3"')
  })
  it('普通新建提供多行任务描述', () => {
    const html = renderToStaticMarkup(<QuickAdd onCreated={() => {}} />)
    expect(html).toMatch(/<textarea[^>]+aria-label="任务描述"/)
  })

  it('长重复任务表单底部也提供添加与取消按钮', () => {
    const html = renderToStaticMarkup(
      <QuickAdd onCreated={() => {}} onCancel={() => {}} defaultRepeat="weekly" />,
    )
    const bottomActions = html.slice(html.indexOf('quickadd__actions--bottom'))
    expect(bottomActions).toContain('>添加</button>')
    expect(bottomActions).toContain('>取消</button>')
  })
})
