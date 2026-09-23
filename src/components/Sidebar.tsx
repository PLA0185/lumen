/**
 * 侧边栏导航（§3：主界面至少包含收件箱、今天、明天、本周、全部任务、
 * 日历、项目/分类、标签、已完成、统计、回收站、设置）。
 *
 * 未实现的视图在此**不出现**，而不是放一个点不动的按钮——
 * 任务书 §3 明确禁止"假按钮或占位功能"。
 *
 * 图标全部来自 `Icons.tsx` 的统一 SVG 集：初版用的是 emoji + 文字符号
 * （☀ 📅 ⑦ ① 混排），粗细与配色都对不齐，看起来像随手拼的。
 */

import { Icon, type IconName } from './Icons'
import type { ViewId } from '../lib/types'

interface NavEntry {
  id: ViewId
  icon: IconName
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
      { id: 'today', icon: 'today', label: '今天' },
      { id: 'tomorrow', icon: 'tomorrow', label: '明天' },
      { id: 'week', icon: 'week', label: '本周安排' },
      { id: 'calendar', icon: 'calendar', label: '日历' },
      { id: 'board', icon: 'board', label: '看板' },
      { id: 'inbox', icon: 'inbox', label: '收件箱' },
      { id: 'all', icon: 'list', label: '全部任务' },
    ],
  },
  {
    // 周期任务与上面的「今天/本周安排」是**不同维度**：
    // 上面按计划时间筛选（任务有具体日期），这里按任务自身的周期跨度筛选
    // （任务没有具体日期，只声明"这周/这个月做完就行"）。命名必须区分开。
    label: '周期任务',
    items: [
      { id: 'period-week', icon: 'period-week', label: '周任务' },
      { id: 'period-month', icon: 'period-month', label: '月任务' },
      { id: 'period-quarter', icon: 'period-quarter', label: '季度任务' },
      { id: 'period-year', icon: 'period-year', label: '年任务' },
    ],
  },
  {
    label: '整理',
    items: [
      { id: 'projects', icon: 'projects', label: '项目与分类' },
      { id: 'tags', icon: 'tags', label: '标签' },
      { id: 'completed', icon: 'completed', label: '已完成' },
      { id: 'trash', icon: 'trash', label: '回收站' },
    ],
  },
  {
    label: '其他',
    items: [
      { id: 'stats', icon: 'stats', label: '统计' },
      { id: 'settings', icon: 'settings', label: '设置' },
    ],
  },
]

/** 视图标题与说明。说明文字用于向用户解释该视图的口径（§4.2 要求规则明确）。 */
export const VIEW_META: Record<ViewId, { title: string; subtitle: string }> = {
  today: { title: '今天', subtitle: '计划时间落在今天的所有任务' },
  tomorrow: { title: '明天', subtitle: '计划时间落在明天的所有任务' },
  week: {
    title: '本周安排',
    subtitle: '计划时间落在本周一到周日的任务（按日期筛选，含已过去的日子）',
  },
  'period-week': {
    title: '周任务',
    subtitle: '标记为「本周内完成」的任务——不绑定到具体某一天，这周做完即可',
  },
  'period-month': {
    title: '月任务',
    subtitle: '标记为「本月内完成」的任务——不绑定到具体某一天，这个月做完即可',
  },
  'period-quarter': {
    title: '季度任务',
    subtitle: '标记为「本季度内完成」的任务，适合阶段性目标',
  },
  'period-year': {
    title: '年任务',
    subtitle: '标记为「今年内完成」的任务，适合年度目标',
  },
  inbox: { title: '收件箱', subtitle: '尚未归属任何项目的未完成任务' },
  all: { title: '全部任务', subtitle: '所有未归档任务' },
  calendar: { title: '日历', subtitle: '按日、周、月查看任务安排，可拖拽改期' },
  board: { title: '看板', subtitle: '按状态分列，拖拽卡片即可改变状态' },
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
        {/* 品牌标记用"光"的意象自绘，而不是一个字母——
            初版这里还留着改名前的 A */}
        <span className="sidebar__logo" aria-hidden="true">
          <Icon name="today" size={17} strokeWidth={1.9} />
        </span>
        <span className="sidebar__title">Lumen</span>
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
                    <Icon name={it.icon} size={18} />
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
