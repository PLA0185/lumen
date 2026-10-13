import { useEffect, useState } from 'react'
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
  const [form, setForm] = useState({ provider: 'github' as const, server: '', account: '', folder: 'Lumen', owner: '', repository: '', branch: 'main', password: '', inheritAll: true })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [connectionFeedback, setConnectionFeedback] = useState<ConnectionFeedback | null>(null)
  const [historyId, setHistoryId] = useState('')
  const [conflicts, setConflicts] = useState<cloud.BusinessConflict[]>([])
  const [defaults, setDefaults] = useState<cloud.SyncOptions>({ scope: 'all', direction: 'both' })
  useEffect(() => {
    let active = true
    const refresh = () => { void Promise.all([cloud.cloudStatus(), cloud.cloudBusinessConflicts()]).then(([s, c]) => { if (active) { setStatus(s); setConflicts(c) } }).catch(e => { if (active) setError(message(e)) }) }
    void cloud.cloudStatus().then(s => {
      if (active) {
        setDefaults(cloud.defaultSync(s.config))
        if (s.config) setForm(f => ({ ...f, provider: 'github', folder: s.config!.folder || 'Lumen', owner: s.config!.provider === 'github' ? s.config!.owner : '', repository: s.config!.provider === 'github' ? s.config!.repository : '', branch: s.config!.provider === 'github' ? s.config!.branch : 'main', inheritAll: s.config!.inheritAll }))
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
        ? ` 当前 GitHub 权限验证请求尚未发出，暂时无法判断 Token 是否有效。${status?.pending ? ` 当前有 ${status.pending} 项变更待上传。` : ''}`
        : ''
      if (!waiting) setError(detail)
      if (connectionMessages) setConnectionFeedback({ type: waiting ? 'waiting' : 'error', message: `${waiting ? '连接验证暂缓' : connectionMessages.failure}：${detail}${pendingText}` })
    } finally { setBusy(false) }
  }
  const updateForm = (patch: Partial<typeof form>) => { setForm(current => ({ ...current, ...patch })); setConnectionFeedback(null) }
  return <section className="setgroup">
    <h2>云同步 · GitHub 私有仓库</h2>
    <p className="setgroup__hint">使用 GitHub 私有仓库在多台电脑之间同步。所有电脑升级到此版本后，用同一个 GitHub Token 即可；内容仍会加密，不需要单独复制或填写 Lumen 密钥。</p>
    {status?.config?.provider === 'webDav' && <p className="alert alert--warn" role="status">检测到旧版云同步配置，此版本已停止该连接。请在下方配置 GitHub 私有仓库；本机数据仍保留，但旧云端内容不会自动搬到 GitHub。</p>}
    {((error || status?.lastError) && !connectionFeedback) && <p className="alert alert--error" role="alert">{error || status?.lastError}</p>}
    {notice && <p role="status">{notice}</p>}
    <fieldset disabled={busy} className="cloud-settings__fields">
      <div className="cloud-settings__fields-row">
        <label>GitHub 用户名或组织<input className="input" autoComplete="off" value={form.owner} onChange={e => updateForm({ owner: e.target.value })} placeholder="例如 PLA0185" /></label>
        <label>私有仓库名<input className="input" autoComplete="off" value={form.repository} onChange={e => updateForm({ repository: e.target.value })} placeholder="例如 lumen-sync" /></label>
      </div>
      <div className="cloud-settings__fields-row">
        <label>分支<input className="input" autoComplete="off" value={form.branch} onChange={e => updateForm({ branch: e.target.value })} placeholder="main" /></label>
        <label>仓库内同步目录<input className="input" value={form.folder} onChange={e => updateForm({ folder: e.target.value })} placeholder="Lumen" /></label>
      </div>
      <label>GitHub Fine-grained Token<input className="input" type="password" autoComplete="new-password" value={form.password} onChange={e => updateForm({ password: e.target.value })} placeholder="仅授予这个私有仓库 Contents 读写权限" /></label>
      <p className="setgroup__hint">请先在 GitHub 建立并初始化私有仓库（例如添加 README，确保所选分支存在），再创建只授权该仓库、Contents 读写权限的 Fine-grained Token。Token 加密保存在本机凭据库，不会同步；请在每台电脑使用同一个 Token。GitHub 提交历史可作为额外回溯记录；Lumen 的冲突版本仍会分别保留。</p>
      <p className="setgroup__hint">旧版同步空间会在这台已连接过的电脑下次同步时自动升级保护信息，不需要再找或复制 Lumen 密钥。</p>
      <label>这台电脑继承的内容<select className="input" value={String(form.inheritAll)} onChange={e => updateForm({ inheritAll: e.target.value === 'true' })}><option value="true">全部业务数据</option><option value="false">只继承备忘录和流程（含图片及文件）</option></select></label>
      <button className="btn btn--primary" aria-busy={connectionFeedback?.type === 'pending'} disabled={!form.password || !form.owner.trim() || !form.repository.trim() || !form.branch.trim()} onClick={() => void run(async () => { setStatus(await cloud.cloudConnect(form)); setForm(f => ({ ...f, password: '' })) }, { pending: '正在验证 GitHub 仓库权限并连接…', success: 'GitHub 私有仓库连接成功；其他电脑使用同一个 GitHub Token 即可加入。', failure: 'GitHub 连接失败' })}>{connectionFeedback?.type === 'pending' ? '正在验证并连接…' : '验证并连接 GitHub'}</button>
    </fieldset>
    {connectionFeedback && <div className={`cloud-settings__connect-feedback ${connectionFeedback.type === 'success' ? 'alert alert--ok' : connectionFeedback.type === 'error' ? 'alert alert--error' : connectionFeedback.type === 'waiting' ? 'alert alert--warn' : 'setgroup__hint'}`} role={connectionFeedback.type === 'error' ? 'alert' : 'status'} aria-live={connectionFeedback.type === 'error' ? 'assertive' : 'polite'}>{connectionFeedback.message}</div>}
    {status?.config?.provider === 'github' && <>
      <h3>默认同步类型</h3>
      <fieldset disabled={busy} className="cloud-settings__fields">
        <SyncOptionsFields prefix="默认" value={defaults} onChange={setDefaults} />
        <p className="setgroup__hint">保存后，自动同步和顶部同步入口都采用此选择。“全部业务数据”还会加密同步知识库原件与解析正文、食谱、每日饮食/训练记录和采购清单；联网搜索密钥仍只保存在各自电脑。选择“仅任务”或“仅备忘与流程”时不会同步这些内容。已保存流程继续按备忘与流程范围同步并参与检索。要共享知识库的电脑都需要运行支持此功能的版本。</p>
        {!status.config.defaultSync && !status.config.inheritAll && <p className="setgroup__hint">旧版继承范围只限制下载；保存默认类型后，上传与下载都会采用这里选择的内容范围。</p>}
        <button className="btn btn--primary" onClick={() => void run(async () => { setStatus(await cloud.cloudSetDefaults(defaults)); setNotice('默认同步类型已保存，自动同步及顶部入口将采用此选择。') })}>保存默认同步类型</button>
      </fieldset>
      <p role="status">自动同步开关：{status.config.enabled ? '已开启' : '已暂停'}<br />待上传云端：{status.pending ? `${status.pending} 项变更（尚未上传）` : '无待上传变更'}<br />最近成功上传：{status.lastUpload ? new Date(status.lastUpload).toLocaleString() : '尚未上传'} · 最近检查：{status.lastScan ? new Date(status.lastScan).toLocaleString() : '尚未检查'}</p>
      <p className="setgroup__hint">任务、项目、标签、重复规则、子任务、提醒、附件、专注、目标、食谱、饮食记录、训练消耗、采购清单、备忘和流程都可以按所选范围同步。知识库资料与饮食采购内容只属于“全部业务数据”范围。窗口、快捷键、AI 设置、联网搜索密钥和账号密码由各电脑单独设置。任务附件最多 200 MiB，内容中的图片/文件及知识库原件最多 20 MiB；大文件会消耗网盘流量。</p>
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
