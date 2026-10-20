// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it } from 'vitest'
import { FitToolbar } from './FitToolbar'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
it('保留完整工具栏内容，不通过缩放压缩按钮文字', async () => {
  const host = document.createElement('div'); document.body.append(host)
  const root = createRoot(host)
  try {
    await act(async () => root.render(<FitToolbar><button>新建流程</button><button>备忘回收站</button><button>刷新列表</button></FitToolbar>))
    const row = host.querySelector<HTMLElement>('.memos__toolbar-row')!
    expect(row.style.transform).toBe('')
    expect(host.querySelector<HTMLElement>('.memos__toolbar')!.style.height).toBe('')
    expect([...row.querySelectorAll('button')].map(button => button.textContent)).toEqual(['新建流程', '备忘回收站', '刷新列表'])
  } finally {
    act(() => root.unmount()); host.remove()
  }
})
