/**
 * 全局状态（zustand）。
 *
 * 设计取舍：状态只保存"从后端读到的真相 + 界面态"，
 * 不做乐观更新以外的缓存推导，避免出现"界面显示与数据库不一致"。
 * 所有写操作都先落库、再用后端返回值刷新本地状态（§1 要求真实持久化闭环）。
 */

import { create } from 'zustand'
import * as ipc from './ipc'
import { IpcError } from './ipc'
import type {
  AppInfo,
  DataPaths,
  Task,
  TaskQuery,
  TaskStatus,
  TodayOverview,
  ViewId,
} from './types'
import { todayRange } from './datetime'

/** 列表加载状态（§3：空列表、加载中、失败都要有明确状态） */
export type LoadState = 'idle' | 'loading' | 'ready' | 'error'

/**
 * 一页加载多少条（第二轮整改任务书 §4.3 建议 100～300）。
 *
 * 为什么必须分页：整改前列表固定 `limit: 500` 且没有任何"加载更多"，
 * 于是第 501 条之后的任务**数据库里有、界面上永远看不到**。
 * 这不是性能问题，是数据可见性问题。
 */
export const PAGE_SIZE = 200

/** 界面反馈消息 */
export interface Toast {
  id: string
  kind: 'success' | 'error' | 'info' | 'reminder'
  text: string
  /** 提醒类消息带上任务 ID，界面据此提供「打开任务」的跳转按钮 */
  taskId?: string
}

interface AppStore {
  // ------------------------------ 数据 ------------------------------
  tasks: Task[]
  /** 任务 ID → 子任务进度。列表页**批量**获取一次，
   * 而不是让每张卡片各查一次，否则 1000 条任务会产生 1000 次查询（§10）。 */
  progressMap: Record<string, { total: number; done: number; percent: number | null }>
  loadState: LoadState
  /** 加载失败时的可读原因 */
  loadError: string | null
  /**
   * 当前筛选条件下的**总条数**（后端 `task_count`，与列表同一套条件）。
   *
   * 注意它与 `tasks.length` 的区别：后者只是"已经加载了多少条"。
   * 破坏性操作的确认数量（如清空回收站）必须用这个值，
   * 否则会出现"确认删 500 项、实际删掉 1200 项"（§5）。
   */
  totalCount: number
  /** 是否还有未加载的任务 */
  hasMore: boolean
  /** 正在加载下一页（界面据此禁用按钮，避免重复请求） */
  loadingMore: boolean
  /** 下一页的 offset，等于已加载条数 */
  nextOffset: number
  /**
   * 查询代数（第三轮任务书 §4）。
   *
   * 任何筛选条件（视图、搜索、状态、排序、逾期）变化都会 +1。
   * 每个异步请求在开始时记下当时的代数，回来时先比对：对不上就说明
   * 用户已经换了条件，**这份结果必须丢弃**——否则会出现
   * "搜 A 的第二页，最后被追加进了搜 B 的结果里"。
   */
  queryGeneration: number
  overview: TodayOverview | null
  appInfo: AppInfo | null
  dataPaths: DataPaths | null

  // ---------------------------- 界面态 ------------------------------
  view: ViewId
  search: string
  /** 状态筛选；空数组表示"不限" */
  statusFilter: TaskStatus[]
  /** 排序方式（§4.1 可切换） */
  sortBy: NonNullable<TaskQuery['sortBy']>
  sortDesc: boolean
  /** 是否只看逾期 */
  overdueOnly: boolean
  /** 未完成数徽标是否显示 */
  theme: 'light' | 'dark' | 'system'
  toasts: Toast[]

  // ---------------------------- 动作 ------------------------------
  init: () => Promise<void>
  reload: () => Promise<void>
  /** 加载下一页并追加（不改动 offset，供滚动到底部或「加载更多」调用） */
  loadMore: () => Promise<void>
  refreshOverview: () => Promise<void>
  /** 别的窗口改了数据：作废在飞请求并从第一页重新加载 */
  handleExternalChange: () => Promise<void>
  setView: (v: ViewId) => void
  setSearch: (s: string) => void
  setStatusFilter: (s: TaskStatus[]) => void
  setSort: (by: NonNullable<TaskQuery['sortBy']>, desc?: boolean) => void
  setOverdueOnly: (v: boolean) => void
  setTheme: (t: 'light' | 'dark' | 'system') => void

