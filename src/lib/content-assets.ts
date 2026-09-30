import { invokeData } from './data-change'
import { IpcError } from './ipc'

export interface ContentAsset {
  id: string
  name: string
  mime: string
  dataBase64: string
  byteSize: number
  sha256: string
  createdAt: string
}

export const assetId = (url: string | undefined): string | null => {
  const match = /^lumen-asset:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i.exec(url ?? '')
  return match?.[1] ?? null
}
export const assetGet = (id: string): Promise<ContentAsset> => invokeData('content_asset_get', { id })
export const assetImportPath = (path: string): Promise<ContentAsset> => invokeData('content_asset_import_path', { path })
export const assetExport = (id: string, path: string): Promise<void> => invokeData('content_asset_export', { id, path })
export async function assetImportFile(file: File): Promise<ContentAsset> {
  if (file.size > 20 * 1024 * 1024) throw new Error('单个文件最多 20 MiB，请分拆大文件')
  const encoded = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(new Error('无法读取文件，请重新选择'))
    reader.onload = () => resolve(String(reader.result).split(',')[1] ?? '')
    reader.readAsDataURL(file)
  })
  return invokeData('content_asset_import', { name: file.name, dataBase64: encoded })
}
export function assetMarkdown(asset: ContentAsset): string {
  const name = asset.name.replace(/[[\]\\\r\n]/g, '_')
  return `${asset.mime.startsWith('image/') ? '!' : ''}[${name}](lumen-asset:${asset.id})`
}
/** Pasting media must not split an existing local resource's Markdown token. */
export function contentInsertionRange(value: string, start: number, end: number): { start: number; end: number } {
  let from = start, to = end
  for (const match of value.matchAll(/!?\[[^\]\n]*\]\(lumen-asset:[0-9a-f-]{36}\)/gi)) {
    const left = match.index, right = left + match[0].length
    if (start === end && start > left && start < right) return { start: right, end: right }
    if (start < right && end > left) { from = Math.min(from, left); to = Math.max(to, right) }
  }
  return { start: from, end: to }
}
export const contentError = (e: unknown): string => e instanceof IpcError ? e.userMessage() : e instanceof Error ? e.message : String(e)
