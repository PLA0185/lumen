/**
 * 大报告导出：**流式**构建打印文档（第三轮整改任务书 §7）。
 *
 * ## 为什么不能沿用"一次取全量 + React 渲染"
 *
 * 上一轮的实现是后端一口气返回最多 `REPORT_MAX_ROWS`（10 万）条完整 Task，
 * 前端把它塞进 React state，再由 `<PrintReport>` 渲染成最多 10 万个 `<tr>`。
 * 在大 description / note / 标签的场景下这有三重风险：
 *
 * 1. 一次 IPC 要序列化几十 MB JSON；
 * 2. React state 里同时留着全部对象，内存翻倍；
 * 3. 渲染 10 万行会长时间卡住界面，甚至打印失败。
 *
 * ## 现在的做法
 *
 * - **分页取数**（每页 500 条），一行渲染完就丢掉，JS 里不留行数组；
 * - 直接构造 DOM 挂到 `document.body`（在 `.app` 之外，打印时主界面隐藏）；
 * - 过程中回报 `已处理 / 总数`，界面据此显示进度（任务书 §7.5 不允许点了没反应）；
 * - **每一行写入之前**都判定一次：先看用户有没有点取消，再看两道规模上限
 *   （`REPORT_MAX_ROWS` 行数、`REPORT_MAX_CHARS` 字符数，任务书 §7.4：不能只看行数）；
 * - 返回 `dispose()`，打印结束后由调用方释放这份 DOM。
 *
 * ## 两道闸的边界语义（第四轮收口任务书 §20/§22：代码、注释、测试同一套说法）
 *
 * - **行数**：上限的含义是"最多写多少行"——第 `REPORT_MAX_ROWS` 行**写入**，
 *   第 `REPORT_MAX_ROWS + 1` 行不写，并标记截断；
 * - **字符**：写入一行**之前**先算"加上这一行会不会超过 `REPORT_MAX_CHARS`"——
 *   累计**恰好等于**上限的内容可以完整写入，超过才停下并**把这一行排除**（`>` 而非 `>=`）；
 * - **取消**：优先于两道上限，而且**逐行检查**——用户点了取消最多再写一行就停，
 *   不必等整页 500 行跑完（§20）。
 *
 * 判定本身是纯计算，集中在 `shouldStop()` / `rowGate()` / `consumeRows()` 里，
 * 构建器与 `report-export.test.ts` 调用的是**同一份实现**，不存在两套说法。
 *
 * 因触顶或用完取消而提前结束时，`truncated` 为 true、`truncatedNote` 带中文原因，
 * 并且**这条原因会如实写进报告的页脚**（同时作为界面提示返回）——
 * 报告不会假装自己包含了全部内容。
 *
 * 表结构与 CSS 类名与上一版的 `PrintReport` 一致，打印出来还是同一份文档。
 */

import * as ipc from './ipc'
import { isOverdue } from './datetime'
import type { TaskQuery } from './types'
import type { TaskReportRow } from './ipc'

/** 一页取多少条（远低于后端单页上限 PAGE_MAX = 1000） */
const PAGE_SIZE = 500

/**
 * 单次导出的**字符**上限（标题 + 描述 + 项目/分类/标签名）。
 *
 * 任务书 §7.4 要求同时限制行数与总规模：只看行数时，
 * "10000 条 × 20k 描述"这种组合仍然能把内存打满。
 * 2000 万字符大约是几十 MB 文本，已远超正常使用场景。
 *
 * 语义（第四轮收口 §22）：写入一行**之前**判断"加上这一行会不会超过上限"，
 * 累计**恰好等于**上限的内容可以完整写入，超过才停（`>` 而不是 `>=`）。
 */
export const REPORT_MAX_CHARS = 20_000_000

/**
 * 单次导出的**行数**上限（第三轮收口任务书 §17）。
 *
 * 与字符上限一起构成两道闸：只看字符数时，"10 万条极短标题"仍能造出十万个
 * DOM 节点把界面拖死；只看行数时，"1 万条 2 万字段落"又能把内存打满。
 *
 * 语义（第四轮收口 §22）：这是"最多写多少行"——第 `REPORT_MAX_ROWS` 行**写入**，
 * 第 `REPORT_MAX_ROWS + 1` 行不写并标记截断。
 */
export const REPORT_MAX_ROWS = 100_000

export interface ReportProgress {
  /** 已经写入 DOM 的行数 */
  loaded: number
  /** 这次导出预计的总行数 */
  total: number
}

