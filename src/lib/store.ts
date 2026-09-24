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
    limit: 500,
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

export const useApp = create<AppStore>((set, get) => ({
  tasks: [],
  progressMap: {},
  loadState: 'idle',
  loadError: null,
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
      const tasks = await ipc.listTasks(buildQuery(s))

      // 批量取子任务进度。失败不应让整个列表报错——
      // 进度只是辅助信息，主数据仍然可用。
      let progressMap: Record<string, { total: number; done: number; percent: number | null }> = {}
      if (tasks.length > 0) {
        try {
          const { subtaskProgressBatch } = await import('./organize-ipc')
          const rows = await subtaskProgressBatch(tasks.map((t) => t.id))
          progressMap = Object.fromEntries(
            rows.map((p) => [p.taskId, { total: p.total, done: p.done, percent: p.percent }]),
          )
        } catch {
          progressMap = {}
        }
      }

      set({ tasks, progressMap, loadState: 'ready', loadError: null })
    } catch (e) {
      const msg = e instanceof IpcError ? e.userMessage() : String(e)
      set({ loadState: 'error', loadError: msg })
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

  setSearch: (s) => set({ search: s }),

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

  pushToast: (kind, text) => {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    set({ toasts: [...get().toasts, { id, kind, text }] })
    // 错误停留更久，给用户时间读完恢复建议
    window.setTimeout(() => get().dismissToast(id), kind === 'error' ? 6000 : 3000)
  },

  pushReminder: (text, taskId) => {
    const id = `r-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    set({ toasts: [...get().toasts, { id, kind: 'reminder', text, taskId }] })
    // 提醒条不自动消失：用户可能不在电脑前，回来时仍要能看到并点进去
  },

  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}))
