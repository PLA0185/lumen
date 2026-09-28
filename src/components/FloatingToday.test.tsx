import { describe, it, expect } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { FloatingToday } from './FloatingToday'
import { QuickAdd } from './QuickAdd'

describe('浮窗直接新建与一周导航', () => {
  it('直接提供日期导航、新建、专注和提醒入口', () => {
    const html = renderToStaticMarkup(<FloatingToday />)
    expect(html).toContain('aria-label="一周日期"')
    expect(html).toContain('aria-label="新建任务"')
    expect(html).toContain('开始专注')
    expect(html).toContain('添加提醒')
  })
  it('新建行提供就地编辑选项，默认沿用选中日期', () => {
    const html = renderToStaticMarkup(
      <QuickAdd compact defaultPlannedDate="2026-09-29" onCreated={() => {}} />,
    )
    expect(html).toContain('aria-label="新任务选项"')
    expect(html).toContain('aria-expanded="false"')
  })
})
