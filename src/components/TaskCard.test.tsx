import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import { TaskCard } from './TaskCard'
import type { Task } from '../lib/types'

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
        progress={{ total: 1, done: 0, percent: 0, items: children }}
      />,
    )
    expect(html).toContain('CA 关键词')
    expect(html).toContain('完成子任务「CA 关键词」')
  })
})
