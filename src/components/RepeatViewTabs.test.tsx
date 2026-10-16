// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { RepeatViewTabs } from './RepeatViewTabs'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  document.body.innerHTML = ''
})

it('所有重复频率都作为常驻横向入口显示，并标记当前周期', async () => {
  const host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  const onSelect = vi.fn()

  await act(async () => root!.render(<RepeatViewTabs current="recurring-monthly" onSelect={onSelect} />))

  const tabs = [...document.querySelectorAll<HTMLButtonElement>('.repeat-view-tabs__item')]
  expect(tabs.map((tab) => tab.textContent)).toEqual(['每日', '每周', '每月', '每季度', '每年'])
  expect(tabs[2]?.getAttribute('aria-current')).toBe('page')
  expect(tabs.every((tab) => !tab.hidden)).toBe(true)

  await act(async () => tabs[1]?.click())
  expect(onSelect).toHaveBeenCalledWith('recurring-weekly')
})
