import type { FlowStep, SaveMemoInput } from './memos-ipc'

export interface ChatGPTImportPreview extends SaveMemoInput {
  source: string
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value)
}

function optionalString(value: unknown, field: string): string | undefined {
  if (value === undefined || value === null) return undefined
  if (typeof value !== 'string') throw new Error(`「${field}」必须是文字`)
  return value
}

function parseJsonBlock(raw: string): Record<string, unknown> | null {
  const fenced = raw.match(/```(?:json)?\s*([\s\S]*?)```/i)
  const candidate = (fenced?.[1] ?? raw).trim()
  if (!candidate.startsWith('{')) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(candidate)
  } catch {
    throw new Error('JSON 格式不正确。请修正后重新生成预览，或改为粘贴 Markdown / 纯文本。')
  }
  if (!isRecord(parsed)) throw new Error('导入内容应为 JSON 对象')
  return parsed
}

function markdownTitle(raw: string): { title: string; bodyMd: string } | null {
  const match = raw.match(/^\s*#\s+(.+?)\s*(?:\r?\n|$)/)
  return match ? { title: match[1]!.trim(), bodyMd: raw.slice(match[0].length).trimStart() } : null
}

function imageReferences(text: string): string[] {
  return [...text.matchAll(/!\[[^\]\n]*\]\(lumen-asset:[0-9a-f-]{36}(?: "(?:\\.|[^"\\\n])*")?\)/gi)].map(match => match[0])
}

function preserveImages(text: string, previous: string): string {
  const existing = new Set(imageReferences(text).map(markdown => markdown.match(/lumen-asset:([0-9a-f-]{36})/i)?.[1]?.toLowerCase()))
  const missing = imageReferences(previous).filter(markdown => {
    const id = markdown.match(/lumen-asset:([0-9a-f-]{36})/i)?.[1]?.toLowerCase()
    return !!id && !existing.has(id)
  })
  return missing.length ? `${text.trimEnd()}${text.trim() ? '\n\n' : ''}${missing.join('\n\n')}` : text
}

function matchPreviousStep(previous: FlowStep[], title: string, used: Set<string>): FlowStep | undefined {
  const named = title ? previous.find(step => step.title.trim() === title && !used.has(step.id)) : undefined
  return named
}

function imageMap(text: string): Map<string, string> {
  return new Map(imageReferences(text).flatMap(markdown => {
    const id = markdown.match(/lumen-asset:([0-9a-f-]{36})/i)?.[1]?.toLowerCase()
    return id ? [[id, markdown] as const] : []
  }))
}

function preserveFlowImages(base: SaveMemoInput, next: ChatGPTImportPreview): void {
  if (base.kind !== 'flow' || next.kind !== 'flow') return
  const previousImages = imageMap([base.bodyMd, ...base.steps.map(step => step.detail)].join('\n'))
  const nextImages = imageMap([next.bodyMd, ...next.steps.map(step => step.detail)].join('\n'))
  const orphaned = [...previousImages].filter(([id]) => !nextImages.has(id)).map(([, markdown]) => markdown)
  if (orphaned.length) next.bodyMd = `${next.bodyMd.trimEnd()}${next.bodyMd.trim() ? '\n\n' : ''}${orphaned.join('\n\n')}`
}

function markdownFlow(raw: string, previous: FlowStep[]) {
  const lines = raw.split(/\r?\n/)
  const titleIndex = lines.findIndex(line => /^\s*#\s+\S/.test(line))
  const title = titleIndex >= 0 ? lines[titleIndex]!.replace(/^\s*#\s+/, '').trim() : ''
  if (titleIndex >= 0) lines.splice(titleIndex, 1)
  const headings = lines.flatMap((line, index) => {
    const match = line.match(/^\s*#{2,6}\s+(.+?)\s*$/)
    return match ? [{ index, title: match[1]!.replace(/^第\s*\d+\s*步\s*[:：、.．-]?\s*/, '').trim() }] : []
  })
  if (!headings.length) return { title, bodyMd: preserveImages(lines.join('\n').trim(), ''), steps: previous }
  const used = new Set<string>()
  const steps = headings.map((heading, index) => {
    const end = headings[index + 1]?.index ?? lines.length
    const old = matchPreviousStep(previous, heading.title, used)
    if (old) used.add(old.id)
    const detail = preserveImages(lines.slice(heading.index + 1, end).join('\n').trim(), old?.detail ?? '')
    if (heading.title.length > 300 || detail.length > 5000) throw new Error(`第 ${index + 1} 步标题或说明超过字段长度限制`)
    return {
      id: old?.id ?? crypto.randomUUID(),
      title: heading.title || `第 ${index + 1} 步`,
      owner: old?.owner ?? '',
      detail,
      ...(old?.layout ? { layout: old.layout } : {}),
      ...(old?.group ? { group: old.group } : {}),
    }
  })
  return { title, bodyMd: preserveImages(lines.slice(0, headings[0]!.index).join('\n').trim(), ''), steps }
}

function parseSteps(value: unknown, previous: FlowStep[]): FlowStep[] {
  if (!Array.isArray(value)) throw new Error('「steps」必须是数组')
  if (value.length > 100) throw new Error('流程步骤最多 100 步')
  const previousById = new Map(previous.map(step => [step.id, step]))
  return value.map((item, index) => {
    if (!isRecord(item)) throw new Error(`第 ${index + 1} 步格式不正确`)
    const title = optionalString(item.title, `第 ${index + 1} 步标题`)?.trim() ?? ''
    const owner = optionalString(item.owner, `第 ${index + 1} 步负责人`) ?? ''
    const detail = optionalString(item.detail, `第 ${index + 1} 步说明`)
      ?? optionalString(item.content, `第 ${index + 1} 步说明`) ?? ''
    if (!title) throw new Error(`第 ${index + 1} 步缺少标题`)
    if (title.length > 300 || owner.length > 100 || detail.length > 5000)
      throw new Error(`第 ${index + 1} 步超过字段长度限制`)
    const suppliedId = optionalString(item.id, `第 ${index + 1} 步编号`)
    const old = (suppliedId ? previousById.get(suppliedId) : undefined) ?? previous[index]
    const nextDetail = preserveImages(detail, old?.detail ?? '')
    if (nextDetail.length > 5000) throw new Error(`第 ${index + 1} 步说明超过字段长度限制`)
    return {
      id: old?.id ?? crypto.randomUUID(),
      title,
      owner,
      detail: nextDetail,
      ...(old?.layout ? { layout: old.layout } : {}),
      ...(old?.group ? { group: old.group } : {}),
    }
  })
}

/** Convert copied ChatGPT Markdown/JSON into an unsaved proposal for explicit review. */
export function parseChatGPTContentImport(
  raw: string,
  base: SaveMemoInput,
  isNew: boolean,
): ChatGPTImportPreview {
  const source = raw.trim()
  if (!source) throw new Error('先粘贴 ChatGPT 的内容')
  if (source.length > 100_000) throw new Error('内容超过 100,000 字符，请拆分后导入')

  const next: ChatGPTImportPreview = { ...base, source }
  const json = parseJsonBlock(source)
  if (!json) {
    if (base.kind === 'flow') {
      const parsed = markdownFlow(source, base.steps)
      if (isNew && parsed.title) next.title = parsed.title
      next.bodyMd = preserveImages(parsed.bodyMd, base.bodyMd)
      next.steps = parsed.steps
    } else {
      const heading = isNew ? markdownTitle(source) : null
      if (heading) {
        next.title = heading.title
      next.bodyMd = preserveImages(heading.bodyMd, base.bodyMd)
      } else {
        next.bodyMd = preserveImages(source, base.bodyMd)
        if (isNew && !next.title.trim()) next.title = 'ChatGPT 导入'
      }
    }
  } else {
    const title = optionalString(json.title, 'title')
    const category = optionalString(json.category, 'category')
    const bodyMd = optionalString(json.bodyMd, 'bodyMd')
      ?? optionalString(json.content, 'content')
      ?? optionalString(json.summary, 'summary')
    const kind = optionalString(json.kind, 'kind')
    if (title !== undefined) next.title = title.trim()
    if (category !== undefined) next.category = category
    if (bodyMd !== undefined) next.bodyMd = preserveImages(bodyMd, base.bodyMd)
    if (kind !== undefined) {
      if (kind !== 'memo' && kind !== 'flow') throw new Error('「kind」只能是 memo 或 flow')
      next.kind = kind
    }
    if (json.steps !== undefined) next.steps = parseSteps(json.steps, base.steps)
    if (isNew && !title && !bodyMd && !json.steps) throw new Error('JSON 中请至少提供 title、bodyMd 或 steps')
  }

  if (!next.title.trim()) throw new Error('标题不能为空')
  if (next.title.length > 500 || next.category.length > 100 || next.bodyMd.length > 100_000)
    throw new Error('标题、分类或正文超过允许长度')
  if (next.kind === 'memo') next.steps = []
  if (next.kind === 'flow' && next.steps.length > 100) throw new Error('流程步骤最多 100 步')
  preserveFlowImages(base, next)
  if (next.bodyMd.length > 100_000) throw new Error('正文含保留图片引用后超过允许长度')
  return next
}

export const CHATGPT_IMPORT_PROMPT = `请把我提供的内容整理成一条 Lumen 记录，并自行判断类型。只返回一个 JSON 对象，不要代码围栏或额外解释。

判断规则：
- 内容是可重复执行、存在先后顺序的操作步骤时，使用业务流程：kind 写 "flow"，按原顺序填写 steps；bodyMd 只放适用范围、背景或材料说明。
- 内容是术语、规则、背景、资料、经验或独立注意事项，不需要按步骤执行时，使用备忘：kind 写 "memo"，正文写入 bodyMd，steps 使用空数组。
- 不要为了凑流程把独立信息改写成步骤，也不要把多个无关业务合并到一个流程。

根据判断结果，只选下面一种对象格式输出：
备忘：
{
  "kind": "memo",
  "title": "备忘标题",
  "category": "分类；未知或不适用时留空",
  "bodyMd": "Markdown 正文",
  "steps": []
}
流程：
{
  "kind": "flow",
  "title": "流程标题",
  "category": "分类；未知或不适用时留空",
  "bodyMd": "适用范围、背景或材料说明；没有则留空",
  "steps": [
    { "title": "步骤名称", "owner": "负责人或部门；未知时留空", "detail": "具体操作、完成标准和注意事项" }
  ]
}

若 kind 是 "memo"，steps 必须是空数组；若 kind 是 "flow"，按原文顺序拆分步骤，只合并同一流程中的连续动作。不得编造原材料中没有的信息。`
