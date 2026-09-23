/**
 * 自然语言日期解析的单元测试（§4.4）。
 *
 * 这些用例同时充当"解析行为契约"：解析只提议、不改库；
 * 识别不了必须返回 null 而不是猜一个日期。
 */

import { describe, it, expect } from 'vitest'
import { parseQuickInput, cnNumberToInt } from './nlp'

/** 固定"现在"，让相对日期断言可复现（2026-09-23 是周三） */
const NOW = new Date(2026, 8, 23, 14, 30, 0)

describe('parseQuickInput', () => {
  it('不识别任何日期时返回空结果并保留原文标题', () => {
    const r = parseQuickInput('写周报', NOW)
    expect(r.title).toBe('写周报')
    expect(r.date).toBeNull()
    expect(r.hasTime).toBe(false)
    expect(r.tags).toEqual([])
    expect(r.priority).toBeNull()
  })

  it('解析「明天 10:00」', () => {
    const r = parseQuickInput('明天 10:00 交周报', NOW)
    expect(r.date).not.toBeNull()
    expect(r.date!.getFullYear()).toBe(2026)
    expect(r.date!.getMonth()).toBe(8)
    expect(r.date!.getDate()).toBe(24)
    expect(r.date!.getHours()).toBe(10)
    expect(r.hasTime).toBe(true)
    expect(r.title).toBe('交周报')
  })

  it('解析「今天」为当天且不含具体时刻', () => {
    const r = parseQuickInput('今天 买牛奶', NOW)
    expect(r.date!.getDate()).toBe(23)
    expect(r.hasTime).toBe(false)
    expect(r.date!.getHours()).toBe(0)
    expect(r.title).toBe('买牛奶')
  })

  it('解析「下午3点」为 15:00（12 小时制换算）', () => {
    const r = parseQuickInput('今天下午3点 打电话', NOW)
    expect(r.date!.getHours()).toBe(15)
    expect(r.hasTime).toBe(true)
  })

  it('解析「晚上8点」为 20:00', () => {
    const r = parseQuickInput('晚上8点 健身', NOW)
    expect(r.date).not.toBeNull()
    expect(r.date!.getHours()).toBe(20)
  })

  it('解析「3天后」', () => {
    const r = parseQuickInput('3天后 回访客户', NOW)
    expect(r.date!.getDate()).toBe(26)
    expect(r.title).toBe('回访客户')
  })

  it('解析「下周一」为下一个周一（2026-09-28）', () => {
    // 2026-09-23 是周三，本周一是 09-21，下周一为 09-28
    const r = parseQuickInput('下周一 提交材料', NOW)
    expect(r.date!.getDate()).toBe(28)
    expect(r.date!.getMonth()).toBe(8)
  })

  it('解析「周五」为本周五（09-25），因为今天周三还没到', () => {
    const r = parseQuickInput('周五 开会', NOW)
    expect(r.date!.getDate()).toBe(25)
  })

  it('已过去的本周星期几顺延到下周，不会返回过去日期', () => {
    // 今天是周三，周一已过 → 应指向下周一 09-28
    const r = parseQuickInput('周一 例会', NOW)
    expect(r.date!.getTime()).toBeGreaterThan(NOW.getTime())
    expect(r.date!.getDate()).toBe(28)
  })

  it('解析「9月30日」', () => {
    const r = parseQuickInput('9月30日 续费', NOW)
    expect(r.date!.getMonth()).toBe(8)
    expect(r.date!.getDate()).toBe(30)
    expect(r.title).toBe('续费')
  })

  it('未写年份的已过日期顺延到明年', () => {
    const r = parseQuickInput('1月5日 交年报', NOW)
    expect(r.date!.getFullYear()).toBe(2027)
    expect(r.date!.getMonth()).toBe(0)
    expect(r.date!.getDate()).toBe(5)
  })

  it('解析标签与优先级，并从标题中移除', () => {
    const r = parseQuickInput('明天 写周报 #工作 #重要 !2', NOW)
    expect(r.tags).toEqual(['工作', '重要'])
    expect(r.priority).toBe(2)
    expect(r.title).toBe('写周报')
  })

  it('支持中文全角叹号表示优先级', () => {
    const r = parseQuickInput('交材料 ！3', NOW)
    expect(r.priority).toBe(3)
  })

  it('解析 9/30 形式的日期', () => {
    const r = parseQuickInput('9/30 报销', NOW)
    expect(r.date!.getMonth()).toBe(8)
    expect(r.date!.getDate()).toBe(30)
  })

  it('日期词出现在标题中间也能解析，且标题保持可读', () => {
    const r = parseQuickInput('记得明天 买菜', NOW)
    expect(r.date!.getDate()).toBe(24)
    expect(r.title).toContain('买菜')
    expect(r.title).not.toContain('明天')
  })

  it('不把普通数字误判为时间', () => {
    const r = parseQuickInput('买 5 个苹果', NOW)
    expect(r.hasTime).toBe(false)
    expect(r.date).toBeNull()
    expect(r.title).toBe('买 5 个苹果')
  })

  it('非法月份不会被解析为日期', () => {
    const r = parseQuickInput('13月45日 无效', NOW)
    expect(r.date).toBeNull()
  })

  it('多名候选日期时只取第一个，避免互相覆盖', () => {
    const r = parseQuickInput('明天 后天 各一件事', NOW)
    expect(r.date!.getDate()).toBe(24) // 命中"明天"
  })
})

describe('cnNumberToInt', () => {
  it('解析阿拉伯数字', () => {
    expect(cnNumberToInt('9')).toBe(9)
    expect(cnNumberToInt('31')).toBe(31)
  })

  it('解析中文数字', () => {
    expect(cnNumberToInt('一')).toBe(1)
    expect(cnNumberToInt('两')).toBe(2)
    expect(cnNumberToInt('十')).toBe(10)
    expect(cnNumberToInt('十一')).toBe(11)
    expect(cnNumberToInt('二十')).toBe(20)
    expect(cnNumberToInt('三十一')).toBe(31)
  })

  it('无法解析时返回 null 而不是猜一个数', () => {
    expect(cnNumberToInt('')).toBeNull()
    expect(cnNumberToInt('abc')).toBeNull()
  })
})
