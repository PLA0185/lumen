import { listen } from '@tauri-apps/api/event'
import { invokeData } from './data-change'

export interface KnowledgeSourceSummary {
  id: string
  assetId: string
  title: string
  mime: string
  byteSize: number
  sha256: string
  status: 'ready' | 'unreadable'
  warnings: string[]
  error: string | null
  createdAt: string
  updatedAt: string
}

export interface KnowledgeCitation {
  id: string
  sourceKind: 'document' | 'flow'
  sourceId: string
  title: string
  category: string | null
  assetId: string | null
  flowId: string | null
  stepId: string | null
  revision: number | null
  contentHash: string | null
  locator: string
  excerpt: string
  startOffset: number | null
}

export interface KnowledgeSearchResult { citations: KnowledgeCitation[] }
export interface KnowledgeFlowCandidate {
  flowId: string
  title: string
  category: string
  evidence: string
}
export interface KnowledgeAskResult {
  status: 'answered' | 'notFound' | 'clarify'
  answer: string | null
  searchTerms: string[]
  citations: KnowledgeCitation[]
  flowCandidates: KnowledgeFlowCandidate[]
  message: string | null
  providerNotice: string
}
export interface KnowledgeHistoryEntry { role: 'user' | 'assistant'; text: string }
export interface KnowledgeImportProgress {
  phase: 'reading' | 'parsing' | 'ocr' | 'indexing' | 'saving' | 'complete'
  message: string
  current: number | null
  total: number | null
}

interface KnowledgeImportProgressEvent extends KnowledgeImportProgress {
  requestId: string
  fileName: string
}

export const knowledgeList = () => invokeData<KnowledgeSourceSummary[]>('knowledge_list')
export const knowledgeSearch = (query: string) => invokeData<KnowledgeSearchResult>('knowledge_search', { query })
export const knowledgeDelete = (id: string) => invokeData<boolean>('knowledge_delete', { id })
export const knowledgeAsk = (input: {
  question: string
  history: KnowledgeHistoryEntry[]
  selectedFlowId: string | null
}) => invokeData<KnowledgeAskResult>('knowledge_ask', { input })

export async function knowledgeImportFile(file: File, onProgress?: (progress: KnowledgeImportProgress) => void) {
  if (file.size > 20 * 1024 * 1024) throw new Error('单个文件最多 20 MiB，请分拆文件后重试。')
  const requestId = crypto.randomUUID()
  let unlisten: (() => void) | undefined
  onProgress?.({ phase: 'reading', message: '正在读取文件…', current: null, total: null })
  if (onProgress) {
    try {
      unlisten = await listen<KnowledgeImportProgressEvent>('knowledge-import-progress', event => {
        if (event.payload.requestId === requestId && event.payload.fileName === file.name) {
          const { phase, message, current, total } = event.payload
          onProgress({ phase, message, current, total })
        }
      })
    } catch {
      onProgress({ phase: 'parsing', message: '实时进度连接暂不可用，仍将继续导入', current: null, total: null })
    }
  }
  try {
    const dataBase64 = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader()
      reader.onerror = () => reject(new Error(`读取「${file.name}」失败，请重新选择。`))
      reader.onload = () => resolve(String(reader.result).split(',')[1] ?? '')
      reader.readAsDataURL(file)
    })
    if (!dataBase64) throw new Error(`「${file.name}」为空文件，无法导入。`)
    return await invokeData<{ source: KnowledgeSourceSummary; duplicate: boolean }>('knowledge_import', {
      name: file.name,
      dataBase64,
      requestId,
    })
  } finally {
    unlisten?.()
  }
}
