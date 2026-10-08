// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as cloud from '../lib/cloud-sync-ipc'
import { IpcError } from '../lib/ipc'
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
async function fill(label: string, value: string) {
  const field = [...document.querySelectorAll('label')].find(l => l.textContent?.startsWith(label))?.querySelector('input') as HTMLInputElement
  expect(field, label).toBeTruthy()
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(field, value); field.dispatchEvent(new Event('input', { bubbles: true })); field.dispatchEvent(new Event('change', { bubbles: true })) })
}
it('顶部入口读取默认选择，修改本次范围方向不改默认，暂停时仍可手动执行', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(connected)
  const now = vi.spyOn(cloud, 'cloudNow').mockResolvedValue(connected)
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSyncButton />))
  await click('同步')
  expect(document.body.textContent).toContain('知识库原件和解析正文只在选择“全部业务数据”时加密同步')
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
  vi.spyOn(cloud, 'cloudRecoveryCode').mockResolvedValue('LUMEN1-saved-key')
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  expect(document.body.textContent).toContain('自动同步开关：已开启')
  expect(document.body.textContent).toContain('待上传云端：2 项变更（尚未上传）')
  expect(document.body.textContent).toContain('“全部业务数据”还会加密同步知识库原件与解析正文')
  const key = document.querySelector('[aria-label="Lumen 同步密钥"]') as HTMLInputElement
  expect(key.value).toBe('LUMEN1-saved-key')
  expect(key.type).toBe('password')
  await click('显示密钥')
  expect(key.type).toBe('text')
})
it('首次连接成功后立即显示可复制的 Lumen 同步密钥', async () => {
  let current: cloud.CloudStatus = { ...connected, config: null }
  vi.spyOn(cloud, 'cloudStatus').mockImplementation(async () => current)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockImplementation(async () => { current = connected; return connected })
  vi.spyOn(cloud, 'cloudRecoveryCode').mockResolvedValue('LUMEN1-test-key')
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  expect(document.body.textContent).toContain('尚未连接，首次连接成功后会生成同步密钥')
  await fill('坚果云账号', 'test@example.com')
  await fill('第三方应用密码', 'app-password')
  await click('验证并连接云空间')
  expect((document.querySelector('[aria-label="Lumen 同步密钥"]') as HTMLInputElement).value).toBe('LUMEN1-test-key')
  expect(document.body.textContent).toContain('复制 Lumen 同步密钥')
})
it('验证云空间期间显示进行中状态，完成后明确显示连接成功', async () => {
  let current: cloud.CloudStatus = { ...connected, config: null }
  vi.spyOn(cloud, 'cloudStatus').mockImplementation(async () => current)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  let finishConnect!: (status: cloud.CloudStatus) => void
  const connect = vi.spyOn(cloud, 'cloudConnect').mockImplementation(() => new Promise(resolve => { finishConnect = resolve }))
  vi.spyOn(cloud, 'cloudRecoveryCode').mockResolvedValue('LUMEN1-test-key')
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('坚果云账号', 'test@example.com')
  await fill('第三方应用密码', 'app-password')
  const button = [...document.querySelectorAll('button')].find(b => b.textContent === '验证并连接云空间')!
  await act(async () => button.click())
  expect(connect).toHaveBeenCalledOnce()
  expect(button.textContent).toBe('正在验证并连接…')
  expect(document.body.textContent).toContain('正在验证服务器、账号和云空间')
  current = connected
  await act(async () => finishConnect(connected))
  expect(document.body.textContent).toContain('云空间连接成功')
})
it('验证云空间失败时在按钮旁明确显示失败原因', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue({ ...connected, config: null })
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockRejectedValue(new Error('坚果云密码错误'))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('坚果云账号', 'test@example.com')
  await fill('第三方应用密码', 'wrong-password')
  await click('验证并连接云空间')
  const feedback = document.querySelector('.cloud-settings__connect-feedback')
  expect(feedback?.textContent).toContain('云空间连接失败：坚果云密码错误')
  expect(feedback?.getAttribute('role')).toBe('alert')
})
it('本机请求额度冷却时说明尚未发送验证请求，不误报连接失败', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue({ ...connected, config: null })
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockRejectedValue(new IpcError(
    'rate_limited',
    'Lumen 本机 WebDAV 请求额度已用完，约 8 分钟后自动重试；当前 WebDAV 请求尚未发出，本机未上传内容仍保留。',
    null,
    null,
  ))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('坚果云账号', 'test@example.com')
  await fill('第三方应用密码', 'app-password')
  await click('验证并连接云空间')
  const feedback = document.querySelector('.cloud-settings__connect-feedback')
  expect(feedback?.textContent).toContain('本机 WebDAV 请求额度已用完')
  expect(feedback?.textContent).toContain('没有账号验证结果')
  expect(feedback?.textContent).toContain('当前有 2 项变更待上传')
  expect(feedback?.textContent).not.toContain('云空间连接失败')
  expect(feedback?.getAttribute('role')).toBe('status')
})
