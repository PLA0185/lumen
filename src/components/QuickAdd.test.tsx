import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { QuickAdd } from './QuickAdd'

describe('统一新建任务入口', () => {
  it('包含重复选项，默认不重复', () => {
    const html = renderToStaticMarkup(<QuickAdd onCreated={() => {}} />)
    expect(html).toContain('aria-label="任务重复"')
    expect(html).toContain('不重复')
  })
})
