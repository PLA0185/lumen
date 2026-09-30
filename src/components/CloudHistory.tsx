import { useEffect, useState } from 'react'
import { cloudHistory, cloudRestore, type CloudHistory as History } from '../lib/cloud-sync-ipc'
import { ContentMarkdown } from './ContentMarkdown'
import { memoMarkdown, type MemoDocument } from '../lib/memos-ipc'
import { IpcError } from '../lib/ipc'
export function CloudHistory({ id, onClose, onRestored }: { id: string; onClose: () => void; onRestored: (doc: MemoDocument) => void }) {
  const [history, setHistory] = useState<History | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [choice, setChoice] = useState('')
  useEffect(() => { let active = true; void cloudHistory(id).then(h => { if (active) { setHistory(h); setChoice(h.heads[0] ?? '') } }).catch(e => { if (active) setError(e instanceof IpcError ? e.userMessage() : String(e)) }); return () => { active = false } }, [id])
  const chosen = history?.versions.find(v => v.id === choice)
  const restore = async (keepBoth: boolean) => {
    if (!history || !chosen || busy) return
    setBusy(true); setError('')
    try { const doc = await cloudRestore(id, choice, history.heads, keepBoth); onRestored(doc); onClose() }
    catch (e) { setError(e instanceof IpcError ? e.userMessage() : String(e)); try { setHistory(await cloudHistory(id)) } catch { /* Keep the original candidates and actionable error. */ } }
    finally { setBusy(false) }
  }
  return <div className="cloud-history" role="dialog" aria-modal="true" aria-label="备忘历史与冲突">
    <div className="cloud-history__panel">
      <div className="memos__toolbar"><h2>历史版本与冲突</h2><button className="btn" disabled={busy} onClick={onClose}>关闭历史</button></div>
      {error && <p role="alert" className="alert alert--error">{error}</p>}
      {history ? <>
        <p>{history.heads.length > 1 ? `有 ${history.heads.length} 个并发版本，尚未覆盖彼此。` : '恢复会保存为一个新版本，原来的历史仍保留。'}</p>
        <select className="input" aria-label="历史版本" value={choice} disabled={busy} onChange={e => setChoice(e.target.value)}>
          {history.versions.map(v => <option key={v.id} value={v.id}>{history.heads.includes(v.id) ? '当前候选 · ' : ''}{new Date(v.document.updatedAt).toLocaleString()} · {v.document.title}{v.document.deletedAt ? '（已删除）' : ''}</option>)}
        </select>
        {chosen && <ContentMarkdown>{memoMarkdown({ ...chosen.document, expectedRevision: chosen.document.revision })}</ContentMarkdown>}
        <div className="memos__toolbar"><button className="btn btn--primary" disabled={busy || !chosen} onClick={() => void restore(false)}>采用这个版本</button>
          {history.heads.length > 1 && <button className="btn" disabled={busy || !chosen} onClick={() => void restore(true)}>保留全部版本为独立备忘</button>}</div>
      </> : <p>读取历史中…</p>}
    </div>
  </div>
}
