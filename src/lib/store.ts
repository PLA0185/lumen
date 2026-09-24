/**
 * 全局状态（zustand）。
 *
 * 设计取舍：状态只保存"从后端读到的真相 + 界面态"，
 * 不做乐观更新以外的缓存推导，避免出现"界面显示与数据库不一致"。
 * 所有写操作都先落库、再用后端返回值刷新本地状态（§1 要求真实持久化闭环）。
 */

import { create } from 'zustand'
import * as ipc from './ipc'
import * as bus from './bus'
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
  purgeAll: () => Promise<void>
  /** 取打印 / PDF 报告数据（与当前列表同一套筛选条件） */
  reportRows: () => Promise<ipc.TaskReportRow[]>
  /** 取**完整**报告数据（分页读全，§7 要求不得静默截断） */
  reportRowsAll: () => Promise<ipc.ReportPage>

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
    set({ loadState: 'loading', loadError: null })
    try {
      const query = buildQuery(s)
      // 列表与总数**同时**取，且用同一套条件（§10 要求条件完全一致）：
      // 总数决定"还有没有更多"，不能靠"这一页是否满"来猜。
      const [tasks, count] = await Promise.all([
        ipc.listTasks({ ...query, limit: PAGE_SIZE, offset: 0 }),
        ipc.countTasks(query),
      ])

      set({
        tasks,
        progressMap: await fetchProgress(tasks),
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
    // 已经在加载、或已经没有更多时什么都不做：
    // 滚动触发会连续调用，这里不加锁会打出重复请求（§4.5 禁止新旧结果混在一起）
    if (s.loadingMore || !s.hasMore) return
    set({ loadingMore: true })
    try {
      const rows = await ipc.listTasks({
        ...buildQuery(s),
        limit: PAGE_SIZE,
        offset: s.nextOffset,
      })
      const seen = new Set(s.tasks.map((t) => t.id))
      const fresh = rows.filter((t) => !seen.has(t.id))
      const tasks = [...s.tasks, ...fresh]
      set({
        tasks,
        progressMap: { ...s.progressMap, ...(await fetchProgress(fresh)) },
        nextOffset: tasks.length,
        // 以**总数**为准判断是否还有更多；但如果这一页一条新的都没拿到
        // （并发删除等），必须停下来，否则会无限请求同一页。
        hasMore: fresh.length > 0 && tasks.length < s.totalCount,
        loadingMore: false,
      })
    } catch (e) {
      set({ loadingMore: false })
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
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

  setView: (v) => {
    set({ view: v })
    void get().reload()
  },

  setSearch: (s) => {
    set({ search: s })
    // 输入即刷新（防抖），而不是只在回车时刷新。
    //
    // 为什么必须这样：分页之后 `tasks` 与 `totalCount` 是**同一套条件**下的结果，
    // 如果条件变了而结果没跟着变，界面就会拿旧的 totalCount 去算"还有多少条"，
    // 于是出现"搜索框里写着 A、列表和计数还是全量"的自相矛盾状态。
    // 任务书 §4.5 要求：条件一变就必须 offset 归零、列表换成新结果。
    scheduleSearchReload()
  },

  setStatusFilter: (s) => {
    set({ statusFilter: s })
    void get().reload()
  },

  setSort: (by, desc) => {
    set({ sortBy: by, sortDesc: desc ?? false })
    void get().reload()
  },

  setOverdueOnly: (v) => {
    set({ overdueOnly: v })
    void get().reload()
  },

  setTheme: (t) => set({ theme: t }),

  toggleDone: async (id, done) => {
    try {
      await ipc.toggleTaskDone(id, done)
      await Promise.all([get().reload(), get().refreshOverview()])
      void bus.notifyTasksChanged()
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  duplicate: async (id) => {
    try {
      const r = await ipc.duplicateTask(id)
      await get().reload()
      void bus.notifyTasksChanged()
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
      void bus.notifyTasksChanged()
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  remove: async (id) => {
    try {
      await ipc.softDeleteTask(id)
      await Promise.all([get().reload(), get().refreshOverview()])
      void bus.notifyTasksChanged()
      get().pushToast('success', '已移入回收站，可随时恢复')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  restore: async (id) => {
    try {
      await ipc.restoreTask(id)
      await Promise.all([get().reload(), get().refreshOverview()])
      void bus.notifyTasksChanged()
      get().pushToast('success', '已恢复')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  purge: async (id) => {
    try {
      await ipc.purgeTask(id)
      await get().reload()
      void bus.notifyTasksChanged()
      get().pushToast('success', '已永久删除')
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  purgeAll: async () => {
    try {
      const r = await ipc.purgeAllDeleted()
      await get().reload()
      void bus.notifyTasksChanged()
      get().pushToast('success', `已永久删除 ${r.purged} 项`)
    } catch (e) {
      get().pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    }
  },

  /** 打印 / PDF 报告数据：走列表同一套筛选，报告里能看到归属名称 */
  reportRows: async () => {
    const q = buildQuery(get())
    // 报告是给用户留档的，不该像列表那样只取前 500 条
    return ipc.taskReport({ ...q, limit: 1000 })
  },

  /**
   * 完整报告数据（§7）。
   *
   * 不再自己设 limit：后端按 500 条一页读到取完，超过 1000 条也不会被截断。
   * 返回的 `truncated` 交给界面提示，而不是悄悄少给用户数据。
   */
  reportRowsAll: async () => {
    const q = buildQuery(get())
    // 分页由后端负责，这里把单页 limit/offset 清掉，避免影响后端的分页循环
    const { limit: _limit, offset: _offset, ...rest } = q
    return ipc.taskReportAll(rest)
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
