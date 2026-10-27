import { afterEach, describe, expect, it, vi } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { TaskCard } from './TaskCard'
import type { Task } from '../lib/types'
import { toUtcIso } from '../lib/datetime'

afterEach(() => vi.useRealTimers())

const task = {
  id: 'parent',
  title: '更新关键词',
  description: '',
  status: 'todo',
  priority: 0,
  plannedAt: null,
  dueAt: null,
  seriesId: null,
  periodType: 'none',
  isPinned: 0,
  estimatedMinutes: null,
} as Task
const children = [
  {
    id: 'child',
    taskId: 'parent',
    title: 'CA 关键词',
    isDone: 0,
    sortOrder: 0,
    completedAt: null,
    createdAt: '',
    updatedAt: '',
  },
]

describe('任务卡片中的文本与子任务', () => {
  it('拖拽只从把手开始，输入框的祖先不能成为拖拽源', () => {
    const html = renderToStaticMarkup(
      <TaskCard task={task} sortable onToggle={() => {}} onDelete={() => {}} />,
    )
    expect(html.slice(0, html.indexOf('>'))).not.toContain('draggable="true"')
    expect(html).toMatch(/class="task__grip"[^>]*draggable="true"/)
  })

  it('折叠卡片也能看到已有子任务的名称与完成状态', () => {
    const html = renderToStaticMarkup(
      <TaskCard
        task={task}
        onToggle={() => {}}
        onDelete={() => {}}
        progress={{ total: 1, done: 0, percent: 0, completionMode: 'free', items: children }}
      />,
    )
    expect(html).toContain('CA 关键词')
    expect(html).toContain('完成子任务「CA 关键词」')
  })

  it('折叠卡片的描述会渲染 Markdown 标题和加粗', () => {
    const html = renderToStaticMarkup(
      <TaskCard
        task={{ ...task, description: '## 月复盘\n\n优先处理**今天**的任务。' }}
        onToggle={() => {}}
        onDelete={() => {}}
      />,
    )
    expect(html).toMatch(/<h2>.*月复盘.*<\/h2>/)
    expect(html).toContain('<strong>今天</strong>')
    expect(html).not.toContain('## 月复盘')
  })

  it('计划在下周的周跨度任务显示「下周内」而不是「本周内」', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 9, 16, 12, 0))
    const html = renderToStaticMarkup(
      <TaskCard
        task={{
          ...task,
          plannedAt: toUtcIso(new Date(2026, 9, 19, 9, 0)),
          periodType: 'week',
        }}
        onToggle={() => {}}
        onDelete={() => {}}
      />,
    )
    expect(html).toContain('下周内')
    expect(html).not.toContain('本周内')
  })

  it('历史日期的后来完成任务显示为未完成，并阻止误改真实完成状态', () => {
    const html = renderToStaticMarkup(
      <TaskCard task={{ ...task, status: 'done', periodType: 'week' }} displayDone={false}
        completionReadOnly onToggle={() => {}} onDelete={() => {}} />,
    )
    expect(html).toContain('aria-checked="false"')
    expect(html).toContain('历史日期不能修改完成状态')
    expect(html).toContain('disabled=""')
    expect(html).toContain('之后已完成')
  })
})