export interface ReportExportOptions {
  /** 与列表一致的筛选条件 */
  query: TaskQuery
  /** 报告标题（例如「今天」「全部任务」） */
  scopeTitle: string
  /** 附加筛选说明，例如 `搜索「报告」` */
  filterNote?: string
  /** 进度回调：只带计数，不带行数据 */
  onProgress?: (p: ReportProgress) => void
  /**
   * 取消标志的读取函数；返回 true 时停止继续导出（第四轮收口 §20）。
   *
   * 构建器在**取下一页之前**与**每一行写入之前**各调用一次，
   * 所以用户点了取消之后最多再写一行就停，不会等整页 500 行处理完。
   */
  isCancelled?: () => boolean
}

export interface ReportExportHandle {
  /** 实际写入的行数 */
  rows: number
  /** 这次导出预期的总行数（来自 count） */
  total: number
  /** 是否因为触及上限/被取消而提前结束（原因见 `truncatedNote`，也写在报告页脚里） */
  truncated: boolean
  /** 提前结束的原因（给用户看的中文说明，与报告页脚上的说法一致） */
  truncatedNote?: string
  /** 挂载在 body 上的报告根节点 */
  mount: HTMLElement
  /** 打印结束后调用：把报告从 DOM 里摘掉 */
  dispose: () => void
}

const STATUS_TEXT: Record<string, string> = {
  todo: '待办',
  doing: '进行中',
  waiting: '等待',
  done: '已完成',
  archived: '已归档',
}

const PRIORITY_TEXT: Record<number, string> = { 0: '—', 1: '低', 2: '中', 3: '高' }

const PERIOD_TEXT: Record<string, string> = {
  week: '本周',
  month: '本月',
  quarter: '本季度',
  year: '本年',
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag)
  if (className) node.className = className
  if (text !== undefined) node.textContent = text
  return node
}

/** 把 UTC ISO 转成"本地时区 + 是否带具体时刻"的可读文本（与旧版报告一致） */
function timeText(v: string | null, hasTime: number): string {
  if (!v) return '—'
  const d = new Date(v)
  if (Number.isNaN(d.getTime())) return '—'
  const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
  if (hasTime !== 1) return date
  return `${date} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

function generatedStamp(): string {
  const now = new Date()
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(
    now.getDate(),
  ).padStart(2, '0')} ${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`
}

/**
 * 一行的字符占用（用于总字符数上限）。
 *
 * 导出成公开函数是为了能单测：这条上限是"大报告不会把内存打满"的
 * 最后一道闸（任务书 §7.4 要求不能只看行数），必须能被测试直接盯住。
 */
export function reportRowChars(r: TaskReportRow): number {
  return (
    r.task.title.length +
    (r.task.description?.length ?? 0) +
    (r.projectName?.length ?? 0) +
    (r.categoryName?.length ?? 0) +
    r.tagNames.join('').length
  )
}

/** 两道上限的判定输入：已写入多少 + 这一行要占多少 */
export interface RowLimitState {
  /** 已经写入 DOM 的行数 */
  rows: number
  /** 已经写入的字符数 */
  chars: number
  /** 即将写入的这一行占多少字符（`reportRowChars`） */
  rowCost: number
}

export interface StopDecision {
  /** true = 这一行**不写**，导出到此为止 */
  stop: boolean
  /** 停止的原因；`stop` 为 false 时不存在 */
  reason?: 'rows' | 'chars'
}

/**
 * 两道上限的判定：**写入这一行之前**问一句"还能不能写"。
 *
 * 边界语义（第四轮收口 §22，与模块头注释、单测保持同一套说法）：
 * - 行数按"最多写多少行"：`rows + 1 > REPORT_MAX_ROWS` 才停，
 *   所以第 `REPORT_MAX_ROWS` 行写得进去，第 `REPORT_MAX_ROWS + 1` 行不写；
 * - 字符按"加上这一行会不会超过"：`chars + rowCost > REPORT_MAX_CHARS` 才停，
 *   所以累计**恰好等于**上限的内容被完整写入，而不是一到上限就停。
 *
 * 这是判定上限的**唯一实现**：`buildPrintDocument` 与 `report-export.test.ts`
 * 调用的是同一份代码。
 *
 * 旧版的 `exceedsCharLimit(chars, max) => chars >= max` 表达的是"已经到顶"，
 * 与本轮统一的"写之前预判"不是同一件事，而且会把恰好等于上限的内容挡在门外，
 * 因此被本函数取代（不留两套说法）。
 */
