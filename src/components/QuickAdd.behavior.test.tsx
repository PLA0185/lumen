// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as ipc from '../lib/ipc'
import type { Task } from '../lib/types'
import { QuickAdd } from './QuickAdd'

vi.mock('./AiAssistant', () => ({ AiAssistant: () => null }))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})
async function mount(onCancel = vi.fn()) {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<QuickAdd onCreated={() => {}} onCancel={onCancel} />))
  return onCancel
}
function field(label: string) {
  return document.querySelector<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(`[aria-label="${label}"]`)!
}
async function input(label: string, value: string) {
  const target = field(label)
  const prototype = target.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : target.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(target, value)
    target.dispatchEvent(new Event(target.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }))
  })
}
const TITLE = '任务标题，可包含日期、标签与优先级'
async function enter(options: KeyboardEventInit = {}) {
  await act(async () => field(TITLE).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, ...options })))
}

it.each([{ isComposing: true }, { keyCode: 229 }])('中文输入法选字 Enter 不创建任务：%j', async options => {
  const create = vi.spyOn(ipc, 'createTask').mockResolvedValue({ id: 'created' } as Task)
  await mount()
  await input(TITLE, '输入中的标题')
  await enter(options)
  expect(create).not.toHaveBeenCalled()
  expect(field(TITLE).value).toBe('输入中的标题')
  await enter()
  expect(create).toHaveBeenCalledOnce()
})

it('组合输入 Escape 不关闭快速添加', async () => {
  const onCancel = await mount()
  await enter({ key: 'Escape', isComposing: true })
  expect(onCancel).not.toHaveBeenCalled()
})

it('保存期间允许继续输入，旧保存返回后保留新标题、描述及优先级', async () => {
  let finish!: (task: Task) => void
  const create = vi.spyOn(ipc, 'createTask').mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    .mockResolvedValue({ id: 'second' } as Task)
  await mount()
  await input(TITLE, '第一项')
  await enter()
  expect(field(TITLE).disabled).toBe(false)
  await input(TITLE, '第二项')
  await input('任务描述', '第二项材料')
  await input('优先级', '3')
  await act(async () => finish({ id: 'first' } as Task))
  expect(field(TITLE).value).toBe('第二项')
  expect(field('任务描述').value).toBe('第二项材料')
  expect(field('优先级').value).toBe('3')
  expect(create).toHaveBeenCalledOnce()
  await enter()
  expect(create).toHaveBeenLastCalledWith(expect.objectContaining({ title: '第二项', description: '第二项材料', priority: 3 }))
  expect(field(TITLE).value).toBe('')
})

it('保存期间仅修改日期和周期也保留，未改草稿才会清空', async () => {
  let finish!: (task: Task) => void
  vi.spyOn(ipc, 'createTask').mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  await mount()
  await input(TITLE, '同一标题')
  await enter()
  await input('计划执行日期', '2026-10-05')
  await input('周期跨度', 'week')
  await act(async () => finish({ id: 'created' } as Task))
  expect(field(TITLE).value).toBe('同一标题')
  expect(field('计划执行日期').value).toBe('2026-10-05')
  expect(field('周期跨度').value).toBe('week')
})
