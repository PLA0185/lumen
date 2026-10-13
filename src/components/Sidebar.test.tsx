// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { Sidebar } from './Sidebar'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  document.body.innerHTML = ''
})

it('备忘和流程有独立导航项，当前入口各自高亮', async () => {
  const onSelect = vi.fn()
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)

  await act(async () => root!.render(<Sidebar current="memos" onSelect={onSelect} counts={{}} />))

  const labels = [...document.querySelectorAll('.nav-item__label')].map(node => node.textContent)
  expect(labels).toContain('备忘')
  expect(labels).toContain('流程')
  expect(labels).not.toContain('备忘与流程')

  const flow = [...document.querySelectorAll<HTMLButtonElement>('.nav-item')]
    .find(button => button.querySelector('.nav-item__label')?.textContent === '流程')!
  await act(async () => flow.click())
  expect(onSelect).toHaveBeenCalledWith('flows')
  expect(flow.getAttribute('aria-current')).toBeNull()

  await act(async () => root!.render(<Sidebar current="flows" onSelect={onSelect} counts={{}} />))
  const active = document.querySelector('.nav-item[aria-current="page"] .nav-item__label')
  expect(active?.textContent).toBe('流程')
})

it('重复大类分别提供每日、每周、每月、每季度和每年的系列入口', async () => {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  const onSelect = vi.fn()
  await act(async () => root!.render(<Sidebar current="recurring-daily" onSelect={onSelect} counts={{}} />))

  const labels = [...document.querySelectorAll('.nav-item__label')].map(node => node.textContent)
  expect(labels).toEqual(expect.arrayContaining(['每日重复', '每周重复', '每月重复', '每季度重复', '每年重复']))
  expect(labels).toContain('周任务')
  const daily = [...document.querySelectorAll<HTMLButtonElement>('.nav-item')]
    .find(button => button.querySelector('.nav-item__label')?.textContent === '每日重复')!
  await act(async () => daily.click())
  expect(onSelect).toHaveBeenCalledWith('recurring-daily')
})
