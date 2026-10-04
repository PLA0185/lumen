// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import * as ipc from '../lib/ipc'
import * as org from '../lib/organize-ipc'
import type { TaskCreationContext } from '../lib/task-creation-context'
import { ContextTaskList } from './ContextTaskList'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

describe('项目页上下文任务布局', () => {
  it('任务区复用项目页的居中内容宽度', async () => {
    vi.spyOn(ipc, 'countTasks').mockResolvedValue({ total: 0 })
    vi.spyOn(ipc, 'listTasks').mockResolvedValue([])
    vi.spyOn(org, 'subtaskProgressBatch').mockResolvedValue([])

    const host = document.createElement('div')
    document.body.append(host)
    root = createRoot(host)
    const context: TaskCreationContext = { label: '项目「Amazon」', projectId: 'amazon' }
    await act(async () => root!.render(
      <ContextTaskList context={context} onEdit={vi.fn()} onToggle={vi.fn()} onDelete={vi.fn()} onDuplicate={vi.fn()} />,
    ))

    const section = document.querySelector('[aria-label="项目「Amazon」的任务"]')
    expect(section?.classList.contains('organize')).toBe(true)
    expect(section?.classList.contains('context-task-list')).toBe(true)
  })
})