export function shouldStop({ rows, chars, rowCost }: RowLimitState): StopDecision {
  if (rows + 1 > REPORT_MAX_ROWS) return { stop: true, reason: 'rows' }
  if (chars + rowCost > REPORT_MAX_CHARS) return { stop: true, reason: 'chars' }
  return { stop: false }
}

/** 停下来的类别：用户取消优先于两道上限 */
export type StopReason = 'cancelled' | 'rows' | 'chars'

export interface RowGateInput extends RowLimitState {
  /** 用户是否已经点过取消（每一行写入之前实时读取） */
  cancelled: boolean
}

export interface RowGateDecision {
  stop: boolean
  reason?: StopReason
  /**
   * 停止时给用户看的中文说明。
   *
   * `ReportExportHandle.truncatedNote` 与报告页脚用的是**同一份文案**，
   * 所以界面上看到的和报告里写的不会两样。
   */
  note?: string
}

/** 取消时给用户看的说明 */
const CANCEL_NOTE = '导出已取消'
/** 触顶行数上限时的说明 */
const ROWS_LIMIT_NOTE = `任务数量超过单次导出上限（${REPORT_MAX_ROWS} 条），剩余部分未包含`
/** 触顶字符上限时的说明 */
const CHARS_LIMIT_NOTE = `内容总长度超过单次导出上限（约 ${Math.round(
  REPORT_MAX_CHARS / 1_000_000,
)} 百万字符），剩余任务未包含`

/**
 * 逐行的完整闸门：**取消优先于两道上限**。
 *
 * 构建器在"取下一页之前"与"写每一行之前"都调用它：前者让取消立刻生效
 * （不必再发一次 IPC），后者让取消与两道上限都是**逐行**生效的（§20）。
 */
export function rowGate({ cancelled, rows, chars, rowCost }: RowGateInput): RowGateDecision {
  if (cancelled) return { stop: true, reason: 'cancelled', note: CANCEL_NOTE }
  const limit = shouldStop({ rows, chars, rowCost })
  if (!limit.stop) return { stop: false }
  return {
    stop: true,
    reason: limit.reason,
    note: limit.reason === 'rows' ? ROWS_LIMIT_NOTE : CHARS_LIMIT_NOTE,
  }
}

export interface ConsumeRowsInput<T> {
  /** 这一批待写入的行（构建器传当前页） */
  items: readonly T[]
  /** 每一行的字符成本（构建器传 `reportRowChars`） */
  costOf: (item: T) => number
  /** 进入这一批之前**已经写入**的行数 */
  rows: number
  /** 进入这一批之前**已经写入**的字符数 */
  chars: number
  /** 取消标志的读取函数：每一行写入之前都会调用一次（§20） */
  isCancelled?: () => boolean
  /** 真正写入一行时调用：构建器在这里 append `<tr>`，单测里只记账、不碰 DOM */
  onWrite: (item: T, index: number) => void
}

export interface ConsumeRowsResult {
  /** 这一批实际写入的行数（<= `items.length`） */
  written: number
  /** 停下来时的判定（原因 + 中文说明）；整批都写完时为 undefined */
  stop?: RowGateDecision
}

/**
 * 逐行写入一批数据：**每一行之前**都重新判定一次（取消 → 行数上限 → 字符上限）。
 *
 * 这是"什么时候该停"的**唯一实现**，`buildPrintDocument` 与单测共用：
 * 构建器把 `onWrite` 接到 DOM 上，单测传一个只记账的回调，
 * 于是"取消发生在第几行、一共写了多少行、截断原因是什么"在没有 DOM 的环境里也能被断言。
 *
 * 取消逐行检查（§20），所以用户点了取消之后不必等整页 500 行跑完。
 */
export function consumeRows<T>(input: ConsumeRowsInput<T>): ConsumeRowsResult {
  const { items, costOf, rows, chars, isCancelled, onWrite } = input
  let written = 0
  let used = 0

  for (const [index, item] of items.entries()) {
    const cost = costOf(item)
    const decision = rowGate({
      cancelled: isCancelled?.() === true,
      rows: rows + written,
      chars: chars + used,
      rowCost: cost,
    })
    if (decision.stop) return { written, stop: decision }

    onWrite(item, index)
    written += 1
    used += cost
  }

  return { written }
}

/** 一个分组（标题 + 表格 + 行数徽标） */
interface Group {
  label: string
  section: HTMLElement
  body: HTMLTableSectionElement
  count: HTMLElement
  shown: boolean
}

