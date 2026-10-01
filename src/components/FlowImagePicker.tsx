import { useState } from 'react'
import { contentImages } from '../lib/content-assets'
import { ContentMarkdown } from './ContentMarkdown'

export function FlowImagePicker({ source, detail, step, onChange }: { source: string; detail: string; step: number; onChange: (detail: string) => void }) {
  const [selectedId, setSelectedId] = useState('')
  const [error, setError] = useState('')
  const images = contentImages(source)
  if (!images.length) return null
  const attached = new Set(contentImages(detail).map(image => image.id))
  const available = images.filter(image => !attached.has(image.id))
  const selected = available.find(image => image.id === selectedId)
  return <div className="flow-image-picker">
    <div className="flow-image-picker__controls">
      <select className="input" aria-label={`第 ${step} 步关联原图`} value={selected?.id ?? ''} onChange={e => { setSelectedId(e.target.value); setError('') }}>
        <option value="">{available.length ? '选择已有原图，预览后放到此步骤' : '此步骤已关联全部原图'}</option>
        {available.map(image => <option key={image.id} value={image.id}>{image.name}</option>)}
      </select>
      <button type="button" className="btn btn--ghost btn--sm" disabled={!selected} onClick={() => {
        if (!selected) return
        const value = `${detail.trimEnd()}\n\n${selected.markdown}`
        if (value.length > 5000) { setError('步骤说明含图片引用最多 5000 字，请缩短说明后再关联'); return }
        onChange(value); setSelectedId(''); setError('')
      }}>放到此步骤</button>
    </div>
    {selected && <div className="mdpreview"><ContentMarkdown>{selected.markdown}</ContentMarkdown></div>}
    {error && <p className="formerr" role="alert">{error}</p>}
  </div>
}
