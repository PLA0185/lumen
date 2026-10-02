import { invokeData } from './data-change'
import { IpcError, isTauri } from './ipc'
import type { BackendError } from './types'

export interface FlowStep {
  id: string
  title: string
  owner: string
  detail: string
  layout?: { width: number; minHeight: number; x?: number; y?: number }
}
export interface MemoSummary {
  id: string
  title: string
  category: string
  kind: 'memo' | 'flow'
  revision: number
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}
export interface MemoDocument extends MemoSummary {
  bodyMd: string
  steps: FlowStep[]
}
export interface SaveMemoInput {
  id: string | null
  expectedRevision: number | null
  title: string
  category: string
  kind: 'memo' | 'flow'
  bodyMd: string
  steps: FlowStep[]
}
async function call<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  if (!isTauri())
    throw new IpcError(
      'internal',
      '请在 Lumen 桌面程序中使用备忘与流程',
      null,
      null,
    )
  try {
    return await invokeData<T>(command, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
      const err = e as BackendError
      throw new IpcError(err.code, err.message, err.hint ?? null, e)
    }
    throw new IpcError(
      'internal',
      typeof e === 'string' ? e : '备忘操作失败，请重试',
      null,
      e,
    )
  }
}
export const memoList = (query = '', deletedOnly = false) =>
  call<MemoSummary[]>('memo_list', { query, deletedOnly })
export const memoGet = (id: string) => call<MemoDocument>('memo_get', { id })
export const memoSave = (input: SaveMemoInput) =>
  call<MemoDocument>('memo_save', { input })
export const memoSetDeleted = (
  id: string,
  revision: number,
  deleted: boolean,
) => call<MemoDocument>('memo_set_deleted', { id, revision, deleted })
// UI/order numbering is separate from the immutable source heading. Leave
// decimal/version prefixes (e.g. 6.2) and numbers in the body untouched.
export function flowStepDisplayTitle(title: string): string {
  return title.replace(/^\s*\d+[.、．]\s*(?=[^\d\s])/, '').trim() || '未命名步骤'
}
export function memoMarkdown(doc: SaveMemoInput): string {
  const steps = doc.steps
    .map(
      (step, i) =>
        `### ${i + 1}. ${flowStepDisplayTitle(step.title)}\n\n${step.owner ? `负责人：${step.owner}\n\n` : ''}${step.detail}`,
    )
    .join('\n\n↓\n\n')
  return `# ${doc.title}\n\n${doc.category ? `分类：${doc.category}\n\n` : ''}${doc.bodyMd}${steps ? `\n\n## 流程步骤\n\n${steps}` : ''}`
}
