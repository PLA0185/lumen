/**
 * 侧边栏导航（§3：主界面至少包含收件箱、今天、明天、本周、全部任务、
 * 日历、项目/分类、标签、已完成、统计、回收站、设置）。
 *
 * 未实现的视图在此**不出现**，而不是放一个点不动的按钮——
 * 任务书 §3 明确禁止"假按钮或占位功能"。
 */

import type { ViewId } from '../lib/types'

interface NavEntry {
  id: ViewId
  icon: string
  label: string
}

interface NavGroup {
  label: string
  items: NavEntry[]
}

/** 导航分组。顺序按使用频率排列，今天的任务在第一位。 */
export const NAV_GROUPS: NavGroup[] = [
  {
    label: '任务',
    items: [
      { id: 'today', icon: '☀', label: '今天' },
      { id: 'tomorrow', icon: '⛅', label: '明天' },
      { id: 'week', icon: '🗓', label: '本周' },
      { id: 'calendar', icon: '📅', label: '日历' },
      { id: 'inbox', icon: '📥', label: '收件箱' },
      { id: 'all', icon: '📋', label: '全部任务' },
    ],
  },
  {
    label: '整理',
    items: [
      { id: 'projects', icon: '📁', label: '项目与分类' },
      { id: 'tags', icon: '🏷', label: '标签' },
      { id: 'completed', icon: '✓', label: '已完成' },
      { id: 'trash', icon: '🗑', label: '回收站' },
    ],
  },
  {
    label: '其他',
    items: [
      { id: 'stats', icon: '📊', label: '统计' },
      { id: 'settings', icon: '⚙', label: '设置' },
    ],
  },
]

/** 视图标题与说明。说明文字用于向用户解释该视图的口径（§4.2 要求规则明确）。 */
export const VIEW_META: Record<ViewId, { title: string; subtitle: string }> = {
  today: { title: '今天', subtitle: '计划时间落在今天的所有任务' },
  tomorrow: { title: '明天', subtitle: '计划时间落在明天的所有任务' },
  week: { title: '本周', subtitle: '本周一到周日计划的任务（含已过去的日子）' },
  inbox: { title: '收件箱', subtitle: '尚未归属任何项目的未完成任务' },
  all: { title: '全部任务', subtitle: '所有未归档任务' },
  calendar: { title: '日历', subtitle: '按日、周、月查看任务安排' },
  projects: { title: '项目与分类', subtitle: '项目是任务集合，分类用于统计归类' },
  tags: { title: '标签', subtitle: '给任务加上跨项目的横向标记' },
  completed: { title: '已完成', subtitle: '完成时间真实记录，可撤销完成' },
  stats: { title: '统计', subtitle: '完成数量、完成率、逾期与耗时趋势' },
  trash: { title: '回收站', subtitle: '删除的任务保留在此，可恢复或永久删除' },
  settings: { title: '设置', subtitle: '外观、窗口、提醒、AI 与数据管理' },
}

interface SidebarProps {
  current: ViewId
  onSelect: (v: ViewId) => void
  counts: Partial<Record<ViewId, number>>
  version?: string
}

export function Sidebar({ current, onSelect, counts, version }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <span className="sidebar__logo" aria-hidden="true">
          A
        </span>
        <span className="sidebar__title">AiTodo</span>
        {version && <span className="sidebar__version">v{version}</span>}
      </div>

      <nav className="sidebar__nav" aria-label="主导航">
        {NAV_GROUPS.map((g) => (
          <div className="nav-group" key={g.label}>
            <div className="nav-group__label">{g.label}</div>
            {g.items.map((it) => {
              const count = counts[it.id]
              const isCurrent = current === it.id
              return (
                <button
                  key={it.id}
                  type="button"
                  className="nav-item"
                  // aria-current 让辅助技术能识别当前页（§3）
                  aria-current={isCurrent ? 'page' : undefined}
                  onClick={() => onSelect(it.id)}
                >
                  <span className="nav-item__icon" aria-hidden="true">
                    {it.icon}
                  </span>
                  <span className="nav-item__label">{it.label}</span>
                  {typeof count === 'number' && count > 0 && (
                    <span
                      className={`nav-item__count${it.id === 'today' ? ' nav-item__count--alert' : ''}`}
                      aria-label={`${count} 项`}
                    >
                      {count > 99 ? '99+' : count}
                    </span>
                  )}
                </button>
              )
            })}
          </div>
        ))}
      </nav>
    </aside>
  )
}
