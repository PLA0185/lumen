/**
 * 大报告导出的边界测试（第三轮整改任务书 §10.5）。
 *
 * ## 这里能测什么、不能测什么（如实说明）
 *
 * `buildPrintDocument()` 要真的操作 DOM（创建 10 万个 `<tr>`），
 * 而本仓库没有 jsdom / testing-library，所以**"渲染 10 万行会不会卡死"
 * 这件事只能靠实机验收**（`tools/verify_remediation3.py`）。
 *
 * 但"什么时候该停止导出"是一条**纯计算**规则，也是防内存打满的最后一道闸
 * （任务书 §7.4：不能只看行数，还要看总字符数）。这条规则必须被单测盯住，
 * 否则上限形同虚设。所以这里测的是 `reportRowChars` 与 `exceedsCharLimit`。
 */

import { describe, expect, it } from 'vitest'
import { REPORT_MAX_CHARS, exceedsCharLimit, reportRowChars } from './report-export'
import type { TaskReportRow } from './ipc'

function row(over: Partial<TaskReportRow['task']> = {}, tagNames: string[] = []): TaskReportRow {
  return {
    task: {
      id: 't1',
      title: '标题',
      description: '',
      projectId: null,
      categoryId: null,
      status: 'todo',
      priority: 0,
      plannedAt: null,
      hasPlannedTime: 0,
      dueAt: null,
      hasDueTime: 0,
      completedAt: null,
      periodType: 'none',
      seriesId: null,
      ...over,
    },
    projectName: null,
    categoryName: null,
    tagNames,
  } as unknown as TaskReportRow
}

describe('报告导出的规模上限', () => {
  it('一行的字符数包含标题、描述、归属与标签', () => {
    const r = row({ title: '一二三', description: '四个字' }, ['标签甲', '标签乙'])
    const withOrg = { ...r, projectName: '项目', categoryName: '分类' } as TaskReportRow
    // 标题 3 + 描述 3 + 项目 2 + 分类 2 + 标签 3 + 3 = 16
    expect(reportRowChars(withOrg)).toBe(16)
  })

  it('可选字段缺失时按 0 计，不会算出 NaN', () => {
    const r = row({ description: undefined }, [])
    expect(reportRowChars(r)).toBe(2) // 只有"标题"两个字
  })

  it('达到上限才停止（边界是 >=，不是 >）', () => {
    expect(exceedsCharLimit(REPORT_MAX_CHARS - 1)).toBe(false)
    expect(exceedsCharLimit(REPORT_MAX_CHARS)).toBe(true)
    expect(exceedsCharLimit(REPORT_MAX_CHARS + 1)).toBe(true)
  })

  it('大量长描述会先于行数触顶', () => {
    // 10000 条 × 20k 描述 = 2 亿字符，远超上限——这正是"只看行数"会漏掉的情况
    const perRow = reportRowChars(row({ title: 'x', description: 'y'.repeat(20_000) }))
    const rowsBeforeLimit = Math.ceil(REPORT_MAX_CHARS / perRow)
    expect(perRow).toBeGreaterThan(20_000)
    // 上限 2000 万字符时，两千条这样的任务就会触顶，远早于 10 万行的行数上限
    expect(rowsBeforeLimit).toBeLessThanOrEqual(1001)
    expect(exceedsCharLimit(perRow * rowsBeforeLimit)).toBe(true)
  })

  it('上限值本身是有限且合理的（防止有人把它调成无限大）', () => {
    expect(Number.isFinite(REPORT_MAX_CHARS)).toBe(true)
    expect(REPORT_MAX_CHARS).toBeGreaterThan(1_000_000)
    expect(REPORT_MAX_CHARS).toBeLessThanOrEqual(100_000_000)
  })
})