  toggleDone: (id: string, done: boolean) => Promise<void>
  /** 复制为副本：新任务插在被复制项之后 */
  duplicate: (id: string) => Promise<void>
  /** 拖拽排序：把 movedId 放到 beforeId 之前（beforeId 为 null 表示放到末尾） */
  reorder: (movedId: string, beforeId: string | null) => Promise<void>
  remove: (id: string) => Promise<void>
  restore: (id: string) => Promise<void>
  purge: (id: string) => Promise<void>
  /**
   * 永久删除**第一阶段**：取回"当前筛选命中的精确任务集合"给用户确认。
   *
   * 返回的 `taskIds` 必须原样交给 `purgeAll()`——只传数量是不够的：
   * 两个不同的集合可以数量相同（A 被恢复、B 被移入且同样命中，count 仍为 1）。
   */
  preparePurge: () => Promise<{ query: TaskQuery; preview: ipc.PurgePreview }>
  /**
   * 永久删除**第二阶段**：只删确认过的那些 ID（第三轮收口任务书 §3）。
   *
   * 后端会在同一事务里核对"当前命中集合是否仍与 `taskIds` 逐个相同"，
   * 不一致就整体取消（零任务、零附件被删）并报冲突。
   */
  purgeAll: (query: TaskQuery, taskIds: string[]) => Promise<void>
  /**
   * 取一页报告数据（任务 + 已解析的归属名称）。
   *
   * 注意：**PDF 导出已经不经过这两个方法**（见 `src/lib/report-export.ts`）——
   * 它改为分页流式构建打印 DOM，数据不进 React state
   * （第三轮任务书 §7：不许把最多 10 万个完整 Task 全量塞进 WebView）。
   * 这两个方法保留给"确实需要一次性拿到全量数据"的调用方。
   */
  reportRows: () => Promise<ipc.TaskReportRow[]>

  pushToast: (kind: Toast['kind'], text: string) => void
  /** 提醒条：不自动消失，并带「打开任务」跳转 */
  pushReminder: (text: string, taskId: string) => void
  dismissToast: (id: string) => void
}

/** 依据当前视图与筛选条件构造查询（§4.1 组合筛选 + §4.2 明确规则） */
export function buildQuery(s: {
  view: ViewId
  search: string
  statusFilter: TaskStatus[]
  sortBy: NonNullable<TaskQuery['sortBy']>
  sortDesc: boolean
  overdueOnly: boolean
}): TaskQuery {
  const q: TaskQuery = {
    sortBy: s.sortBy,
    sortDesc: s.sortDesc,
    search: s.search.trim() || null,
    // 默认只显示未归档；「已完成」与「回收站」视图覆盖此设置
    statuses: s.statusFilter.length > 0 ? s.statusFilter : ['todo', 'doing', 'waiting', 'done'],
    // 这里**不设 limit**：分页由调用方在每次请求时明确给出
    // （整改前写死 500，导致第 501 条之后的任务界面永远看不到）。
  }

  const today = todayRange()
  switch (s.view) {
    case 'today':
      q.plannedFrom = today.start
      q.plannedTo = today.end
      break
    // ---------------- 周期任务视图 ----------------
    // 与上面的"今天/本周"是**不同维度**：这里按任务自身的周期跨度筛选，
    // 因此**不加时间范围条件**——"这周做完就行"的任务本来就没有具体计划日，
    // 加时间过滤会让它永远查不出来。
    case 'period-week':
      q.periodTypes = ['week']
      break
    case 'period-month':
      q.periodTypes = ['month']
      break
    case 'period-quarter':
      q.periodTypes = ['quarter']
      break
    case 'period-year':
      q.periodTypes = ['year']
      break
    case 'tomorrow': {
      const d = new Date()
      d.setDate(d.getDate() + 1)
      const r = todayRange(d)
      q.plannedFrom = r.start
      q.plannedTo = r.end
      break
    }
    case 'week': {
      const d = new Date()
      const day = (d.getDay() + 6) % 7 // 周一为 0
      const monday = new Date(d.getFullYear(), d.getMonth(), d.getDate() - day)
      const sunday = new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() + 6)
      const r1 = todayRange(monday)
      const r2 = todayRange(sunday)
      q.plannedFrom = r1.start
      q.plannedTo = r2.end
      break
    }
    case 'completed':
      q.statuses = ['done']
      q.sortBy = 'created'
      q.sortDesc = true
      break
    case 'trash':
      q.deletedOnly = true
      q.statuses = []
      break
    case 'inbox':
      // 收件箱 = 没有归属项目的未完成任务（§4.2 项目语义明确）。
      //
      // 这里**必须**用 withoutProject 而不是 `projectId = null`：
      // null 走 IPC 会变成 Rust 的 None，与"不限制项目"完全同义，
      // 收件箱就会退化成"所有未完成任务"。语义区分在 TaskQuery 里做了明确设计。
      q.withoutProject = true
      q.statuses = ['todo', 'doing', 'waiting']
      break
    case 'all':
      q.statuses = s.statusFilter.length > 0 ? s.statusFilter : ['todo', 'doing', 'waiting', 'done']
      break
    default:
      break
  }

  if (s.overdueOnly) q.overdueOnly = true
  return q
}