/** 建一个分组容器；一开始隐藏，出现第一行时才显示 */
function makeGroup(mount: HTMLElement, label: string): Group {
  const section = el('section', 'report__group')
  section.style.display = 'none'

  const h2 = el('h2', 'report__group-title', label)
  const count = el('span', 'report__group-count', '0')
  h2.appendChild(count)
  section.appendChild(h2)

  const table = el('table', 'report__table')
  const thead = el('thead')
  const headRow = el('tr')
  const heads: [string, string][] = [
    ['#', 'report__col-idx'],
    ['状态', 'report__col-state'],
    ['任务', ''],
    ['计划', 'report__col-time'],
    ['截止', 'report__col-time'],
    ['优先级', 'report__col-prio'],
    ['项目 / 分类', 'report__col-org'],
    ['标签', 'report__col-tags'],
  ]
  for (const [txt, cls] of heads) headRow.appendChild(el('th', cls || undefined, txt))
  thead.appendChild(headRow)
  table.appendChild(thead)

  const body = el('tbody')
  table.appendChild(body)
  section.appendChild(table)
  mount.appendChild(section)
  return { label, section, body, count, shown: false }
}

/**
 * 流式构建打印报告。
 *
 * 调用方拿到 handle 后：
 * 1. 设 `document.body.dataset.print = 'on'`；
 * 2. 调后端 `export_pdf`（它让 WebView2 打印当前页面）；
 * 3. 无论成败都 `dispose()` 并清掉 `data-print`。
 */
