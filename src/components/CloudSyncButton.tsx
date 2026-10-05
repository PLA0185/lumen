import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import * as cloud from '../lib/cloud-sync-ipc'
import { IpcError } from '../lib/ipc'
import { SyncOptionsFields } from './SyncOptionsFields'

const message = (e: unknown) => e instanceof IpcError ? e.userMessage() : String(e)
export function CloudSyncButton() {
  const dialog = useRef<HTMLDialogElement>(null)
  const generation = useRef(0)
  const running = useRef(false)
  const [open, setOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<cloud.CloudStatus | null>(null)
  const [options, setOptions] = useState<cloud.SyncOptions>({ scope: 'all', direction: 'both' })
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  useEffect(() => { if (open) dialog.current?.showModal() }, [open])
  useEffect(() => () => { generation.current++ }, [])
  const show = async () => {
    const request = ++generation.current
    setOpen(true); setLoading(true); setStatus(null); setError(''); setNotice('')
    try {
      const value = await cloud.cloudStatus()
      if (request !== generation.current) return
      setStatus(value); setOptions(cloud.defaultSync(value.config))
    } catch (e) { if (request === generation.current) setError(message(e)) }
    finally { if (request === generation.current) setLoading(false) }
  }
  const sync = async () => {
    if (running.current || !status?.config) return
    running.current = true; setBusy(true); setError(''); setNotice('')
    const request = generation.current
    try {
      const value = await cloud.cloudNow(options)
      if (request !== generation.current) return
      setStatus(value)
      setNotice(`本次同步已执行：${cloud.SYNC_SCOPES[options.scope]} · ${cloud.SYNC_DIRECTIONS[options.direction]}。${value.pending ? `本机仍有 ${value.pending} 个版本等待上传（可能在未选范围内）。` : '本机没有待上传版本。'}`)
    } catch (e) { if (request === generation.current) setError(message(e)) }
    finally { running.current = false; if (request === generation.current) setBusy(false) }
  }
  const close = () => { if (!running.current) { generation.current++; setOpen(false) } }
  return <>
    <button type="button" className="btn btn--ghost btn--sm" aria-haspopup="dialog" onClick={() => void show()} disabled={busy}>同步</button>
    {open && createPortal(<dialog ref={dialog} className="sync-dialog" aria-label="选择同步内容和方向" onCancel={e => { if (running.current) e.preventDefault(); else close() }}>
      <h2>同步</h2>
      <p className="setgroup__hint">本次选择不会修改默认设置。默认内容与方向可在「设置 → 云同步」保存。</p>
      {loading && <p role="status">正在读取同步配置…</p>}
      {!loading && status && !status.config && <p className="alert alert--warn">请先在「设置 → 云同步」连接云空间。</p>}
      {status?.config && <>
        <fieldset disabled={busy}><SyncOptionsFields value={options} onChange={setOptions} prefix="本次" /></fieldset>
        {!status.config.enabled && <p className="setgroup__hint">自动同步已暂停；本次手动同步仍可执行，不会恢复自动同步。</p>}
        <p className="setgroup__hint">知识库原件和解析正文只在选择“全部业务数据”时加密同步；其他范围不会上传或下载知识库资料。流程仍按“备忘与流程”范围同步。</p>
        <p className="setgroup__hint">仅上传不下载；仅下载保留本地待上传内容。双向同步保留并发版本，遇到冲突会提示处理。</p>
      </>}
      {error && <p className="alert alert--error" role="alert">{error}</p>}
      {notice && <p className="alert alert--ok" role="status">{notice}</p>}
      <div className="sync-dialog__actions"><button className="btn btn--primary" disabled={busy || loading || !status?.config} onClick={() => void sync()}>{busy ? '同步中…' : '开始同步'}</button><button className="btn" disabled={busy} onClick={close}>关闭</button></div>
    </dialog>, document.body)}
  </>
}