/**
 * 批量取子任务进度。
 *
 * 失败时返回空表而不是抛错：进度只是辅助信息，主数据仍然可用（§10）。
 * 列表页批量取一次，而不是让每张卡片各查一次。
 */
async function fetchProgress(
  tasks: Task[],
): Promise<Record<string, { total: number; done: number; percent: number | null }>> {
  if (tasks.length === 0) return {}
  try {
    const { subtaskProgressBatch } = await import('./organize-ipc')
    const rows = await subtaskProgressBatch(tasks.map((t) => t.id))
    return Object.fromEntries(
      rows.map((p) => [p.taskId, { total: p.total, done: p.done, percent: p.percent }]),
    )
  } catch {
    return {}
  }
}

/**
 * 搜索输入的防抖时长（毫秒）。
 *
 * 250ms 是"打字时不会每敲一个字都查一次库"与"停手就能看到结果"之间的折中。
 */
export const SEARCH_DEBOUNCE_MS = 250

/** 待执行的搜索刷新（模块级：同一时刻只允许一个） */
let searchReloadTimer: ReturnType<typeof setTimeout> | null = null

function scheduleSearchReload() {
  if (searchReloadTimer !== null) clearTimeout(searchReloadTimer)
  searchReloadTimer = setTimeout(() => {
    searchReloadTimer = null
    void useApp.getState().reload()
  }, SEARCH_DEBOUNCE_MS)
}

