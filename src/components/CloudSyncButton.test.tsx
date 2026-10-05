// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as cloud from '../lib/cloud-sync-ipc'
import { CloudSyncButton } from './CloudSyncButton'
import { CloudSettings } from './CloudSettings'
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })
let root: Root | undefined
afterEach(() => { act(() => root?.unmount()); root = undefined; vi.restoreAllMocks(); document.body.innerHTML = '' })
const connected = { config: { server: 'https://example.test', account: 'test', folder: 'Lumen', connectionId: 'test', workspaceId: 'space', enabled: false, inheritAll: true, defaultSync: { scope: 'memos' as const, direction: 'download' as const } }, pending: 2, conflicts: [], lastScan: null, lastUpload: null, lastError: null, retryUntil: 0 }
async function click(label: string) {
  const button = [...document.querySelectorAll('button')].find(b => b.textContent === label)
  expect(button, label).toBeTruthy()
  await act(async () => button!.click())
}
async function select(label: string, value: string) {
  const field = document.querySelector(`[aria-label="${label}"]`) as HTMLSelectElement
  expect(field, label).toBeTruthy()
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('change', { bubbles: true })) })
}
it('顶部入口读取默认选择，修改本次范围方向不改默认，暂停时仍可手动执行', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(connected)
  const now = vi.spyOn(cloud, 'cloudNow').mockResolvedValue(connected)
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSyncButton />))
  await click('同步')
  expect((document.querySelector('[aria-label="本次同步内容"]') as HTMLSelectElement).value).toBe('memos')
  expect((document.querySelector('[aria-label="本次同步方向"]') as HTMLSelectElement).value).toBe('download')
  await select('本次同步内容', 'tasks'); await select('本次同步方向', 'upload')
  expect(now).not.toHaveBeenCalled()
  await click('开始同步')
  expect(now).toHaveBeenCalledWith({ scope: 'tasks', direction: 'upload' })
  expect(document.body.textContent).toContain('本次同步已执行')
  await click('关闭')
  await click('同步')
  expect((document.querySelector('[aria-label="本次同步内容"]') as HTMLSelectElement).value).toBe('memos')
})
it('同步失败显示错误，不显示完成提示', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(connected)
  vi.spyOn(cloud, 'cloudNow').mockRejectedValue(new Error('网络失败'))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSyncButton />))
  await click('同步'); await click('开始同步')
  expect(document.body.textContent).toContain('网络失败')
  expect(document.body.textContent).not.toContain('本次同步已执行')
})
it('云同步设置保存默认内容和方向，不重连也不需要重新填写密码', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(connected)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  const saved = vi.spyOn(cloud, 'cloudSetDefaults').mockResolvedValue(connected)
  const connect = vi.spyOn(cloud, 'cloudConnect')
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await select('默认同步内容', 'tasks'); await select('默认同步方向', 'upload')
  await click('保存默认同步类型')
  expect(saved).toHaveBeenCalledWith({ scope: 'tasks', direction: 'upload' })
  expect(connect).not.toHaveBeenCalled()
  expect(document.body.textContent).toContain('默认同步类型已保存')
})
it('云同步设置把自动同步开关和上传队列状态分开说明', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue({ ...connected, config: { ...connected.config, enabled: true } })
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  expect(document.body.textContent).toContain('自动同步开关：已开启')
  expect(document.body.textContent).toContain('待上传云端：2 项变更（尚未上传）')
})
