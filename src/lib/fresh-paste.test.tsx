// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { installFreshPaste } from './fresh-paste'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
const cleanup: (() => void)[] = []
afterEach(() => {
  cleanup
    .splice(0)
    .reverse()
    .forEach((fn) => fn())
  document.body.innerHTML = ''
})

async function setup(read: () => Promise<string>) {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  function Field() {
    const [value, setValue] = useState('旧内容')
    return (
      <>
        <input value={value} onChange={(e) => setValue(e.target.value)} />
        <output>{value}</output>
      </>
    )
  }
  await act(async () => root.render(<Field />))
  cleanup.push(() => {
    act(() => root.unmount())
  })
  const onError = vi.fn()
  cleanup.push(installFreshPaste(read, onError))
  const field = host.querySelector('input')!
  field.focus()
  field.select()
  return { field, onError, host }
}

async function key(field: HTMLInputElement, shiftKey = false) {
  await act(async () =>
    field.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'v',
        ctrlKey: true,
        shiftKey,
        bubbles: true,
        cancelable: true,
      }),
    ),
  )
}

describe('手动粘贴读取当前系统剪贴板', () => {
  it('交给内容编辑器处理图片，文本拦截器不能吞掉粘贴事件', async () => {
    const read = vi.fn().mockResolvedValue('旧文本')
    const { field } = await setup(read)
    field.dataset.contentEditor = 'true'
    const event = new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true, cancelable: true })
    field.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(read).not.toHaveBeenCalled()
  })
  it('Ctrl+V 与 Ctrl+Shift+V 都替换选择范围，每次重新读取并更新 React 状态', async () => {
    const read = vi
      .fn()
      .mockResolvedValueOnce('第一份')
      .mockResolvedValueOnce('新复制的第二份')
    const { field, host } = await setup(read)
    await key(field)
    expect(field.value).toBe('第一份')
    field.select()
    await key(field, true)
    expect(field.value).toBe('新复制的第二份')
    expect(host.querySelector('output')?.textContent).toBe('新复制的第二份')
    expect(read).toHaveBeenCalledTimes(2)
  })

  it('只替换当前选中的文字，保留两侧文本', async () => {
    const { field } = await setup(async () => '替换')
    field.setSelectionRange(1, 2)
    await key(field, true)
    expect(field.value).toBe('旧替换容')
  })

  it('读取期间用户编辑或改变光标时，不把过时结果写回', async () => {
    let resolve!: (text: string) => void
    const { field } = await setup(
      () =>
        new Promise((done) => {
          resolve = done
        }),
    )
    await key(field)
    field.setSelectionRange(0, 0)
    await act(async () => resolve('过时结果'))
    expect(field.value).toBe('旧内容')
  })

  it('读取失败时保留输入框，并明确提示失败', async () => {
    const { field, onError } = await setup(async () => {
      throw new Error('busy')
    })
    await key(field, true)
    expect(field.value).toBe('旧内容')
    expect(onError).toHaveBeenCalledOnce()
  })

  it('进入或重新聚焦输入框不读取剪贴板', async () => {
    const read = vi.fn()
    const { field } = await setup(read)
    field.blur()
    field.focus()
    expect(read).not.toHaveBeenCalled()
  })
})
