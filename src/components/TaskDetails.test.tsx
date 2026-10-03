// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as ipc from '../lib/ipc'
import * as rec from '../lib/recurrence-ipc'
import * as org from '../lib/organize-ipc'
import { QuickAdd } from './QuickAdd'
import { SubtaskList } from './SubtaskList'
import { SubtaskPreview } from './SubtaskPreview'
import { formatCompletionTime } from '../lib/datetime'
import type { Task } from '../lib/types'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  window.getSelection()?.removeAllRanges()
  document.body.innerHTML = ''
})
async function mount(node: React.ReactNode) {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(node))
}
async function input(label: string, value: string) {
  const field = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[aria-label="${label}"]`)!
  const prototype = field.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(field, value)
    field.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
async function click(text: string) {
  const button = [...document.querySelectorAll('button')].find((node) => node.textContent?.trim() === text)!
  expect(button).toBeDefined()
  await act(async () => button.click())
}
const child: org.Subtask = { id: 'child', taskId: 'parent', title: 'CA', isDone: 0, sortOrder: 1,
  completedAt: null, createdAt: '2026-09-28', updatedAt: '2026-09-28' }
async function recurringList(rows: org.Subtask[] = []) {
  vi.spyOn(org, 'subtaskList').mockResolvedValue(rows)
  vi.spyOn(rec, 'recurringScopeInfo').mockResolvedValue({
    isRecurring: true, totalInstances: 90, completed: 1, completedBefore: 1,
  } as rec.ScopeInfo)
  await mount(<SubtaskList taskId="parent" taskTitle="关键词更新" isRecurring />)
}

it('普通新建真正提交多行描述，保存失败保留草稿，成功后才清空', async () => {
  const create = vi.spyOn(ipc, 'createTask').mockRejectedValueOnce(new Error('写入失败'))
    .mockResolvedValueOnce({ id: 'new' } as Task)
  await mount(<QuickAdd onCreated={() => {}} />)
  await input('任务标题，可包含日期、标签与优先级', '包装')
  await input('任务描述', '要完成什么\n注意事项')
  await click('添加')
  expect(create).toHaveBeenLastCalledWith(expect.objectContaining({ description: '要完成什么\n注意事项' }))
  expect(document.querySelector<HTMLTextAreaElement>('[aria-label="任务描述"]')!.value).toBe('要完成什么\n注意事项')
  expect(document.body.textContent).toContain('写入失败')
  await click('添加')
  expect(document.querySelector<HTMLTextAreaElement>('[aria-label="任务描述"]')!.value).toBe('')
})
it('重复子任务添加先选择范围，没有默认选中，取消不写入并保留输入', async () => {
  const change = vi.spyOn(org, 'subtaskChange').mockResolvedValue([child])
  await recurringList()
  await input('新子任务标题', 'CA')
  await click('添加')
  expect(change).not.toHaveBeenCalled()
  expect(document.querySelector('input[type="radio"]:checked')).toBeNull()
  expect(document.querySelectorAll('input[type="radio"]')).toHaveLength(2)
  await click('取消')
  expect(change).not.toHaveBeenCalled()
  expect(document.querySelector<HTMLInputElement>('[aria-label="新子任务标题"]')!.value).toBe('CA')
  await click('添加')
  await act(async () => document.querySelector<HTMLInputElement>('input[value="whole_series"]')!.click())
  await click('确认修改')
  expect(change).toHaveBeenCalledExactlyOnceWith('parent', { kind: 'create', title: 'CA' }, 'whole_series')
  expect(document.querySelector('[role="dialog"]')).toBeNull()
})
it('重复子任务删除同样要求范围，失败后保持对话框且显示错误', async () => {
  const change = vi.spyOn(org, 'subtaskChange').mockRejectedValue(new Error('事务失败'))
  await recurringList([child])
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="删除子任务「CA」"]')!.click())
  expect(change).not.toHaveBeenCalled()
  await act(async () => document.querySelector<HTMLInputElement>('input[value="this_only"]')!.click())
  await click('确认删除')
  expect(change).toHaveBeenCalledExactlyOnceWith('parent', { kind: 'delete', id: 'child' }, 'this_only')
  expect(document.body.textContent).toContain('事务失败')
  expect(document.querySelector('[role="dialog"]')).not.toBeNull()
})
it('复制上一次子任务也先选择范围；完成勾选只更新本次状态', async () => {
  const change = vi.spyOn(org, 'subtaskChange').mockResolvedValue([child])
  const update = vi.spyOn(org, 'subtaskUpdate').mockResolvedValue({ ...child, isDone: 1 })
  await recurringList()
  await click('从上一次复制子任务')
  expect(change).not.toHaveBeenCalled()
  await act(async () => document.querySelector<HTMLInputElement>('input[value="this_only"]')!.click())
  await click('确认修改')
  expect(change).toHaveBeenCalledExactlyOnceWith('parent', { kind: 'copy_previous' }, 'this_only')
  await act(async () => document.querySelector<HTMLButtonElement>('[role="checkbox"]')!.click())
  expect(update).toHaveBeenCalledExactlyOnceWith('child', { isDone: true })
  expect(document.querySelector('[role="dialog"]')).toBeNull()
})
it('完成简览显示实际完成时间到秒，缺失的历史时间明确显示未记录', async () => {
  const completedAt = '2026-09-29T06:23:45.000Z'
  await mount(<SubtaskPreview items={[{ ...child, isDone: 1, completedAt }, { ...child, id: 'legacy', isDone: 1 }]} />)
  expect(document.body.textContent).toContain(formatCompletionTime(completedAt))
  expect(document.body.textContent).toContain('完成时间未记录')
})

it('子任务重命名按钮可聚焦，键盘激活后回车提交新标题', async () => {
  vi.spyOn(org, 'subtaskList').mockResolvedValue([child])
  const change = vi.spyOn(org, 'subtaskChange').mockResolvedValue([{ ...child, title: '新标题' }])
  await mount(<SubtaskList taskId="parent" />)
  const button = document.querySelector<HTMLButtonElement>('button[aria-label="重命名子任务「CA」"]')
  expect(button).not.toBeNull()
  await act(async () => {
    button!.focus()
    expect(document.activeElement).toBe(button)
    // Enter/Space on a native button produces this detail=0 activation event.
    button!.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 }))
  })
  expect(document.activeElement).toBe(document.querySelector('[aria-label="子任务标题"]'))
  await input('子任务标题', '新标题')
  await act(async () => document.querySelector('[aria-label="子任务标题"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
  expect(change).toHaveBeenCalledExactlyOnceWith('parent', { kind: 'rename', id: 'child', title: '新标题' }, undefined)
  expect(document.body.textContent).toContain('新标题')
})

it('子任务双击标题仍可进入重命名', async () => {
  vi.spyOn(org, 'subtaskList').mockResolvedValue([child])
  await mount(<SubtaskList taskId="parent" />)
  const button = document.querySelector<HTMLButtonElement>('button[aria-label="重命名子任务「CA」"]')
  expect(button).not.toBeNull()
  await act(async () => button!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, detail: 2 })))
  expect(document.querySelector<HTMLInputElement>('[aria-label="子任务标题"]')!.value).toBe('CA')
})

it.each(['子任务标题', '新子任务标题'])('%s 的组合输入回车只选字，结束组合后回车才提交', async label => {
  vi.spyOn(org, 'subtaskList').mockResolvedValue([child])
  const change = vi.spyOn(org, 'subtaskChange').mockResolvedValue([{ ...child, title: '中文标题' }])
  await mount(<SubtaskList taskId="parent" />)
  if (label === '子任务标题') await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="重命名子任务「CA」"]')!.click())
  await input(label, '中文标题')
  const field = document.querySelector(`[aria-label="${label}"]`)!
  await act(async () => field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true })))
  expect(change).not.toHaveBeenCalled()
  await act(async () => field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 229, bubbles: true })))
  expect(change).not.toHaveBeenCalled()
  await act(async () => field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
  expect(change).toHaveBeenCalledOnce()
})

it('鼠标选择子任务标题文本后不立即进入重命名，选区保留', async () => {
  vi.spyOn(org, 'subtaskList').mockResolvedValue([child])
  await mount(<SubtaskList taskId="parent" />)
  const button = document.querySelector('[aria-label="重命名子任务「CA」"]')!
  const range = document.createRange()
  range.selectNodeContents(button)
  window.getSelection()!.addRange(range)
  await act(async () => button.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })))
  expect(document.querySelector('[aria-label="子任务标题"]')).toBeNull()
  expect(window.getSelection()!.toString()).toBe('CA')
})

it('重命名按 Esc 取消，保留原标题且不提交修改', async () => {
  vi.spyOn(org, 'subtaskList').mockResolvedValue([child])
  const change = vi.spyOn(org, 'subtaskChange')
  await mount(<SubtaskList taskId="parent" />)
  await act(async () => document.querySelector<HTMLButtonElement>('[aria-label="重命名子任务「CA」"]')!.click())
  await input('子任务标题', '取消的草稿')
  await act(async () => document.querySelector('[aria-label="子任务标题"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
  expect(change).not.toHaveBeenCalled()
  expect(document.querySelector('[aria-label="子任务标题"]')).toBeNull()
  expect(document.querySelector('[aria-label="重命名子任务「CA」"]')?.textContent).toBe('CA')
})