export async function buildPrintDocument(
  opts: ReportExportOptions,
): Promise<ReportExportHandle> {
  const { query, scopeTitle, filterNote, onProgress, isCancelled } = opts

  const mount = el('div', 'report')
  mount.id = 'print-report'
  document.body.appendChild(mount)

  const generated = generatedStamp()
  const header = el('header', 'report__head')
  header.appendChild(el('h1', 'report__title', 'Lumen 任务清单'))
  header.appendChild(
    el('div', 'report__sub', `范围：${scopeTitle}${filterNote ? `　·　${filterNote}` : ''}`),
  )
  header.appendChild(el('div', 'report__sub', `生成时间：${generated}`))
  mount.appendChild(header)

  // 统计数字最后回填（这里先把结构立起来，避免为了统计先攒一遍数据）
  const statsSection = el('section', 'report__stats')
  const statNodes: Record<'total' | 'open' | 'overdue' | 'done', HTMLElement> = {
    total: el('span'),
    open: el('span'),
    overdue: el('span'),
    done: el('span'),
  }
  const statDefs: ['total' | 'open' | 'overdue' | 'done', string, string][] = [
    ['total', '任务总数', ''],
    ['open', '未完成', ''],
    ['overdue', '已逾期', 'report__stat-num--danger'],
    ['done', '已完成', ''],
  ]
  for (const [key, label, extra] of statDefs) {
    const box = el('div', 'report__stat')
    const num = statNodes[key]
    num.className = extra ? `report__stat-num ${extra}` : 'report__stat-num'
    num.textContent = '0'
    box.appendChild(num)
    box.appendChild(el('span', 'report__stat-label', label))
    statsSection.appendChild(box)
  }
  mount.appendChild(statsSection)

  // 三个分组：逾期 → 未完成 → 已完成（与上一版报告一致）
  const groups: Record<'overdue' | 'open' | 'done', Group> = {
    overdue: makeGroup(mount, '已逾期'),
    open: makeGroup(mount, '进行中 / 待办'),
    done: makeGroup(mount, '已完成'),
  }

  const footer = el('footer', 'report__foot', '')
  mount.appendChild(footer)

  const chars = { total: 0 }
  const stats = { total: 0, open: 0, overdue: 0, done: 0 }

  const appendRow = (group: Group, row: TaskReportRow, idx: number) => {
    const t = row.task
    const overdue = isOverdue(t)
    const tr = el('tr', overdue ? 'report__row--overdue' : '')
    tr.appendChild(el('td', 'report__col-idx', String(idx)))
    tr.appendChild(
      el(
        'td',
        'report__col-state',
        `${t.status === 'done' ? '✓ ' : ''}${STATUS_TEXT[t.status] ?? t.status}`,
      ),
    )

    const titleCell = el('td')
    titleCell.appendChild(el('div', 'report__task-title', t.title))
    if (t.description) titleCell.appendChild(el('div', 'report__task-desc', t.description))
    const period = t.periodType && t.periodType !== 'none' ? PERIOD_TEXT[t.periodType] : null
    if (period) titleCell.appendChild(el('div', 'report__task-note', `周期：${period}内完成即可`))
    if (t.seriesId) titleCell.appendChild(el('div', 'report__task-note', '重复任务的一次发生'))
    tr.appendChild(titleCell)

    tr.appendChild(el('td', 'report__col-time', timeText(t.plannedAt, t.hasPlannedTime)))
    tr.appendChild(el('td', 'report__col-time', timeText(t.dueAt, t.hasDueTime)))
    tr.appendChild(el('td', 'report__col-prio', PRIORITY_TEXT[t.priority] ?? '—'))
    tr.appendChild(
      el(
        'td',
        'report__col-org',
        [row.projectName, row.categoryName].filter(Boolean).join(' / ') || '—',
      ),
    )
    tr.appendChild(el('td', 'report__col-tags', row.tagNames.join('、') || '—'))

    group.body.appendChild(tr)
    if (!group.shown) {
      group.section.style.display = ''
      group.shown = true
    }
    group.count.textContent = String(group.body.childElementCount)
    chars.total += reportRowChars(row)
  }

  // ---- 先拿总数，用来显示进度 ----
  let total = 0
  try {
    total = (await ipc.countTasks(query)).total
  } catch {
    // 计数失败不阻断：下面按页读到取空为止
  }
  onProgress?.({ loaded: 0, total })

  // ---- 分页流式取数并渲染 ----
  let offset = 0
  let truncated = false
  let truncatedNote: string | undefined
  /** 逐行循环因为取消/触顶停下后置为 true：外层据此不再取下一页 */
  let stopped = false

  for (;;) {
    // 取数之前也走同一道闸：用户已经点了取消就不必再发这一次 IPC。
    // 这样"取页前的取消"与"某一行之前取消"用的是同一份判定（§20/§21）。
    const beforePage = rowGate({
      cancelled: isCancelled?.() === true,
      rows: stats.total,
      chars: chars.total,
      rowCost: 0,
    })
    if (beforePage.stop) {
      // 取数之前就停下：直接跳出循环，不必再经过下面的 stopped 判定
      truncated = true
      truncatedNote = beforePage.note
      break
    }

    const page = await ipc.taskReport({ ...query, limit: PAGE_SIZE, offset })
    if (page.length === 0) break

    // **逐行**判定与写入（第三轮收口任务书 §17 + 第四轮 §20/§22）：
    // - 取消逐行检查，用户点取消后不必等整页 500 行跑完；
    // - 两道上限同样逐行判定——按页检查时"一页 500 条 × 每条 20 万字符"
    //   会先被整个塞进 DOM，上限就形同虚设。
    // 判定规则全在 consumeRows()/rowGate()/shouldStop() 里（单测调用同一份实现）。
    const consumed = consumeRows({
      items: page,
      costOf: reportRowChars,
      rows: stats.total,
      chars: chars.total,
      isCancelled,
      onWrite: (row) => {
        const t = row.task
        stats.total += 1
        if (t.status === 'done') {
          stats.done += 1
          appendRow(groups.done, row, stats.total)
        } else if (isOverdue(t)) {
          stats.overdue += 1
          stats.open += 1
          appendRow(groups.overdue, row, stats.total)
        } else {
          stats.open += 1
          appendRow(groups.open, row, stats.total)
        }
      },
    })

    if (consumed.stop) {
      truncated = true
      truncatedNote = consumed.stop.note
      stopped = true
    }

    offset += consumed.written
    onProgress?.({ loaded: stats.total, total: total || stats.total })

    // 已经因为上限停下（或用户取消）就不再继续取下一页
    if (stopped) break
    if (consumed.written < PAGE_SIZE) break
  }

  // ---- 回填统计与页脚 ----
  statNodes.total.textContent = String(stats.total)
  statNodes.open.textContent = String(stats.open)
  statNodes.overdue.textContent = String(stats.overdue)
  statNodes.done.textContent = String(stats.done)
  // 截断原因**如实**写进页脚：被用户取消与"触及规模上限"是两回事，
  // 不能都笼统写成"因超出上限"。这里用的是与 handle.truncatedNote 相同的文案。
  const summary = `由 Lumen 生成　·　导出时间为 ${generated}　·　共 ${stats.total} 条任务`
  footer.textContent = truncated
    ? `${summary}（${truncatedNote ?? '已提前结束'}，本报告仅含已写入的部分）`
    : summary

  if (stats.total === 0) {
    mount.appendChild(el('p', 'report__empty', '当前范围内没有任务。'))
  }

  return {
    rows: stats.total,
    total: total || stats.total,
    truncated,
    truncatedNote,
    mount,
    dispose: () => {
      mount.remove()
    },
  }
}
