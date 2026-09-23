/**
 * PDF 导出用的打印报告（任务书 §6「导出为可读格式」）。
 *
 * ## 为什么是一张"打印视图"而不是在 Rust 里画 PDF
 *
 * 中文可读是硬要求。纯 Rust 的 PDF 库内置字体是 WinAnsi 单字节编码，
 * 中文会被静默丢弃；自嵌 CJK 字体又有已知的字形回归。而 Lumen 本身
 * 就跑在 WebView2（Chromium）里——直接用它自己的 `PrintToPdf` 打印
 * **当前页面**，中文由系统字体渲染，且所见即所得。
 *
 * 于是导出流程是：
 * 1. 把本组件挂到 `#print-report` 并给 `<body data-print="on">`；
 * 2. CSS 让报告成为唯一可见内容（屏幕上也是），等一帧布局稳定；
 * 3. 调后端 `export_pdf` 让 WebView2 打印当前页面；
 * 4. 完成后摘掉 `data-print`，界面恢复原样。
 *
 * 报告刻意做成"可读文档"而不是数据转储：有标题、生成时间、统计概览，
 * 并且按状态分组、逾期项标红。
 */

import { useMemo } from 'react'
import { fromUtcIso, isOverdue } from '../lib/datetime'
import type { TaskReportRow } from '../lib/ipc'

export interface PrintReportProps {
  /** 报告数据：任务本体 + 已在后端解析好的项目/分类/标签名称 */
  rows: TaskReportRow[]
  /** 报告标题（例如「今天」「全部任务」），来自当前视图 */
  scopeTitle: string
  /** 附加的筛选说明，例如"搜索：报告" */
  filterNote?: string
}

const STATUS_TEXT: Record<string, string> = {
  todo: '待办',
  doing: '进行中',
  waiting: '等待',
  done: '已完成',
  archived: '已归档',
}

const PRIORITY_TEXT: Record<number, string> = {
  0: '—',
  1: '低',
  2: '中',
  3: '高',
}

/** 把 UTC ISO 转成"本地时区 + 是否带具体时刻"的可读文本 */
function timeText(v: string | null, hasTime: number): string {
  if (!v) return '—'
  const d = fromUtcIso(v)
  if (!d) return '—'
  const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
  if (hasTime !== 1) return date
  return `${date} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

const PERIOD_TEXT: Record<string, string> = {
  week: '本周',
  month: '本月',
  quarter: '本季度',
  year: '本年',
}

export function PrintReport({ rows, scopeTitle, filterNote }: PrintReportProps) {
  // 分组：逾期 → 未完成 → 已完成。这样打印出来第一页就是最要紧的事。
  const { groups, stats } = useMemo(() => {
    const overdue: TaskReportRow[] = []
    const open: TaskReportRow[] = []
    const done: TaskReportRow[] = []
    for (const r of rows) {
      if (r.task.status === 'done') done.push(r)
      else if (isOverdue(r.task)) overdue.push(r)
      else open.push(r)
    }
    return {
      groups: [
        { key: 'overdue', label: '已逾期', rows: overdue },
        { key: 'open', label: '进行中 / 待办', rows: open },
        { key: 'done', label: '已完成', rows: done },
      ].filter((g) => g.rows.length > 0),
      stats: {
        total: rows.length,
        done: done.length,
        open: open.length + overdue.length,
        overdue: overdue.length,
      },
    }
  }, [rows])

  const now = new Date()
  const generated = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(
    now.getDate(),
  ).padStart(2, '0')} ${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`

  return (
    <div id="print-report" className="report">
      <header className="report__head">
        <h1 className="report__title">Lumen 任务清单</h1>
        <div className="report__sub">
          范围：{scopeTitle}
          {filterNote ? `　·　${filterNote}` : ''}
        </div>
        <div className="report__sub">生成时间：{generated}</div>
      </header>

      <section className="report__stats">
        <div className="report__stat">
          <span className="report__stat-num">{stats.total}</span>
          <span className="report__stat-label">任务总数</span>
        </div>
        <div className="report__stat">
          <span className="report__stat-num">{stats.open}</span>
          <span className="report__stat-label">未完成</span>
        </div>
        <div className="report__stat">
          <span className="report__stat-num report__stat-num--danger">{stats.overdue}</span>
          <span className="report__stat-label">已逾期</span>
        </div>
        <div className="report__stat">
          <span className="report__stat-num">{stats.done}</span>
          <span className="report__stat-label">已完成</span>
        </div>
      </section>

      {groups.length === 0 ? (
        <p className="report__empty">当前范围内没有任务。</p>
      ) : (
        groups.map((g, gi) => (
          <section key={g.key} className="report__group">
            <h2 className="report__group-title">
              {g.label}
              <span className="report__group-count">{g.rows.length}</span>
            </h2>
            <table className="report__table">
              <thead>
                <tr>
                  <th className="report__col-idx">#</th>
                  <th className="report__col-state">状态</th>
                  <th>任务</th>
                  <th className="report__col-time">计划</th>
                  <th className="report__col-time">截止</th>
                  <th className="report__col-prio">优先级</th>
                  <th className="report__col-org">项目 / 分类</th>
                  <th className="report__col-tags">标签</th>
                </tr>
              </thead>
              <tbody>
                {g.rows.map((row, i) => {
                  const t = row.task
                  const overdue = isOverdue(t)
                  const period =
                    t.periodType && t.periodType !== 'none' ? PERIOD_TEXT[t.periodType] : null
                  return (
                    <tr key={t.id} className={overdue ? 'report__row--overdue' : ''}>
                      <td className="report__col-idx">{gi * 1000 + i + 1}</td>
                      <td className="report__col-state">
                        {t.status === 'done' ? '✓ ' : ''}
                        {STATUS_TEXT[t.status] ?? t.status}
                      </td>
                      <td>
                        <div className="report__task-title">{t.title}</div>
                        {t.description && <div className="report__task-desc">{t.description}</div>}
                        {period && <div className="report__task-note">周期：{period}内完成即可</div>}
                        {t.seriesId && <div className="report__task-note">重复任务的一次发生</div>}
                      </td>
                      <td className="report__col-time">
                        {timeText(t.plannedAt, t.hasPlannedTime)}
                      </td>
                      <td className="report__col-time">{timeText(t.dueAt, t.hasDueTime)}</td>
                      <td className="report__col-prio">{PRIORITY_TEXT[t.priority] ?? '—'}</td>
                      <td className="report__col-org">
                        {[row.projectName, row.categoryName].filter(Boolean).join(' / ') || '—'}
                      </td>
                      <td className="report__col-tags">{row.tagNames.join('、') || '—'}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </section>
        ))
      )}

      <footer className="report__foot">
        由 Lumen 生成　·　导出时间为 {generated}　·　共 {stats.total} 条任务
      </footer>
    </div>
  )
}
