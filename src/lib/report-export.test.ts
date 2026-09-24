/**
 * 报告导出的边界测试（第三轮整改任务书 §10.5 + 第四轮收口任务书 §21/§31 的 PDF 部分）。
 *
 * ## 这里能测什么、不能测什么（如实说明）
 *
 * `buildPrintDocument()` 要真的操作 DOM（最多创建 10 万个 `<tr>`），本仓库没有
 * jsdom / testing-library，所以**它自身的 DOM 行为**——"渲染 10 万行会不会卡死、
 * 点了取消之后多久停手"——只能靠实机验收（`tools/verify_remediation3.py`）。
 *
 * 但"下一行还能不能写"是一条**纯计算**规则，也是防内存打满的最后一道闸
 * （任务书 §7.4：不能只看行数，还要看总字符数）。这条规则被抽成了
 * `shouldStop()` / `rowGate()` / `consumeRows()`，**构建器与这里调用的是同一份实现**：
 * `consumeRows` 的 `onWrite` 在构建器里 append `<tr>`，在测试里只记账。
 * 因此下面的断言盯住的是真实判定代码，而不是测试里另写的一份循环。
 *
 * ## 边界语义（第四轮收口任务书 §22：代码、注释、测试三处同一套说法）
 *
 * - **行数**：上限的含义是"最多写多少行"——第 `REPORT_MAX_ROWS` 行**写入**，
 *   第 `REPORT_MAX_ROWS + 1` 行不写并标记截断；
 * - **字符**：写入一行**之前**先算"加上这一行会不会超过 `REPORT_MAX_CHARS`"——
 *   累计**恰好等于**上限的内容可以完整写入，超过才停下并**把这一行排除**；
 * - **取消**：优先于两道上限，并且**逐行**检查（§20），已写入的行保留。
 */

import { describe, expect, it } from 'vitest'
import {
  REPORT_MAX_CHARS,
  REPORT_MAX_ROWS,
  consumeRows,
  reportRowChars,
  rowGate,
  shouldStop,
} from './report-export'
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

/** 造一批"每行成本都相同"的行，用来喂 `consumeRows`（不涉及 DOM） */
function costs(count: number, each: number): number[] {
  return Array.from({ length: count }, () => each)
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

  it('上限值本身是有限且合理的（防止有人把它调成无限大）', () => {
    expect(Number.isFinite(REPORT_MAX_CHARS)).toBe(true)
    expect(REPORT_MAX_CHARS).toBeGreaterThan(1_000_000)
    expect(REPORT_MAX_CHARS).toBeLessThanOrEqual(100_000_000)
    expect(Number.isFinite(REPORT_MAX_ROWS)).toBe(true)
    expect(REPORT_MAX_ROWS).toBeGreaterThan(10_000)
    expect(REPORT_MAX_ROWS).toBeLessThanOrEqual(1_000_000)
  })

  it('大量长描述会先于行数触顶（走真实写入循环，不是只算公式）', () => {
    // 10000 条 × 20k 描述 = 2 亿字符，远超上限——这正是"只看行数"会漏掉的情况
    const perRow = reportRowChars(row({ title: 'x', description: 'y'.repeat(20_000) }))
    expect(perRow).toBeGreaterThan(20_000)

    // 恰好写得下的行数：累计不超过字符上限
    const fitRows = Math.floor(REPORT_MAX_CHARS / perRow)
    // 上限 2000 万字符时，一千行左右就触顶，远早于 10 万行的行数上限
    expect(fitRows).toBeGreaterThan(1)
    expect(fitRows).toBeLessThanOrEqual(1001)
    expect(fitRows).toBeLessThan(REPORT_MAX_ROWS)

    const consumed = consumeRows({
      items: costs(fitRows + 1, perRow),
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      onWrite: () => {},
    })
    expect(consumed.written).toBe(fitRows)
    expect(consumed.stop?.reason).toBe('chars')
  })

  it('没取消也没触顶时整批写完，没有停止原因', () => {
    const written: number[] = []
    const consumed = consumeRows({
      items: [1, 2, 3],
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      onWrite: (v) => written.push(v),
    })
    expect(written).toEqual([1, 2, 3])
    expect(consumed.written).toBe(3)
    expect(consumed.stop).toBeUndefined()
  })
})

