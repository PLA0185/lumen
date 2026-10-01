import { useEffect, useRef, useState, type ComponentProps } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { readImage, readText } from '@tauri-apps/plugin-clipboard-manager'
import { assetImportFile, assetImportPath, assetExtract, assetMarkdown, contentInsertionRange, contentError, type ContentAsset } from '../lib/content-assets'
import { ContentMarkdown } from './ContentMarkdown'

type Props = ComponentProps<'textarea'> & { onBusyChange?: (busy: boolean) => void; extractFiles?: boolean }
export function ContentEditor({ onBusyChange, extractFiles = false, ...props }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const mounted = useRef(true)
  const pending = useRef(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [preview, setPreview] = useState(false)
  const [recognize, setRecognize] = useState(extractFiles)
  const [importResults, setImportResults] = useState('')
  useEffect(() => {
    onBusyChange?.(busy)
    return () => onBusyChange?.(false)
  }, [busy, onBusyChange])
  const latest = useRef(props)
  latest.current = props
  const images = String(props.value ?? '').match(/!\[[^\]]*\]\(lumen-asset:[^)]+\)/g) ?? []
  const removeAsset = (id: string) => {
    const field = ref.current
    if (!field || field.disabled || field.readOnly || pending.current) return
    const value = field.value.replace(/!?\[[^\]\n]*\]\(lumen-asset:([0-9a-f-]{36})\)/gi, (token, reference: string) => reference.toLowerCase() === id.toLowerCase() ? '' : token)
    field.focus()
    field.setSelectionRange(0, field.value.length)
    if (document.execCommand?.('insertText', false, value)) return
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set?.call(field, value)
    field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }))
  }
  const onRemoveAsset = props.disabled || props.readOnly || busy ? undefined : removeAsset

  const insert = async (load: () => Promise<string>) => {
    const field = ref.current
    if (!field || pending.current || field.disabled || field.readOnly) return
    const value = field.value, start = field.selectionStart, end = field.selectionEnd
    const focused = document.activeElement
    pending.current = true; setBusy(true); setError(''); setImportResults('')
    try {
      const text = await load()
      if (!mounted.current || !field.isConnected) return
      if (field.value !== value || field.selectionStart !== start || field.selectionEnd !== end || document.activeElement !== focused) throw new Error('导入期间内容或编辑位置发生变化，请在需要的位置重新粘贴')
      if (!text) return
      const range = text.includes('lumen-asset:') ? contentInsertionRange(value, start, end) : { start, end }
      if (field.maxLength >= 0 && value.length - (range.end - range.start) + text.length > field.maxLength) throw new Error(`内容超过 ${field.maxLength} 字，请缩短后重新导入`)
      // Controlled textarea updates through its native editing/input path.
      field.focus()
      field.setSelectionRange(range.start, range.end)
      if (document.execCommand?.('insertText', false, text)) return
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set?.call(field, value.slice(0, range.start) + text + value.slice(range.end))
      field.setSelectionRange(range.start + text.length, range.start + text.length)
      field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste', data: text }))
    } catch (e) { if (mounted.current) setError(contentError(e)) }
    finally { pending.current = false; if (mounted.current) setBusy(false) }
  }
  const importMany = async (loads: Array<() => Promise<ContentAsset>>) => {
    const parts: string[] = []
    const errors: string[] = []
    const overflow: string[] = []
    const field = ref.current
    const remaining = field && field.maxLength >= 0 ? field.maxLength - field.value.length + field.selectionEnd - field.selectionStart : Infinity
    const fits = (part: string) => `\n${[...parts, part].join('\n')}\n`.length <= remaining
    const addReference = (asset: ContentAsset) => {
      const reference = assetMarkdown(asset)
      if (fits(reference)) parts.push(reference)
      else overflow.push(reference)
    }
    // Keep successful files visible even when a later file fails.
    for (const load of loads) {
      try {
        const asset = await load()
        addReference(asset)
        if (extractFiles && recognize) {
          try {
            const extracted = await assetExtract(asset.id)
            const recognized = [extracted.text.trim() ? `\n### ${asset.name.replace(/[\r\n]/g, '_')} · 识别内容\n\n${extracted.text}` : '', ...extracted.images.map(assetMarkdown)].filter(Boolean).join('\n')
            if (!fits(recognized) && recognized) {
              const complete = await assetImportFile(new File([recognized], `${asset.name.slice(0, 180)}-识别内容.md`, { type: 'text/plain' }))
              addReference(complete)
              errors.push('识别正文超过当前位置剩余字数，完整识别内容已保存为文件；原文件也保留，可另存后分段编辑。')
            } else if (recognized) parts.push(recognized)
            errors.push(...extracted.warnings)
          } catch (e) { errors.push(`${asset.name}：${contentError(e)}。原文件已保留。`) }
        }
      }
      catch (e) { errors.push(contentError(e)) }
    }
    if (overflow.length) {
      setImportResults(overflow.join('\n'))
      errors.push('当前位置字数不足，部分文件未放进当前内容；可从下方导入结果查看或另存。')
    }
    if (errors.length) setError(errors.join('\n'))
    return parts.length ? `\n${parts.join('\n')}\n` : ''
  }
  const nativePaste = () => insert(async () => {
    let clipboardImage
    try { clipboardImage = await readImage() } catch { return readText() }
    try {
      const { width, height } = await clipboardImage.size()
      if (width * height > 25_000_000) throw new Error('剪贴板图片过大，请先缩小后再粘贴')
      const rgba = await clipboardImage.rgba()
      const canvas = document.createElement('canvas')
      canvas.width = width; canvas.height = height
      const ctx = canvas.getContext('2d')
      if (!ctx) throw new Error('无法读取剪贴板图片')
      ctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0)
      const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((b) => b ? resolve(b) : reject(new Error('图片编码失败')), 'image/png'))
      return importMany([() => assetImportFile(new File([blob], `粘贴图片-${Date.now()}.png`, { type: 'image/png' }))])
    } finally { await clipboardImage.close() }
  })
  const current = useRef({ insert, importMany })
  current.current = { insert, importMany }
  useEffect(() => {
    mounted.current = true
    if (!('__TAURI_INTERNALS__' in window)) return () => { mounted.current = false }
    let disposed = false, off: (() => void) | undefined
    void getCurrentWindow().onDragDropEvent(({ payload }) => {
      if (payload.type !== 'drop' || latest.current.disabled || latest.current.readOnly) return
      const target = document.elementFromPoint(payload.position.x / window.devicePixelRatio, payload.position.y / window.devicePixelRatio)
      if (!target || !ref.current?.parentElement?.contains(target)) return
      void current.current.insert(() => current.current.importMany(payload.paths.map((path) => () => assetImportPath(path))))
    }).then((stop) => { if (disposed) stop(); else off = stop })
      .catch((e) => { if (!disposed) setError(`文件拖入监听失败：${contentError(e)}`) })
    return () => { disposed = true; mounted.current = false; off?.() }
  }, [])
  return <div className="content-editor">
    <textarea {...props} ref={ref} data-content-editor="true" aria-busy={busy}
      onKeyDown={(e) => {
        props.onKeyDown?.(e)
        if (!e.defaultPrevented && !e.altKey && !e.nativeEvent.isComposing && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v') {
          e.preventDefault(); if (!e.repeat) void nativePaste()
        }
      }}
      onPaste={(e) => {
        props.onPaste?.(e)
        if (e.defaultPrevented) return
        e.preventDefault()
        const files = [...e.clipboardData.files]
        if (files.length) void insert(() => importMany(files.map((file) => () => assetImportFile(file))))
        else void nativePaste()
      }}
      onDragOver={(e) => { if (e.dataTransfer.types.includes('Files')) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy' } }}
      onDrop={(e) => {
        if (!e.dataTransfer.files.length) return
        e.preventDefault(); e.stopPropagation()
        const files = [...e.dataTransfer.files]
        void insert(() => importMany(files.map((file) => () => assetImportFile(file))))
      }} />
    <div className="content-editor__tools">
      <label className="btn btn--quiet btn--sm">{busy ? '正在导入…' : '添加图片 / 文件'}<input className="sr-only" type="file" multiple disabled={props.disabled || props.readOnly || busy} onChange={(e) => {
        const files = [...(e.target.files ?? [])]; e.target.value = ''
        if (files.length) void insert(() => importMany(files.map((file) => () => assetImportFile(file))))
      }} /></label>
      <button type="button" className="btn btn--quiet btn--sm" aria-expanded={preview} onClick={() => setPreview(!preview)}>{preview ? '收起预览' : '预览内容'}</button>
      {extractFiles && <label className="setgroup__hint"><input type="checkbox" checked={recognize} disabled={props.disabled || props.readOnly || busy} onChange={e => setRecognize(e.target.checked)} /> 导入时在本机识别内容</label>}
      <span className="setgroup__hint">可粘贴图片、拖入文件，单文件最多 20 MiB</span>
    </div>
    {error && <p className="formerr" role="alert">{error}</p>}
    {importResults && <div className="mdpreview" aria-label="本次导入结果"><p className="setgroup__hint">本次导入结果（尚未放进当前内容）</p><ContentMarkdown>{importResults}</ContentMarkdown></div>}
    {!preview && images.length > 0 && <div className="mdpreview" aria-label="已插入图片预览"><ContentMarkdown onRemoveAsset={onRemoveAsset}>{images.join('\n')}</ContentMarkdown></div>}
    {preview && <div className="mdpreview"><ContentMarkdown onRemoveAsset={onRemoveAsset}>{String(props.value ?? '')}</ContentMarkdown></div>}
  </div>
}
