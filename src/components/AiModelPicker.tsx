import { useEffect, useRef, useState } from 'react'
import * as ai from '../lib/ai-ipc'
import { IpcError } from '../lib/ipc'

export function AiModelPicker({ config, disabled, onSaved }: { config: ai.ProviderConfig; disabled?: boolean; onSaved: () => Promise<void> }) {
  const [open, setOpen] = useState(false)
  const [models, setModels] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const generation = useRef(0)
  useEffect(() => { const requests = generation; requests.current++; setModels([]); setOpen(false); setBusy(false); setError(''); return () => { requests.current++ } }, [config.provider, config.baseUrl])
  const load = async () => {
    const request = ++generation.current
    setOpen(true); setBusy(true); setError('')
    try {
      const result = await ai.aiListModels(config)
      if (request !== generation.current) return
      setModels(result.models)
      if (!result.ok || !result.models.length) setError(result.reason ?? '服务商没有返回可用模型，请在配置 AI 中手动填写模型 ID。')
    } catch (e) { if (request === generation.current) setError(e instanceof IpcError ? e.userMessage() : String(e)) }
    finally { if (request === generation.current) setBusy(false) }
  }
  const choose = async (model: string) => {
    const request = ++generation.current
    setBusy(true); setError('')
    try {
      const latest = await ai.aiGetConfig()
      if (request !== generation.current) return
      if (!latest || latest.provider !== config.provider || latest.baseUrl !== config.baseUrl) throw new Error('AI 服务配置已改变，请刷新后重新选择模型。')
      await ai.aiSetConfig({ ...latest, model }, null)
      await onSaved()
    } catch (e) { if (request === generation.current) setError(e instanceof IpcError ? e.userMessage() : String(e)) }
    finally { if (request === generation.current) setBusy(false) }
  }
  return <div className="ai-model-picker">
    <span className="setgroup__hint">当前模型：{config.model || '未选择'}</span>{' '}
    <button type="button" className="btn btn--quiet btn--sm" disabled={disabled || busy} onClick={() => void load()}>{busy ? '读取中…' : '选择模型'}</button>
    {open && models.length > 0 && <select className="input" aria-label="助手使用的模型" disabled={disabled || busy} value={models.includes(config.model) ? config.model : ''} onChange={e => void choose(e.target.value)}>
      <option value="" disabled>选择模型（选择后保存并生效）</option>
      {models.map(model => <option key={model} value={model}>{model}</option>)}
    </select>}
    {error && <p className="alert alert--error" role="alert">{error}</p>}
  </div>
}
