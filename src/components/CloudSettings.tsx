import { useEffect, useState } from 'react'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import * as cloud from '../lib/cloud-sync-ipc'
import { IpcError } from '../lib/ipc'
import { CloudHistory } from './CloudHistory'
import { SyncOptionsFields } from './SyncOptionsFields'
const labels: Record<string, string> = { tasks: '任务', subtasks: '子任务', projects: '项目', categories: '分类', tags: '标签', reminders: '提醒', attachments: '附件', task_series: '重复任务', task_series_template: '重复子任务', task_series_segments: '重复规则', task_series_skips: '跳过记录', task_series_rebuilds: '重建记录', task_series_tags: '重复标签', task_tags: '任务标签', task_dependencies: '依赖', focus_sessions: '专注记录', goals: '目标', settings: '业务设置', knowledge_sources: '知识库资料' }
const message = (e: unknown) => e instanceof IpcError ? e.userMessage() : e instanceof Error ? e.message : String(e)
type ConnectionFeedback = { type: 'pending' | 'success' | 'error' | 'waiting'; message: string }
const fieldLabels: Record<string,string> = { title:'标题',name:'名称',description:'说明',note_md:'备注',body_md:'正文',rrule:'重复规则',tzid:'时区',status:'状态',is_done:'完成状态',planned_at:'计划时间',due_at:'截止时间',completed_at:'完成时间',file_name:'文件名',size_bytes:'文件字节数',priority:'优先级',estimated_minutes:'预计分钟',actual_minutes:'实际分钟',created_at:'创建时间',updated_at:'修改时间' }
function versionText(row: Record<string, unknown> | null): string {
  if (!row) return '这个版本删除了记录。'
  const states: Record<string,string> = { todo:'待办',doing:'进行中',waiting:'等待',done:'已完成',archived:'已归档' }
  return Object.entries(row).filter(([key,value]) => key in fieldLabels && value != null && value !== '').map(([key,value]) => `${fieldLabels[key]}：${key === 'status' ? states[String(value)] ?? String(value) : key === 'is_done' ? Number(value) ? '已完成' : '未完成' : String(value)}`).join('\n') || '这是归属或关联记录的一个版本；原来的关联仍保留在历史中。'
}
export function CloudSettings() {
  const [status, setStatus] = useState<cloud.CloudStatus | null>(null)
  const [form, setForm] = useState({ server: 'https://dav.jianguoyun.com/dav/', account: '', folder: 'Lumen', password: '', recoveryCode: '', inheritAll: true })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [connectionFeedback, setConnectionFeedback] = useState<ConnectionFeedback | null>(null)
  const [syncKey, setSyncKey] = useState('')
  const [showSyncKey, setShowSyncKey] = useState(false)
  const [historyId, setHistoryId] = useState('')
  const [conflicts, setConflicts] = useState<cloud.BusinessConflict[]>([])
  const [defaults, setDefaults] = useState<cloud.SyncOptions>({ scope: 'all', direction: 'both' })
  useEffect(() => {
    let active = true
    const refresh = () => { void Promise.all([cloud.cloudStatus(), cloud.cloudBusinessConflicts()]).then(([s, c]) => { if (active) { setStatus(s); setConflicts(c) } }).catch(e => { if (active) setError(message(e)) }) }
    void cloud.cloudStatus().then(async s => {
      const key = s.config ? await cloud.cloudRecoveryCode() : ''
      if (active) {
        setDefaults(cloud.defaultSync(s.config))
        if (s.config) setForm(f => ({ ...f, server: s.config!.server, account: s.config!.account, folder: s.config!.folder, inheritAll: s.config!.inheritAll }))
        setSyncKey(key)
      }
    }).catch(e => { if (active) setError(message(e)) })
    refresh(); const timer = setInterval(refresh, 3000)
    return () => { active = false; clearInterval(timer) }
  }, [])
  const run = async (action: () => Promise<void>, connectionMessages?: { pending: string; success: string; failure: string }) => {
    setBusy(true); setError(''); setNotice('')
    setConnectionFeedback(connectionMessages ? { type: 'pending', message: connectionMessages.pending } : null)
    try {
      await action()
      setStatus(await cloud.cloudStatus())
      setConflicts(await cloud.cloudBusinessConflicts())
      if (connectionMessages) setConnectionFeedback({ type: 'success', message: connectionMessages.success })
    } catch (e) {
      const detail = message(e)
      const waiting = connectionMessages && e instanceof IpcError && e.code === 'rate_limited'
      const pendingText = waiting
        ? ` 当前没有账号验证结果，无法据此判断账号或第三方应用密码是否正确。${status?.pending ? ` 当前有 ${status.pending} 项变更待上传。` : ''}`
        : ''
      if (!waiting) setError(detail)
      if (connectionMessages) setConnectionFeedback({ type: waiting ? 'waiting' : 'error', message: `${waiting ? '连接验证暂缓' : connectionMessages.failure}：${detail}${pendingText}` })
    } finally { setBusy(false) }
  }
  const updateForm = (patch: Partial<typeof form>) => { setForm(current => ({ ...current, ...patch })); setConnectionFeedback(null) }
  return <section className="setgroup">
    <h2>云同步 · 坚果云 / WebDAV</h2>
    <p className="setgroup__hint">修改先保存在本机，再按默认内容和方向同步。顶部“同步”可单独选择本次类型。无需运行坚果云客户端，云端内容加密保存。</p>
    {((error || status?.lastError) && !connectionFeedback) && <p className="alert alert--error" role="alert">{error || status?.lastError}</p>}
    {notice && <p role="status">{notice}</p>}
    {status?.config ? <div className="cloud-sync-key">
      <h3>连接其他电脑</h3>
      <p className="setgroup__hint">把这把 Lumen 同步密钥复制到另一台电脑的云同步设置中。坚果云不保存此密钥。</p>
      <div className="cloud-sync-key__row">
        <input className="input" aria-label="Lumen 同步密钥" type={showSyncKey ? 'text' : 'password'} readOnly value={syncKey} placeholder="正在读取本机密钥…" />
        <button className="btn" disabled={!syncKey} onClick={() => setShowSyncKey(value => !value)}>{showSyncKey ? '隐藏密钥' : '显示密钥'}</button>
        <button className="btn" disabled={busy || !syncKey} onClick={() => void run(async () => { await writeText(syncKey); setNotice('Lumen 同步密钥已复制。请安全保存；在另一台电脑加入此空间时粘贴。坚果云不会保存或重置这个密钥。') })}>复制 Lumen 同步密钥</button>
      </div>
    </div> : <p className="setgroup__hint">尚未连接，首次连接成功后会生成同步密钥；连接成功后可在这里查看或复制。</p>}
    <fieldset disabled={busy} className="cloud-settings__fields">
      <label>服务器地址<input className="input" value={form.server} onChange={e => updateForm({ server: e.target.value })} /></label>
      <label>坚果云账号<input className="input" autoComplete="username" value={form.account} onChange={e => updateForm({ account: e.target.value })} /></label>
      <label>同步文件夹<input className="input" value={form.folder} onChange={e => updateForm({ folder: e.target.value })} /></label>
      <label>第三方应用密码<input className="input" type="password" autoComplete="new-password" value={form.password} onChange={e => updateForm({ password: e.target.value })} /></label>
      <label>已有 Lumen 云空间的同步密钥<input className="input" type="password" autoComplete="off" placeholder="首次创建空间留空；另一台电脑加入时粘贴" value={form.recoveryCode} onChange={e => updateForm({ recoveryCode: e.target.value })} /></label>
      <p className="setgroup__hint">这是 Lumen 为云端数据加密生成的密钥，不是坚果云提供的密码。首次创建空间时留空；连接已有空间时，从已连接的电脑复制并粘贴。坚果云无法重置此密钥。</p>
      <label>这台电脑继承的内容<select className="input" value={String(form.inheritAll)} onChange={e => updateForm({ inheritAll: e.target.value === 'true' })}><option value="true">全部业务数据</option><option value="false">只继承备忘录和流程（含图片及文件）</option></select></label>
      <button className="btn btn--primary" aria-busy={connectionFeedback?.type === 'pending'} disabled={!form.account.trim() || !form.password} onClick={() => void run(async () => { setStatus(await cloud.cloudConnect(form)); const key = await cloud.cloudRecoveryCode(); setSyncKey(key); setShowSyncKey(true); setForm(f => ({ ...f, password: '', recoveryCode: '' })) }, { pending: '正在验证服务器、账号和云空间，请稍候…', success: '云空间连接成功。同步密钥已显示，自动同步会按默认类型开始运行。', failure: '云空间连接失败' })}>{connectionFeedback?.type === 'pending' ? '正在验证并连接…' : '验证并连接云空间'}</button>
    </fieldset>
    {connectionFeedback && <div className={`cloud-settings__connect-feedback ${connectionFeedback.type === 'success' ? 'alert alert--ok' : connectionFeedback.type === 'error' ? 'alert alert--error' : connectionFeedback.type === 'waiting' ? 'alert alert--warn' : 'setgroup__hint'}`} role={connectionFeedback.type === 'error' ? 'alert' : 'status'} aria-live={connectionFeedback.type === 'error' ? 'assertive' : 'polite'}>{connectionFeedback.message}</div>}
    {status?.config && <>
      <h3>默认同步类型</h3>
      <fieldset disabled={busy} className="cloud-settings__fields">
        <SyncOptionsFields prefix="默认" value={defaults} onChange={setDefaults} />
        <p className="setgroup__hint">保存后，自动同步和顶部同步入口都采用此选择。“全部业务数据”还会加密同步知识库原件与解析正文，并在接收电脑重建本机检索索引；“仅任务”及“仅备忘与流程”不包含知识库资料。已保存流程继续按备忘与流程范围同步并参与检索。要共享知识库的电脑都需要运行支持此功能的版本。</p>
        {!status.config.defaultSync && !status.config.inheritAll && <p className="setgroup__hint">旧版继承范围只限制下载；保存默认类型后，上传与下载都会采用这里选择的内容范围。</p>}
        <button className="btn btn--primary" onClick={() => void run(async () => { setStatus(await cloud.cloudSetDefaults(defaults)); setNotice('默认同步类型已保存，自动同步及顶部入口将采用此选择。') })}>保存默认同步类型</button>
      </fieldset>
      <p role="status">自动同步开关：{status.config.enabled ? '已开启' : '已暂停'}<br />待上传云端：{status.pending ? `${status.pending} 项变更（尚未上传）` : '无待上传变更'}<br />最近成功上传：{status.lastUpload ? new Date(status.lastUpload).toLocaleString() : '尚未上传'} · 最近检查：{status.lastScan ? new Date(status.lastScan).toLocaleString() : '尚未检查'}</p>
      <p className="setgroup__hint">任务、项目、标签、重复规则、子任务、提醒、附件、专注、目标、备忘和流程都可以按所选范围同步。知识库资料只属于“全部业务数据”范围。窗口、快捷键和账号密码由各电脑单独设置。任务附件最多 200 MiB，内容中的图片/文件及知识库原件最多 20 MiB；大文件会消耗网盘流量。</p>
      <div className="memos__toolbar">
        <button className="btn" disabled={busy} onClick={() => void run(async () => { setStatus(await cloud.cloudNow(cloud.defaultSync(status.config))); setNotice('本次同步已执行，采用已保存的默认类型。') })}>立即同步</button>
        <button className="btn" disabled={busy} onClick={() => void run(async () => { setStatus(await cloud.cloudEnable(!status.config!.enabled)) })}>{status.config.enabled ? '暂停同步' : '恢复同步'}</button>
      </div>
      {status.conflicts.map(id => <button key={id} className="btn" onClick={() => setHistoryId(id)}>查看备忘/流程冲突</button>)}
      {conflicts.map(c => <div key={c.id} className="setgroup">
        <h3>{labels[c.table] ?? '业务记录'}需要选择版本</h3>
        <p className="setgroup__hint">两边内容都已保留。采用一个版本后，另一个仍在历史中。</p>
        {c.versions.map(v => <div key={v.id}>
          <p>{c.heads.includes(v.id) ? '当前候选 · ' : '历史版本 · '}{new Date(v.createdAt).toLocaleString()} · {v.row ? String(v.row.title ?? v.row.name ?? v.row.file_name ?? '记录') : '删除版本'}</p>
          <details><summary>查看这个版本的详细内容</summary><pre className="selectable cloud-settings__version">{versionText(v.row)}</pre></details>
          <button className="btn" disabled={busy} onClick={() => void run(async () => { await cloud.cloudBusinessResolve(c.id, v.id, c.heads) })}>采用这个版本</button>
        </div>)}
      </div>)}
    </>}
    {historyId && <CloudHistory id={historyId} onClose={() => setHistoryId('')} onRestored={() => void run(async () => {})} />}
  </section>
}
