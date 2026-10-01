import { useEffect, useState } from 'react'
import Markdown, { defaultUrlTransform } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'
import { save } from '@tauri-apps/plugin-dialog'
import { assetId, assetGet, assetExport, contentError, type ContentAsset } from '../lib/content-assets'

const schema = {
  ...defaultSchema,
  protocols: { ...defaultSchema.protocols, src: ['lumen-asset'], href: ['http', 'https', 'mailto', 'lumen-asset'] },
}
function Asset({ id, image, onRemove }: { id: string; image: boolean; onRemove?: () => void }) {
  const [asset, setAsset] = useState<ContentAsset | null>(null)
  const [error, setError] = useState('')
  const [url, setUrl] = useState('')
  useEffect(() => {
    let active = true
    let objectUrl = ''
    setAsset(null); setUrl(''); setError('')
    void assetGet(id).then((item) => {
      if (!active) return
      setAsset(item)
      if (image && ['image/png', 'image/jpeg', 'image/gif', 'image/webp'].includes(item.mime)) {
        const bytes = Uint8Array.from(atob(item.dataBase64), (c) => c.charCodeAt(0))
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: item.mime }))
        setUrl(objectUrl)
      }
    }).catch((e) => { if (active) setError(contentError(e)) })
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl) }
  }, [id, image])
  return <span className="content-asset">
    {error ? <span className="formerr" role="alert">{error}</span> : !asset ? <span>正在读取本地内容…</span> : url && <img src={url} alt={asset.name} className="content-asset__image" onError={() => setError('图片无法解码，请检查原文件')} />}
    <span className="content-asset__actions">
    {asset && <button type="button" className="btn btn--quiet btn--sm" onClick={() => {
      void (async () => {
        try { const path = await save({ defaultPath: asset.name }); if (path) await assetExport(id, path) }
        catch (e) { setError(contentError(e)) }
      })()
    }}>{asset.name} · {(asset.byteSize / 1024).toFixed(1)} KB · 另存为</button>}
    {onRemove && <button type="button" className="btn btn--ghost btn--sm" aria-label={`移除${image ? '图片' : '文件'}：${asset?.name ?? id}`} title="仅从当前内容移除，保留本地资源及其它记录" onClick={onRemove}>移除{image ? '图片' : '文件'}</button>}
    </span>
  </span>
}
export function ContentMarkdown({ children, onRemoveAsset }: { children: string; onRemoveAsset?: (id: string) => void }) {
  return <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[[rehypeSanitize, schema]]}
    urlTransform={(url) => assetId(url) ? url : defaultUrlTransform(url)}
    components={{
      img: ({ src, alt }) => assetId(src) ? <Asset id={assetId(src)!} image onRemove={onRemoveAsset ? () => onRemoveAsset(assetId(src)!) : undefined} /> : <span>{alt || '外部图片'}（请粘贴图片本身或拖入文件）</span>,
      a: ({ href, children: label }) => assetId(href) ? <Asset id={assetId(href)!} image={false} onRemove={onRemoveAsset ? () => onRemoveAsset(assetId(href)!) : undefined} /> : <a href={href} target="_blank" rel="noreferrer">{label}</a>,
    }}>{children}</Markdown>
}
