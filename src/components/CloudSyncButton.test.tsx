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
const connected = { config: { provider: 'github' as const, server: '', account: '', folder: 'Lumen', owner: 'PLA0185', repository: 'lumen-sync', branch: 'main', connectionId: 'test', workspaceId: 'space', enabled: false, inheritAll: true, defaultSync: { scope: 'memos' as const, direction: 'download' as const } }, pending: 2, conflicts: [], lastScan: null, lastUpload: null, lastError: null, retryUntil: 0 }
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
it('旧版云同步在顶部入口提示切换到 GitHub，不能继续手动上传', async () => {
  const legacy = { ...connected, config: { ...connected.config!, provider: 'webDav' as const, enabled: true } }
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(legacy)
  const now = vi.spyOn(cloud, 'cloudNow')
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSyncButton />))
  await click('同步')
  expect(document.body.textContent).toContain('旧版云同步已暂停')
  expect(document.body.textContent).toContain('配置 GitHub 私有仓库')
  expect((document.querySelector('button.btn--primary') as HTMLButtonElement | null)?.disabled).toBe(true)
  expect(now).not.toHaveBeenCalled()
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
  expect(document.body.textContent).toContain('“全部业务数据”还会加密同步知识库原件与解析正文')
  expect(document.body.textContent).toContain('不需要单独复制或填写 Lumen 密钥')
  expect(document.querySelector('[aria-label="Lumen 同步密钥"]')).toBeNull()
})
it('检测到旧版配置时说明其已暂停且旧云端不会迁移', async () => {
  const legacy = { ...connected, config: { ...connected.config!, provider: 'webDav' as const, enabled: true } }
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue(legacy)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  expect(document.body.textContent).toContain('此版本已停止该连接')
  expect(document.body.textContent).toContain('旧云端内容不会自动搬到 GitHub')
  expect(document.body.textContent).not.toContain('Lumen 同步密钥')
  expect(document.body.textContent).not.toContain('自动同步开关：已开启')
  expect(document.body.textContent).not.toContain('立即同步')
})
it('首次连接不要求 Lumen 密钥，其他设备使用相同 GitHub Token', async () => {
  let current: cloud.CloudStatus = { ...connected, config: null }
  vi.spyOn(cloud, 'cloudStatus').mockImplementation(async () => current)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockImplementation(async () => { current = connected; return connected })
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  expect(document.body.textContent).toContain('请在每台电脑使用同一个 Token')
  await fill('GitHub 用户名或组织', 'PLA0185')
  await fill('私有仓库名', 'lumen-sync')
  await fill('GitHub Fine-grained Token', 'github_pat_example')
  await click('验证并连接 GitHub')
  expect(document.body.textContent).toContain('其他电脑使用同一个 GitHub Token 即可加入')
  expect(document.querySelector('[aria-label="Lumen 同步密钥"]')).toBeNull()
})
it('设置页只提供 GitHub，同步连接提交仓库和分支信息', async () => {
  let current: cloud.CloudStatus = { ...connected, config: null }
  vi.spyOn(cloud, 'cloudStatus').mockImplementation(async () => current)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  const githubConfig = { ...connected.config!, provider: 'github' as const, owner: 'PLA0185', repository: 'lumen-sync', branch: 'main' }
  const githubStatus = { ...connected, config: githubConfig }
  const connect = vi.spyOn(cloud, 'cloudConnect').mockImplementation(async () => { current = githubStatus; return githubStatus })
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('GitHub 用户名或组织', 'PLA0185')
  await fill('私有仓库名', 'lumen-sync')
  await fill('GitHub Fine-grained Token', 'github_pat_example')
  await click('验证并连接 GitHub')
  expect(connect).toHaveBeenCalledWith(expect.objectContaining({ provider: 'github', owner: 'PLA0185', repository: 'lumen-sync', branch: 'main', password: 'github_pat_example' }))
  expect(document.querySelector('[aria-label="同步方式"]')).toBeNull()
})
it('验证云空间期间显示进行中状态，完成后明确显示连接成功', async () => {
  let current: cloud.CloudStatus = { ...connected, config: null }
  vi.spyOn(cloud, 'cloudStatus').mockImplementation(async () => current)
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  let finishConnect!: (status: cloud.CloudStatus) => void
  const connect = vi.spyOn(cloud, 'cloudConnect').mockImplementation(() => new Promise(resolve => { finishConnect = resolve }))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('GitHub 用户名或组织', 'PLA0185')
  await fill('私有仓库名', 'lumen-sync')
  await fill('GitHub Fine-grained Token', 'github_pat_example')
  const button = [...document.querySelectorAll('button')].find(b => b.textContent === '验证并连接 GitHub')!
  await act(async () => button.click())
  expect(connect).toHaveBeenCalledOnce()
  expect(button.textContent).toBe('正在验证并连接…')
  expect(document.body.textContent).toContain('正在验证 GitHub 仓库权限并连接')
  current = connected
  await act(async () => finishConnect(connected))
  expect(document.body.textContent).toContain('GitHub 私有仓库连接成功')
})
it('GitHub 连接失败时在按钮旁明确显示失败原因', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue({ ...connected, config: null })
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockRejectedValue(new Error('GitHub Token 无权限'))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('GitHub 用户名或组织', 'PLA0185')
  await fill('私有仓库名', 'lumen-sync')
  await fill('GitHub Fine-grained Token', 'github_pat_example')
  await click('验证并连接 GitHub')
  const feedback = document.querySelector('.cloud-settings__connect-feedback')
  expect(feedback?.textContent).toContain('GitHub 连接失败：GitHub Token 无权限')
  expect(feedback?.getAttribute('role')).toBe('alert')
})
it('本机请求额度冷却时说明尚未发送验证请求，不误报连接失败', async () => {
  vi.spyOn(cloud, 'cloudStatus').mockResolvedValue({ ...connected, config: null })
  vi.spyOn(cloud, 'cloudBusinessConflicts').mockResolvedValue([])
  vi.spyOn(cloud, 'cloudConnect').mockRejectedValue(new IpcError(
    'rate_limited',
    'Lumen 本机 GitHub 请求额度已用完，约 8 分钟后自动重试；当前 GitHub 请求尚未发出，本机未上传内容仍保留。',
    null,
    null,
  ))
  const host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  await act(async () => root!.render(<CloudSettings />))
  await fill('GitHub 用户名或组织', 'PLA0185')
  await fill('私有仓库名', 'lumen-sync')
  await fill('GitHub Fine-grained Token', 'github_pat_example')
  await click('验证并连接 GitHub')
  const feedback = document.querySelector('.cloud-settings__connect-feedback')
  expect(feedback?.textContent).toContain('本机 GitHub 请求额度已用完')
  expect(feedback?.textContent).toContain('暂时无法判断 Token 是否有效')
  expect(feedback?.textContent).toContain('当前有 2 项变更待上传')
  expect(feedback?.textContent).not.toContain('云空间连接失败')
  expect(feedback?.getAttribute('role')).toBe('status')
})