export const useApp = create<AppStore>((set, get) => ({
  tasks: [],
  progressMap: {},
  loadState: 'idle',
  loadError: null,
  totalCount: 0,
  hasMore: false,
  loadingMore: false,
  nextOffset: 0,
  queryGeneration: 0,
  overview: null,
  appInfo: null,
  dataPaths: null,

  view: 'today',
  search: '',
  statusFilter: [],
  sortBy: 'manual',
  sortDesc: false,
  overdueOnly: false,
  theme: 'system',
  toasts: [],

  init: async () => {
    set({ loadState: 'loading', loadError: null })
    try {
      const [info, paths] = await Promise.all([ipc.getAppInfo(), ipc.getDataPaths()])
      set({ appInfo: info, dataPaths: paths })
      await Promise.all([get().reload(), get().refreshOverview()])
    } catch (e) {
      const msg = e instanceof IpcError ? e.userMessage() : String(e)
      set({ loadState: 'error', loadError: msg })
    }
  },

  reload: async () => {
    const s = get()
    // 记下这次请求属于哪一代查询（第三轮任务书 §4.3）
    const generation = s.queryGeneration
    set({ loadState: 'loading', loadError: null })
    try {
      const query = buildQuery(s)
      // 列表与总数**同时**取，且用同一套条件（§10 要求条件完全一致）：
      // 总数决定"还有没有更多"，不能靠"这一页是否满"来猜。
      const [tasks, count] = await Promise.all([
        ipc.listTasks({ ...query, limit: PAGE_SIZE, offset: 0 }),
        ipc.countTasks(query),
      ])

      // 期间用户可能已经改了条件：这份结果属于上一代查询，必须丢弃。
      // 不加这一层就会出现"旧请求比新请求更晚返回，把新结果覆盖掉"。
      if (get().queryGeneration !== generation) return

      // 注意 `fetchProgress` 也是一次 await：**它之后必须再查一次代数**
      // （第三轮收口任务书 §9）。只在它之前检查会留下窗口：
      // 检查通过 → fetchProgress 挂起 → 用户切条件且新结果落地 →
      // 旧 fetchProgress 返回 → 旧 set 覆盖新结果。
      const progressMap = await fetchProgress(tasks)
      if (get().queryGeneration !== generation) return

      set({
        tasks,
        progressMap,
        totalCount: count.total,
        hasMore: tasks.length < count.total,
        nextOffset: tasks.length,
        loadingMore: false,
        loadState: 'ready',
        loadError: null,
      })
    } catch (e) {
      const msg = e instanceof IpcError ? e.userMessage() : String(e)
      set({ loadState: 'error', loadError: msg })
    }
  },

  loadMore: async () => {
    const s = get()
    // 已经在加载时不重复发（滚动会连续触发）
    if (s.loadingMore) return
    // 记下这次请求属于哪一代查询（第三轮任务书 §4.3）
    const generation = s.queryGeneration

    // 先按当前条件**刷新一次总数**再决定能不能继续加载。
    //
    // 为什么必须这样：`totalCount` 是上一次查询的快照。并发写入（别的窗口新建、
    // 提醒任务生成、回收站清空…）会让它过期，而过期会造出两种讨厌的状态：
    // ① 其实还有更多，却因为旧的 hasMore=false 而不再加载；
    // ② 界面渲染出「加载更多」，点下去 `loadMore` 却直接返回——按钮点了没反应。
    // 计数很便宜（一条 count 查询），拿它当"要不要继续"的唯一依据最稳。
    let total = s.totalCount
    try {
      total = (await ipc.countTasks(buildQuery(s))).total
    } catch {
      // 计数失败就用旧值继续，不让一次计数错误挡住翻页
    }
    // 计数期间条件可能已经变了：这份判断也作废，交给新的一代去处理
    if (get().queryGeneration !== generation) return
    if (s.tasks.length >= total) {
      set({ totalCount: total, hasMore: false, loadingMore: false })
      return
    }

    set({ loadingMore: true })
    try {
      const rows = await ipc.listTasks({
        ...buildQuery(s),
        limit: PAGE_SIZE,
        // 用**已加载条数**当偏移，而不是上次记下的 nextOffset：
        // 两者本应相等，但列表被别处改过时，实际长度才是真相。
        offset: s.tasks.length,
      })
      // 过期结果直接丢弃：绝不能把"A 条件的第二页"追加进"B 条件的结果"里
      if (get().queryGeneration !== generation) {
        set({ loadingMore: false })
        return
      }
      const seen = new Set(s.tasks.map((t) => t.id))
      const fresh = rows.filter((t) => !seen.has(t.id))
      // fetchProgress 同样是一次 await：写回前必须**再查一次**代数（收口任务书 §10）
      const freshProgress = await fetchProgress(fresh)
      if (get().queryGeneration !== generation) {
        set({ loadingMore: false })
        return
      }
      const tasks = [...s.tasks, ...fresh]
      set({
        tasks,
        progressMap: { ...s.progressMap, ...freshProgress },
        totalCount: total,
        nextOffset: tasks.length,
        // 这一页一条新的都没拿到（并发删除等）就停下，避免无限请求同一页
        hasMore: fresh.length > 0 && tasks.length < total,
        loadingMore: false,
      })
    } catch (e) {
      set({ loadingMore: false })
      if (get().queryGeneration === generation) {
        get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
      }
    }
  },

  refreshOverview: async () => {
    try {
      const r = todayRange()
      const overview = await ipc.getTodayOverview(r.start, r.end)
      set({ overview })
    } catch {
      // 概览失败不应让整页报错；保持上一次的值
    }
  },

  /**
   * 别的窗口改动了任务数据时调用（第三轮任务书 §5.4 的临时方案）。
   *
   * OFFSET 分页在数据集变化时天然不稳定：已经加载了 1..200，
   * 另一个窗口删掉第 50 条，再用 OFFSET 200 取下一页就会**漏掉**原来的第 201 条。
   * 彻底解决要换 keyset 分页；本轮先按任务书允许的方式处理：
   *
   * 1. `queryGeneration + 1` —— 让所有在飞的请求（尤其是 loadMore）作废，
   *    它们的结果回来时会被丢弃，不会追加到新数据上；
   * 2. `reload()` —— 从第一页重新取，等于清空已加载的后续页。
   */
  handleExternalChange: async () => {
    set({ queryGeneration: get().queryGeneration + 1 })
    await get().reload()
  },

  // 下面五个 action 都会改变查询条件。它们的共同点是：
  // **先把 queryGeneration +1，再重新查询**（第三轮任务书 §4.3）。
  // 加这一句之后，所有"上一代查询"的异步结果回来时都会自动作废。

  setView: (v) => {
    set({ view: v, queryGeneration: get().queryGeneration + 1 })
    void get().reload()
  },

  setSearch: (s) => {
    set({ search: s, queryGeneration: get().queryGeneration + 1 })
    // 输入即刷新（防抖），而不是只在回车时刷新。
    //
    // 为什么必须这样：分页之后 `tasks` 与 `totalCount` 是**同一套条件**下的结果，
    // 如果条件变了而结果没跟着变，界面就会拿旧的 totalCount 去算"还有多少条"，
    // 于是出现"搜索框里写着 A、列表和计数还是全量"的自相矛盾状态。
    // 任务书 §4.5 要求：条件一变就必须 offset 归零、列表换成新结果。
    scheduleSearchReload()
  },

  setStatusFilter: (s) => {
    set({ statusFilter: s, queryGeneration: get().queryGeneration + 1 })
    void get().reload()
  },

  setSort: (by, desc) => {
    set({
      sortBy: by,
      sortDesc: desc ?? false,
      queryGeneration: get().queryGeneration + 1,
    })
    void get().reload()
  },

  setOverdueOnly: (v) => {
    set({ overdueOnly: v, queryGeneration: get().queryGeneration + 1 })
    void get().reload()
  },

  setTheme: (t) => set({ theme: t }),

  toggleDone: async (id, done) => {
    try {
      await ipc.toggleTaskDone(id, done)
      await Promise.all([get().reload(), get().refreshOverview()])
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  duplicate: async (id) => {
    try {
      const r = await ipc.duplicateTask(id)
      await get().reload()
      // 附件不随副本复制，这一点必须告诉用户，不能静默丢掉
      if (r.skippedAttachments > 0) {
        get().pushToast('info', r.note)
      } else {
        get().pushToast('success', `已复制为「${r.title}」`)
      }
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  reorder: async (movedId, beforeId) => {
    try {
      await ipc.reorderTask(movedId, beforeId)
      await get().reload()
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  remove: async (id) => {
    try {
      await ipc.softDeleteTask(id)
      await Promise.all([get().reload(), get().refreshOverview()])
      get().pushToast('success', '已移入回收站，可随时恢复')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  restore: async (id) => {
    try {
      await ipc.restoreTask(id)
      await Promise.all([get().reload(), get().refreshOverview()])
      get().pushToast('success', '已恢复')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  purge: async (id) => {
    try {
      await ipc.purgeTask(id)
      await get().reload()
      get().pushToast('success', '已永久删除')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  preparePurge: async () => {
    // 与列表同一套条件 + 锁死"只看回收站"：确认数量与删除范围才是同一个集合
    const query: TaskQuery = { ...buildQuery(get()), deletedOnly: true }
    const preview = await ipc.preparePurgeDeleted(query)
    return { query, preview }
  },

  purgeAll: async (query, taskIds) => {
    try {
      const r = await ipc.commitPurgeDeleted(query, taskIds)
      await get().reload()
      get().pushToast('success', `已永久删除 ${r.purged} 项`)
    } catch (e) {
      const msg = e instanceof IpcError ? e.userMessage() : String(e)
      get().pushToast('error', msg)
      // 冲突意味着"内容已变化"：必须刷新，否则界面还停在旧数字上
      void get().reload()
    }
  },

  /** 打印 / PDF 报告数据：走列表同一套筛选，报告里能看到归属名称 */
  reportRows: async () => {
    const q = buildQuery(get())
    // 单页接口有 PAGE_MAX 上限，这里要 1000 条（历史用法）；需要全量请用 reportRowsAll
    return ipc.taskReport({ ...q, limit: 1000 })
  },

  pushToast: (kind, text) => {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    set({ toasts: [...get().toasts, { id, kind, text }] })
    // 错误停留更久，给用户时间读完恢复建议。
    // 用 globalThis 而不是 window：store 是纯状态机，不该依赖浏览器全局
    // （单元测试在 Node 环境里跑，那里没有 window）。
    globalThis.setTimeout(() => get().dismissToast(id), kind === 'error' ? 6000 : 3000)
  },

  pushReminder: (text, taskId) => {
    const id = `r-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    set({ toasts: [...get().toasts, { id, kind: 'reminder', text, taskId }] })
    // 提醒条不自动消失：用户可能不在电脑前，回来时仍要能看到并点进去
  },

  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}))
