import { useEffect, useState } from 'react'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as cloud from '../lib/cloud-sync-ipc'
import { IpcError } from '../lib/ipc'
import { CloudHistory } from './CloudHistory'
const labels: Record<string, string> = { tasks: '任务', subtasks: '子任务', projects: '项目', categories: '分类', tags: '标签', reminders: '提醒', attachments: '附件', task_series: '重复任务', task_series_template: '重复子任务', task_series_segments: '重复规则', task_series_skips: '跳过记录', task_series_rebuilds: '重建记录', task_series_tags: '重复标签', task_tags: '任务标签', task_dependencies: '依赖', focus_sessions: '专注记录', goals: '目标', settings: '业务设置' }
const message = (e: unknown) => e instanceof IpcError ? e.userMessage() : String(e)
export function CloudSettings() {
  const [status, setStatus] = useState<cloud.CloudStatus | null>(null)
  const [form, setForm] = useState({ server: 'https://dav.jianguoyun.com/dav/', account: '', folder: 'Lumen', password: '', recoveryCode: '', inheritAll: true })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [historyId, setHistoryId] = useState('')
  const [conflicts, setConflicts] = useState<cloud.BusinessConflict[]>([])
  useEffect(() => {
    let active = true
    const refresh = () => { void Promise.all([cloud.cloudStatus(), cloud.cloudBusinessConflicts()]).then(([s, c]) => { if (active) { setStatus(s); setConflicts(c) } }).catch(e => { if (active) setError(message(e)) }) }
    void cloud.cloudStatus().then(s => { if (active && s.config) setForm(f => ({ ...f, server: s.config!.server, account: s.config!.account, folder: s.config!.folder, inheritAll: s.config!.inheritAll })) }).catch(e => { if (active) setError(message(e)) })
    refresh(); const timer = setInterval(refresh, 3000)
    return () => { active = false; clearInterval(timer) }
  }, [])
  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError(''); setNotice('')
    try { await action(); setStatus(await cloud.cloudStatus()); setConflicts(await cloud.cloudBusinessConflicts()) }
    catch (e) { setError(message(e)) } finally { setBusy(false) }
  }
  return <section className="setgroup">
    <h2>云同步 · 坚果云 / WebDAV</h2>
    <p className="setgroup__hint">修改先保存在本机，再自动上传。另一台电脑打开 Lumen 后下载，不需要运行坚果云客户端。云端内容加密保存。</p>
    {(error || status?.lastError) && <p className="alert alert--error" role="alert">{error || status?.lastError}</p>}
    {notice && <p role="status">{notice}</p>}
    <fieldset disabled={busy} className="cloud-settings__fields">
      <label>服务器地址<input className="input" value={form.server} onChange={e => setForm({ ...form, server: e.target.value })} /></label>
      <label>坚果云账号<input className="input" autoComplete="username" value={form.account} onChange={e => setForm({ ...form, account: e.target.value })} /></label>
      <label>同步文件夹<input className="input" value={form.folder} onChange={e => setForm({ ...form, folder: e.target.value })} /></label>
      <label>第三方应用密码<input className="input" type="password" autoComplete="new-password" value={form.password} onChange={e => setForm({ ...form, password: e.target.value })} /></label>
      <label>另一台电脑的恢复码<input className="input" type="password" autoComplete="off" placeholder="首次创建云空间留空；公司电脑接入时必填" value={form.recoveryCode} onChange={e => setForm({ ...form, recoveryCode: e.target.value })} /></label>
      <label>这台电脑继承的内容<select className="input" value={String(form.inheritAll)} onChange={e => setForm({ ...form, inheritAll: e.target.value === 'true' })}><option value="true">全部业务数据</option><option value="false">只继承备忘录和流程（含图片及文件）</option></select></label>
      <button className="btn btn--primary" disabled={!form.account.trim() || !form.password} onClick={() => void run(async () => { setStatus(await cloud.cloudConnect(form)); setForm(f => ({ ...f, password: '', recoveryCode: '' })); setNotice('连接成功，已开启自动同步。请复制恢复码并保存，公司电脑接入时需要。') })}>验证并连接云空间</button>
    </fieldset>
    {status?.config && <>
      <p role="status">{status.config.enabled ? '自动同步已开启' : '自动同步已暂停'} · {status.pending ? `${status.pending} 个版本等待上传` : '当前没有待上传版本'}<br />最近上传：{status.lastUpload ? new Date(status.lastUpload).toLocaleString() : '尚未上传'} · 最近检查：{status.lastScan ? new Date(status.lastScan).toLocaleString() : '尚未检查'}</p>
      <p className="setgroup__hint">任务、项目、标签、重复规则、子任务、提醒、附件、专注、目标、备忘和流程都可以同步。窗口、快捷键和账号密码由各电脑单独设置。任务附件最多 200 MiB，内容中的图片/文件最多 20 MiB；大附件会消耗网盘流量。</p>
      <div className="memos__toolbar">
        <button className="btn" disabled={busy} onClick={() => void run(async () => { setStatus(await cloud.cloudNow()) })}>立即同步</button>
        <button className="btn" disabled={busy} onClick={() => void run(async () => { setStatus(await cloud.cloudEnable(!status.config!.enabled)) })}>{status.config.enabled ? '暂停同步' : '恢复同步'}</button>
        <button className="btn" disabled={busy} onClick={() => void run(async () => { setStatus(await cloud.cloudInheritance(form.inheritAll)); setNotice('已更新本机继承范围，云端其它数据仍然保留。') })}>应用本机继承范围</button>
        <button className="btn" disabled={busy} onClick={() => void run(async () => { await writeText(await cloud.cloudRecoveryCode()); setNotice('恢复码已复制。请妥善保存；在公司电脑填入，不要发到聊天或 GitHub。') })}>复制同步恢复码</button>
      </div>
      {status.conflicts.map(id => <button key={id} className="btn" onClick={() => setHistoryId(id)}>查看备忘/流程冲突</button>)}
      {conflicts.map(c => <div key={c.id} className="setgroup">
        <h3>{labels[c.table] ?? '业务记录'}需要选择版本</h3>
        <p className="setgroup__hint">两边内容都已保留。采用一个版本后，另一个仍在历史中。</p>
        {c.versions.map(v => <div key={v.id}>
          <p>{c.heads.includes(v.id) ? '当前候选 · ' : '历史版本 · '}{new Date(v.createdAt).toLocaleString()} · {v.row ? String(v.row.title ?? v.row.name ?? v.row.file_name ?? '记录') : '删除版本'}</p>
          <details><summary>查看这个版本的详细内容</summary><pre className="selectable cloud-settings__version">{JSON.stringify(v.row, null, 2)}</pre></details>
          <button className="btn" disabled={busy} onClick={() => void run(async () => { await cloud.cloudBusinessResolve(c.id, v.id, c.heads) })}>采用这个版本</button>
        </div>)}
      </div>)}
    </>}
    {historyId && <CloudHistory id={historyId} onClose={() => setHistoryId('')} onRestored={() => void run(async () => {})} />}
  </section>
}
