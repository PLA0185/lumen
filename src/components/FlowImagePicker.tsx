import { useState } from 'react'
import { contentImages } from '../lib/content-assets'
import { ContentMarkdown } from './ContentMarkdown'

export function FlowImagePicker({ source, detail, step, onChange }: { source: string; detail: string; step: number; onChange: (detail: string) => void }) {
  const [selectedId, setSelectedId] = useState('')
  const [preview, setPreview] = useState(false)
  const [error, setError] = useState('')
  const images = contentImages(source)
  if (!images.length) return null
  const attached = new Set(contentImages(detail).map(image => image.id))
  const available = images.filter(image => !attached.has(image.id))
  const selected = available.find(image => image.id === selectedId)
  if (!available.length) return null
  return <div className="flow-image-picker">
    <details><summary>从原始材料添加图片（{available.length} 张）</summary>
    <div className="flow-image-picker__thumbnails"><ContentMarkdown onSelectImage={id => { setSelectedId(id); setPreview(false); setError('') }} selectedImageId={selected?.id}>{available.map(image => image.markdown).join('\n\n')}</ContentMarkdown></div>
    <div className="flow-image-picker__controls">
      <button type="button" className="btn btn--ghost btn--sm" disabled={!selected} onClick={() => setPreview(!preview)}>预览选中图片</button>
      <button type="button" className="btn btn--ghost btn--sm" disabled={!selected} onClick={() => {
        if (!selected) return
        const value = `${detail.trimEnd()}\n\n${selected.markdown}`
        if (value.length > 5000) { setError('步骤说明含图片引用最多 5000 字，请缩短说明后再关联'); return }
        onChange(value); setSelectedId(''); setPreview(false); setError('')
      }} aria-label={`添加到第 ${step} 步`}>添加到此步骤</button>
    </div>
    {selected && preview && <div className="mdpreview"><ContentMarkdown>{selected.markdown}</ContentMarkdown></div>}
    </details>
    {error && <p className="formerr" role="alert">{error}</p>}
  </div>
}
