import { createContext, useContext, useEffect, useRef, useState } from 'react'
import Markdown, { defaultUrlTransform, type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'
import { save } from '@tauri-apps/plugin-dialog'
import { assetId, assetGet, assetExport, contentError, type ContentAsset } from '../lib/content-assets'
import { ImageViewer } from './ImageViewer'
import { rehypeNoOrphans } from '../lib/no-orphan'
import { SafeText } from './SafeText'

const schema = {
  ...defaultSchema,
  protocols: { ...defaultSchema.protocols, src: ['lumen-asset'], href: ['http', 'https', 'mailto', 'lumen-asset'] },
}
type AssetActions = { onRemoveAsset?: (id: string) => void; onExtractAsset?: (asset: ContentAsset) => void; onUpdateImage?: (id: string, replacement: string | null) => void | Promise<void>; onSelectImage?: (id: string) => void; selectedImageId?: string }
const AssetActionsContext = createContext<AssetActions>({})
type LoadedAsset = { asset: ContentAsset; url: string }
type CachedAsset = { loaded?: LoadedAsset; pending: Promise<LoadedAsset> }
const AssetCacheContext = createContext(new Map<string, CachedAsset>())
function Asset({ id, image, caption = '' }: { id: string; image: boolean; caption?: string }) {
  const { onRemoveAsset: onRemove, onExtractAsset: onExtract, onUpdateImage, onSelectImage, selectedImageId } = useContext(AssetActionsContext)
  const latestUpdate = useRef(onUpdateImage); latestUpdate.current = onUpdateImage
  const [viewer, setViewer] = useState(false)
  const cache = useContext(AssetCacheContext)
  const [asset, setAsset] = useState<ContentAsset | null>(cache.get(id)?.loaded?.asset ?? null)
  const [error, setError] = useState('')
  const [url, setUrl] = useState(image ? cache.get(id)?.loaded?.url ?? '' : '')
  useEffect(() => {
    let active = true
    setError('')
    let entry = cache.get(id)
    if (!entry) {
      entry = { pending: assetGet(id).then(item => {
        let objectUrl = ''
        if (cache.get(id) === entry && ['image/png', 'image/jpeg', 'image/gif', 'image/webp'].includes(item.mime)) {
        const bytes = Uint8Array.from(atob(item.dataBase64), (c) => c.charCodeAt(0))
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: item.mime }))
        }
        const loaded = { asset: item, url: objectUrl }
        entry!.loaded = loaded
        return loaded
      }) }
      cache.set(id, entry)
    }
    void entry.pending.then((loaded) => {
      if (!active) return
      setAsset(loaded.asset); setUrl(image ? loaded.url : '')
    }).catch((e) => { if (active) setError(contentError(e)) })
    return () => { active = false }
  }, [id, image, cache])
  return <span className="content-asset">
    {error ? <span className="formerr" role="alert">{error}</span> : !asset ? <span>正在读取本地内容…</span> : url && <img src={url} alt={onSelectImage ? '原图缩略图' : asset.name} className="content-asset__image" role="button" tabIndex={0} aria-pressed={onSelectImage ? selectedImageId === id : undefined} aria-label={onSelectImage ? '选择原图' : caption ? `查看图片：${caption}` : '查看图片'} onClick={e => { e.stopPropagation(); if (onSelectImage) onSelectImage(id); else setViewer(true) }} onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); e.stopPropagation(); if (onSelectImage) onSelectImage(id); else setViewer(true) } }} onError={() => setError('图片无法解码，请检查原文件')} />}
    {image && caption && !onSelectImage && <span className="content-asset__caption"><SafeText>{caption}</SafeText></span>}
    {!onSelectImage && <span className="content-asset__actions">
    {asset && <button type="button" className="btn btn--quiet btn--sm" onClick={() => {
      void (async () => {
        try { const path = await save({ defaultPath: asset.name }); if (path) await assetExport(id, path) }
        catch (e) { setError(contentError(e)) }
      })()
    }}>{image ? '另存为' : `${asset.name} · ${(asset.byteSize / 1024).toFixed(1)} KB · 另存为`}</button>}
    {asset && onExtract && <button type="button" className="btn btn--quiet btn--sm" aria-label={`识别内容：${asset.name}`} onClick={() => onExtract(asset)}>识别内容</button>}
    {onRemove && <button type="button" className="btn btn--ghost btn--sm" aria-label={`移除${image ? '图片' : '文件'}：${asset?.name ?? id}`} title="仅从当前内容移除，保留本地资源及其它记录" onClick={() => onRemove(id)}>移除{image ? '图片' : '文件'}</button>}
    </span>}
    {viewer && asset && url && <ImageViewer asset={asset} url={url} caption={caption} onClose={() => setViewer(false)} onUpdate={onUpdateImage ? replacement => {
      if (!latestUpdate.current) throw new Error('当前图片不可编辑')
      return latestUpdate.current(id, replacement)
    } : undefined} />}
  </span>
}
// Stable component types keep resource state and object URLs alive while text changes.
const components: Components = {
  img: ({ src, alt, title }) => assetId(src) ? <Asset key={assetId(src)} id={assetId(src)!} image caption={title ?? ''} /> : <span>{alt || '外部图片'}（请粘贴图片本身或拖入文件）</span>,
  a: ({ href, children: label }) => assetId(href) ? <Asset key={assetId(href)} id={assetId(href)!} image={false} /> : <a href={href} target="_blank" rel="noreferrer">{label}</a>,
}
export function ContentMarkdown({ children, onRemoveAsset, onExtractAsset, onUpdateImage, onSelectImage, selectedImageId }: { children: string } & AssetActions) {
  const cache = useRef(new Map<string, CachedAsset>()).current
  useEffect(() => {
    const referenced = new Set([...children.matchAll(/lumen-asset:([0-9a-f-]{36})/gi)].map(match => match[1]))
    for (const [id, entry] of cache) if (!referenced.has(id)) {
      if (entry.loaded?.url) URL.revokeObjectURL(entry.loaded.url)
      cache.delete(id)
    }
  }, [children, cache])
  useEffect(() => () => {
    for (const entry of cache.values()) if (entry.loaded?.url) URL.revokeObjectURL(entry.loaded.url)
    cache.clear()
  }, [cache])
  return <AssetCacheContext.Provider value={cache}><AssetActionsContext.Provider value={{ onRemoveAsset, onExtractAsset, onUpdateImage, onSelectImage, selectedImageId }}><Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[[rehypeSanitize, schema], rehypeNoOrphans]}
    urlTransform={(url) => assetId(url) ? url : defaultUrlTransform(url)}
    components={components}>{children}</Markdown></AssetActionsContext.Provider></AssetCacheContext.Provider>
}
