// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as up from '../lib/update-ipc'
import { UpdatePanel } from './UpdatePanel'

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

let root: Root | undefined
let host: HTMLDivElement | undefined

afterEach(() => {
  act(() => root?.unmount())
  root = undefined
  host?.remove()
  host = undefined
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

it('切换离开设置页再回来时，更新下载在后台继续并保留进度', async () => {
  const handle = {} as up.UpdateHandle
  const check = vi.spyOn(up, 'checkUpdate').mockResolvedValue({
    info: {
      version: '0.4.44',
      date: null,
      notes: null,
      currentVersion: '0.4.41',
    },
    handle,
  })
  let reportProgress: ((progress: up.UpdateProgress) => void) | undefined
  const download = vi.spyOn(up, 'downloadAndInstall').mockImplementation((_handle, onProgress) => {
    reportProgress = onProgress
    return new Promise<void>(() => undefined)
  })

  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<UpdatePanel currentVersion="0.4.41" />))
  await act(async () => {})

  const install = [...document.querySelectorAll('button')].find(button => button.textContent === '下载并安装 v0.4.44')
  expect(install).toBeTruthy()
  await act(async () => install!.click())
  await act(async () => reportProgress?.({ percent: 0.42, downloaded: 42 }))

  expect(document.body.textContent).toContain('下载中 42%')
  await act(async () => root!.unmount())
  root = undefined
  host.remove()

  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  await act(async () => root!.render(<UpdatePanel currentVersion="0.4.41" />))
  await act(async () => {})

  expect(document.body.textContent).toContain('下载中 42%')
  expect(check).toHaveBeenCalledTimes(1)
  expect(download).toHaveBeenCalledTimes(1)
})