describe('字符上限 REPORT_MAX_CHARS 的边界（§21/§22）', () => {
  it('已写入 REPORT_MAX_CHARS - 1 时再写 1 个字符：恰好到上限，仍然写入', () => {
    // 原断言是 exceedsCharLimit(REPORT_MAX_CHARS - 1) === false / (REPORT_MAX_CHARS) === true
    // （即 >= 语义：一到上限就停）。第四轮收口 §22 统一为"写之前预判加上这一行会不会超过"，
    // 所以恰好等于上限必须**写入**，而不是被挡在门外。
    expect(shouldStop({ rows: 0, chars: REPORT_MAX_CHARS - 1, rowCost: 1 })).toEqual({ stop: false })
  })

  it('已写入恰好 REPORT_MAX_CHARS：0 成本的行仍可写，多 1 个字符就停', () => {
    expect(shouldStop({ rows: 0, chars: REPORT_MAX_CHARS, rowCost: 0 })).toEqual({ stop: false })
    expect(shouldStop({ rows: 0, chars: REPORT_MAX_CHARS, rowCost: 1 })).toEqual({
      stop: true,
      reason: 'chars',
    })
  })

  it('已写入 REPORT_MAX_CHARS + 1 时：下一行一定不写，原因是字符数', () => {
    expect(shouldStop({ rows: 0, chars: REPORT_MAX_CHARS + 1, rowCost: 0 })).toEqual({
      stop: true,
      reason: 'chars',
    })
  })

  it('真实写入循环：累计恰好等于上限的内容完整写入，超出的那一行被排除并标记截断', () => {
    const written: number[] = []
    const consumed = consumeRows({
      // 第一行写完后累计恰好等于上限，第二行是 "0 成本"，第三行才真正超过
      items: [REPORT_MAX_CHARS, 0, 1],
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      onWrite: (v) => written.push(v),
    })
    expect(written).toEqual([REPORT_MAX_CHARS, 0])
    expect(consumed.written).toBe(2)
    expect(consumed.stop?.reason).toBe('chars')
    // 截断说明要如实告诉用户"剩余任务未包含"，与报告页脚、界面提示是同一份文案
    expect(consumed.stop?.note).toContain('超过单次导出上限')
    expect(consumed.stop?.note).toContain('剩余任务未包含')
    // 被排除的是"加上去会超过"的那一行本身：它没有被写
    expect(written).not.toContain(1)
  })
})

describe('行数上限 REPORT_MAX_ROWS 的边界（§21）', () => {
  it('已写入 REPORT_MAX_ROWS - 1 行时：还能写第 REPORT_MAX_ROWS 行', () => {
    expect(shouldStop({ rows: REPORT_MAX_ROWS - 1, chars: 0, rowCost: 0 })).toEqual({ stop: false })
  })

  it('已写入 REPORT_MAX_ROWS 行时：第 REPORT_MAX_ROWS + 1 行不写，原因是行数', () => {
    expect(shouldStop({ rows: REPORT_MAX_ROWS, chars: 0, rowCost: 0 })).toEqual({
      stop: true,
      reason: 'rows',
    })
  })

  it('已写入 REPORT_MAX_ROWS + 1 行时：同样按行数上限停下', () => {
    expect(shouldStop({ rows: REPORT_MAX_ROWS + 1, chars: 0, rowCost: 0 })).toEqual({
      stop: true,
      reason: 'rows',
    })
  })

  it('真实写入循环：REPORT_MAX_ROWS + 1 行里只写进 REPORT_MAX_ROWS 行，并带截断说明', () => {
    let writes = 0
    const consumed = consumeRows({
      items: costs(REPORT_MAX_ROWS + 1, 1),
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      onWrite: () => {
        writes += 1
      },
    })
    expect(writes).toBe(REPORT_MAX_ROWS)
    expect(consumed.written).toBe(REPORT_MAX_ROWS)
    expect(consumed.stop?.reason).toBe('rows')
    expect(consumed.stop?.note).toContain(`${REPORT_MAX_ROWS} 条`)
  })
})

describe('取消的语义（§20/§21）', () => {
  it('取数之前就取消：一行都不写，截断标记是「导出已取消」', () => {
    // 构建器在每次取页之前也走一次 rowGate（rowCost: 0）——下面就是那一次判定
    expect(rowGate({ cancelled: true, rows: 0, chars: 0, rowCost: 0 })).toEqual({
      stop: true,
      reason: 'cancelled',
      note: '导出已取消',
    })

    // 即使这一页有 500 行，取消之后一行也不会写
    const written: number[] = []
    const consumed = consumeRows({
      items: costs(500, 1),
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      isCancelled: () => true,
      onWrite: (v) => written.push(v),
    })
    expect(consumed.written).toBe(0)
    expect(written).toEqual([])
    expect(consumed.stop).toEqual({ stop: true, reason: 'cancelled', note: '导出已取消' })
  })

  it('写到第 3 行之前才取消：前 2 行保留，第 3 行起不写', () => {
    const written: number[] = []
    const consumed = consumeRows({
      items: costs(5, 10),
      costOf: (v) => v,
      rows: 0,
      chars: 0,
      // 第 1、2 行之前返回 false；第 3 行之前开始返回 true（模拟用户此刻点了取消）
      isCancelled: () => written.length >= 2,
      onWrite: (v) => written.push(v),
    })
    // 取消是逐行检查的：已经写好的 2 行留下，后面 3 行不再写（不会等整页跑完）
    expect(written).toEqual([10, 10])
    expect(consumed.written).toBe(2)
    expect(consumed.stop).toEqual({ stop: true, reason: 'cancelled', note: '导出已取消' })
  })

  it('前几页已经写过行之后才取消：已写入的行保留，且不会再多写一行', () => {
    const consumed = consumeRows({
      items: costs(3, 1),
      costOf: (v) => v,
      rows: 7, // 之前几页已经写入 7 行
      chars: 300,
      isCancelled: () => true,
      onWrite: () => {
        throw new Error('取消之后不应该再写入任何一行')
      },
    })
    expect(consumed.written).toBe(0)
    expect(consumed.stop?.reason).toBe('cancelled')
  })

  it('取消优先于两道上限：已经超限时提示仍然记为用户取消，不会被说成「超出上限」', () => {
    expect(
      rowGate({
        cancelled: true,
        rows: REPORT_MAX_ROWS,
        chars: REPORT_MAX_CHARS,
        rowCost: 0,
      }),
    ).toEqual({ stop: true, reason: 'cancelled', note: '导出已取消' })
  })
})
