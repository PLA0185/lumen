// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import * as ipc from '../lib/ipc'
import * as org from '../lib/organize-ipc'
import * as rec from '../lib/recurrence-ipc'
import { publishDataChange } from '../lib/data-change'
import type { Task } from '../lib/types'
import { TaskEditor } from './TaskEditor'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
const tag: org.TagWithCount = { id: 'tag-a', name: '工作', color: null, sortOrder: 0, createdAt: '', updatedAt: '', deletedAt: null, taskCount: 0 }
const task: Task = {
  id: 'task-a', title: '原任务', description: '', noteMd: '', linkUrl: null,
  status: 'todo', priority: 0, projectId: null, categoryId: null,
  plannedAt: null, hasPlannedTime: 0, dueAt: null, hasDueTime: 0,
  estimatedMinutes: null, actualMinutes: 0, completedAt: null,
  createdAt: '', updatedAt: '', deletedAt: null, sortOrder: 0,
  isPinned: 0, isFavorite: 0, periodType: 'none', seriesId: null,
  occurrenceKey: null, occurrenceIndex: null, occurrenceKind: null, isException: 0,
}
beforeEach(() => {
  vi.spyOn(org, 'projectList').mockResolvedValue([])
  vi.spyOn(org, 'categoryList').mockResolvedValue([])
  vi.spyOn(org, 'tagList').mockResolvedValue([tag])
  vi.spyOn(org, 'taskTagsGet').mockResolvedValue([])
  vi.spyOn(ipc, 'saveTask').mockResolvedValue(task)
})
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})
async function mount(value = task, onClose = vi.fn()) {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<TaskEditor task={value} onClose={onClose} onSaved={() => {}} />))
  return onClose
}
async function input(selector: string, value: string) {
  const field = document.querySelector<HTMLInputElement>(selector)!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value)
    field.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
async function click(selector: string) {
  await act(async () => document.querySelector<HTMLButtonElement>(selector)!.click())
}
async function chooseTag() {
  await input('[aria-label="搜索并添加标签"]', '工作')
  await click('.tagresult')
}

it('Esc 只取消重复修改范围，保留任务编辑器和未保存标题', async () => {
  vi.spyOn(rec, 'recurringScopeInfo').mockResolvedValue({ isRecurring: true, totalInstances: 3, completed: 0, completedBefore: 0 } as rec.ScopeInfo)
  const edit = vi.spyOn(rec, 'recurringEditInstance')
  const onClose = await mount({ ...task, seriesId: 'series-a', occurrenceKey: '2026-10-03' })
  await input('#ed-title', '未保存的新标题')
  await click('.modal__actions .btn--primary')
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(2)
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })))
  expect(onClose).not.toHaveBeenCalled()
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1)
  expect(document.querySelector<HTMLInputElement>('#ed-title')!.value).toBe('未保存的新标题')
  expect(edit).not.toHaveBeenCalled()
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })))
  expect(onClose).toHaveBeenCalledOnce()
})

it('组织刷新保留新增的未保存标签，保存仍提交用户选择', async () => {
  await mount()
  await chooseTag()
  await act(async () => publishDataChange(['organization']))
  expect(document.querySelector('[aria-label="移除标签 工作"]')).not.toBeNull()
  await click('.modal__actions .btn--primary')
  expect(ipc.saveTask).toHaveBeenCalledWith(task.id, expect.any(Object), [tag.id])
})

it('组织刷新保留移除的未保存标签', async () => {
  vi.mocked(org.taskTagsGet).mockResolvedValue([tag])
  await mount()
  await click('[aria-label="移除标签 工作"]')
  await act(async () => publishDataChange(['organization']))
  expect(document.querySelector('[aria-label="移除标签 工作"]')).toBeNull()
  await click('.modal__actions .btn--primary')
  expect(ipc.saveTask).toHaveBeenCalledWith(task.id, expect.any(Object), [])
})

it('刷新在选择标签前开始、在选择后返回，也保留新选择', async () => {
  await mount()
  let finish!: (tags: org.Tag[]) => void
  vi.mocked(org.taskTagsGet).mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  await act(async () => publishDataChange(['organization']))
  await chooseTag()
  await act(async () => finish([]))
  expect(document.querySelector('[aria-label="移除标签 工作"]')).not.toBeNull()
})

it('未编辑标签时仍接受外部更新的标签', async () => {
  await mount()
  vi.mocked(org.taskTagsGet).mockResolvedValue([tag])
  await act(async () => publishDataChange(['organization']))
  expect(document.querySelector('[aria-label="移除标签 工作"]')).not.toBeNull()
})

it('已选择标签被外部删除后仍可见、可移除，不留下无法保存的隐藏选择', async () => {
  await mount()
  await chooseTag()
  vi.mocked(org.tagList).mockResolvedValue([])
  await act(async () => publishDataChange(['organization']))
  expect(document.querySelector('[aria-label="移除标签 工作"]')).not.toBeNull()
  await click('[aria-label="移除标签 工作"]')
  await click('.modal__actions .btn--primary')
  expect(ipc.saveTask).toHaveBeenCalledWith(task.id, expect.any(Object), [])
})
