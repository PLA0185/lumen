/**
 * 看板分页状态机（第三轮整改任务书 §12 / §13）。
 *
 * ## 为什么它必须从组件里搬出来
 *
 * 看板自己分页（每页 `PAGE_SIZE` + 「加载更多」），而 OFFSET 分页在数据集
 * 变化时天然不稳定：已经加载了 1..200，另一个窗口删掉第 50 条，
 * 再用 `offset: 200` 取下一页就会**漏掉**原来的第 201 条。
 * 所以别的窗口一改数据，看板必须作废在飞请求并从第一页重取——
 * 这与 `App.tsx` 里主列表走 `handleExternalChange` 是同一套做法。
 *
 * 但看板的这套逻辑原本写在组件里，而本仓库没有 jsdom / testing-library
 * （`vite.config.ts` 的 `test.environment` 是 `node`），组件内的代码
 * **测不到**。于是把"分页 + 代际作废"整段搬到这里：这里不碰 DOM、
 * 不 import `./ipc`（数据源由调用方注入），可以被 vitest 直接驱动。
 *
 * ## 两条时间线（分工不重叠）
 *
 * - `generation`（查询代数）：**唯一的代际作废依据**。只有"别的窗口改了数据"
 *   与"组件卸载"会让它 +1。`reload` / `loadMore` 在开始时**记下**当时的代数，
 *   每次 `await` 回来（尤其是最后一次）都拿它比对，对不上就整份丢弃——
 *   任务书要求的是"记下并比对"，不是"每次加载都换代"。
 * - `loadToken`（加载序号）：**只管 `loadingMore` 的归属**，即"这一次加载还是不是
 *   最新的一次加载"。没有它，一个过期的 `loadMore` 回来时会把新一代正在飞的
 *   「正在加载…」擦掉，按钮重新可点，就能对同一段 offset 重复发请求。
 *
 * 两道检查都不重叠、各有一个测试盯着（见 `board-paging.test.ts`）。
 *
 * ## 已知边界（如实记录）
 *
 * `reload` 不会作废在飞的 `loadMore`：拖拽改状态后的重载若与一次在飞的
 * 「加载更多」重叠，那一页仍会被去重后追加进来，可能带上一条过时的任务。
 * 下一次重载即修正。彻底解决要换 keyset 分页（与主列表同一条待办）。
 */

import type { Task, TaskQuery } from './types'

/** 分页数据：纯函数 `nextBoardState` 只处理这一层 */
export interface BoardPagingData {
  /** 已经加载出来的任务（看板会把它们全部渲染成卡片） */
  tasks: Task[]
  /** 当前条件下的总条数（后端 count）——看板同样不得静默只显示一部分 */
  total: number
  /**
   * 是否还有下一页。
   *
   * 为什么不能只看 `tasks.length < total`（第三轮任务书 §6）：
   * `total` 是快照，并发删除之后它会过期，于是"总数说还有、下一页却是空的"，
   * 按钮就会永远留着一个点了没反应的入口。这里改用与主列表一致的
   * "取到空页就停 + 空页后刷新一次真实计数"。
   */
  hasMore: boolean
}

/** 看板分页的完整快照（BoardView 直接渲染它） */
export interface BoardPageState extends BoardPagingData {
  /** 第一页是否正在加载（此时界面显示骨架屏） */
  loading: boolean
  /** 「加载更多」是否正在飞 */
  loadingMore: boolean
  /** 加载失败时的可读原因 */
  error: string | null
}

/** 一次加载回来后要写回的分页数据 */
export type BoardPageUpdate =
  /** 第一页：整体替换（重载、外部变化之后都走这里） */
  | { kind: 'replace'; rows: Task[]; total: number }
  /** 下一页：追加并去重 */
  | { kind: 'append'; rows: Task[]; total: number }

/**
 * 由"当前数据 + 这一页的返回"算出"新的数据"——纯函数，没有副作用。
 *
 * 三件事都在这里定死，避免散在组件与状态机两处各写一遍：
 *
 * 1. `replace` 是**整体替换**而不是拼接（换条件/重载后旧的页必须消失）；
 * 2. `append` 必须**按 id 去重**：并发插入会让相邻两页出现重叠，
 *    不去重用户就会在列表里看到同一个任务两次；
 * 3. 停下来的条件用"这一页有没有拿到新东西"（`fresh.length > 0`）而不是
 *    "这一页是否取满"，这样并发删除导致的空页会立刻停下，不会无限请求。
 */
export function nextBoardState(prev: BoardPagingData, update: BoardPageUpdate): BoardPagingData {
  if (update.kind === 'replace') {
    return {
      tasks: update.rows,
      total: update.total,
      hasMore: update.rows.length < update.total,
    }
  }

  const seen = new Set(prev.tasks.map((t) => t.id))
  const fresh = update.rows.filter((t) => !seen.has(t.id))
  const tasks = [...prev.tasks, ...fresh]
  return {
    tasks,
    total: update.total,
    hasMore: fresh.length > 0 && tasks.length < update.total,
  }
}

/** 状态机需要的数据源与配置（真实实现是 `ipc.listTasks` / `ipc.countTasks`） */
export interface BoardPagingDeps {
  listTasks: (query: TaskQuery) => Promise<Task[]>
  countTasks: (query: TaskQuery) => Promise<{ total: number }>
  /** 看板的查询条件（不显示归档与回收站内容） */
  query: () => TaskQuery
  /** 每页多少条（`PAGE_SIZE`） */
  pageSize: number
  /** 把异常转成界面文案（组件传的是 `IpcError` 的 userMessage） */
  describeError: (e: unknown) => string
}

/** 看板分页状态机对外的接口 */
export interface BoardPaging {
  /** 当前快照（未变化时返回同一个对象，`useSyncExternalStore` 依赖这一点） */
  getState: () => BoardPageState
  /** 订阅快照变化，返回取消订阅的函数 */
  subscribe: (listener: () => void) => () => void
  /** 重新加载第一页，并重新 count */
  reload: () => Promise<void>
  /** 追加下一页（先刷新真实总数再决定要不要加载） */
  loadMore: () => Promise<void>
  /** 别的窗口改了任务数据：作废在飞请求，从第一页重取 */
  handleExternalChange: () => Promise<void>
  /** 本地乐观更新（拖拽改状态），不改变代数 */
  applyLocalUpdate: (updater: (tasks: Task[]) => Task[]) => void
  /** 设置/清除错误提示 */
  setError: (message: string | null) => void
  /** 组件卸载：作废所有在飞请求 */
  dispose: () => void
}

/** 建一个看板分页状态机。每个看板实例一个，卸载时调用 `dispose`。 */
export function createBoardPaging(deps: BoardPagingDeps): BoardPaging {
  /** 查询代数：外部变化 / 卸载时 +1，让所有在飞请求作废 */
  let generation = 0
  /** 加载序号：每次 loadMore 开始时 +1，用来识别"我是不是最新那次翻页" */
  let loadToken = 0
  const listeners = new Set<() => void>()

  let state: BoardPageState = {
    tasks: [],
    total: 0,
    hasMore: false,
    // 初始就是加载中：挂载后马上会 reload，先渲染骨架屏比先闪一下空看板好
    loading: true,
    loadingMore: false,
    error: null,
  }

  function set(patch: Partial<BoardPageState>): void {
    state = { ...state, ...patch }
    // 复制一份再遍历：监听者可能在回调里退订
    for (const listener of [...listeners]) listener()
  }

  function getState(): BoardPageState {
    return state
  }

  function subscribe(listener: () => void): () => void {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  }

  async function reload(): Promise<void> {
    // **记下**这次请求属于哪一代查询（任务书 §12.1）：
    // 只有外部变化与卸载会让代数 +1，所以代数变了就说明这份结果已经过期。
    const gen = generation

    set({ loading: true, loadingMore: false, error: null })
    try {
      const query = deps.query()
      // 与主列表同一个策略：列表与总数**同时**取，且用同一套条件（§4.2 不得静默截断）
      const [rows, count] = await Promise.all([
        deps.listTasks({ ...query, limit: deps.pageSize, offset: 0 }),
        deps.countTasks(query),
      ])

      // **最后一次 await 之后再检查一次代数**：期间别的窗口改过数据（或组件已卸载），
      // 这份结果就属于上一代，必须整份丢弃，否则旧数据会覆盖新结果。
      if (generation !== gen) return

      set({
        ...nextBoardState(state, { kind: 'replace', rows, total: count.total }),
        loading: false,
        loadingMore: false,
        error: null,
      })
    } catch (e) {
      // 失败同样要看代数：过期的失败不该在新一代的界面上报错
      if (generation !== gen) return
      set({ loading: false, error: deps.describeError(e) })
    }
  }

  async function loadMore(): Promise<void> {
    // 已经在加载、首屏还在加载、或没有下一页时都不重复发请求
    if (state.loading || state.loadingMore || !state.hasMore) return

    // 记下代数（外部变化会让它作废）与加载序号（更新的翻页会让它作废）
    const gen = generation
    const token = ++loadToken
    const current = () => generation === gen && loadToken === token
    /**
     * 丢弃过期结果。
     *
     * `loadingMore` 只在**没有更新的翻页顶掉它**时才复位：否则一个过期的
     * `loadMore` 回来时会擦掉新一代正在飞的「正在加载…」，按钮重新可点，
     * 就又能对同一段 offset 重复发请求。
     */
    const discard = () => {
      if (loadToken === token) set({ loadingMore: false })
    }

    set({ loadingMore: true, error: null })
    // 本次翻页的基准：**已加载条数就是下一页的 offset**
    // （不能只看总数，列表被别处改过时，实际长度才是真相）
    const loaded = state.tasks
    try {
      // 先刷新一次真实总数：`total` 是上一次查询的快照，并发写入后可能已经过期
      let total = state.total
      try {
        total = (await deps.countTasks(deps.query())).total
      } catch {
        // 计数失败就用旧值继续，不挡住翻页
      }
      // countTasks 与 listTasks 之间的这次 await 也要核对：
      // 期间数据被别处改过的话，这份"总数"已经不作数，交给新一代去处理
      if (!current()) {
        discard()
        return
      }
      if (loaded.length >= total) {
        set({ total, hasMore: false, loadingMore: false })
        return
      }

      const rows = await deps.listTasks({
        ...deps.query(),
        limit: deps.pageSize,
        offset: loaded.length,
      })
      // **最后一次 await 之后再检查一次代数**：
      // 过期的页绝不能追加进新一代的数据里
      if (!current()) {
        discard()
        return
      }

      set({
        ...nextBoardState(state, { kind: 'append', rows, total }),
        loadingMore: false,
        error: null,
      })
    } catch (e) {
      if (!current()) {
        discard()
        return
      }
      // 失败不能卡在「正在加载…」，已加载的内容也不动
      set({ loadingMore: false, error: deps.describeError(e) })
    }
  }

  /**
   * 别的窗口改动了任务数据时调用（第三轮任务书 §5.4 的临时方案，看板侧补齐）。
   *
   * 彻底解决要换 keyset 分页；本轮按任务书允许的方式处理：
   *
   * 1. `generation + 1` —— 让所有在飞的请求（尤其是 `loadMore`）作废，
   *    它们的结果回来时会被丢弃，不会追加到新数据上；
   * 2. 清空已加载的页（offset 随之归零）并重置计数；
   * 3. `reload()` —— 从第一页重新取，并重新 count。
   */
  async function handleExternalChange(): Promise<void> {
    generation += 1
    set({ tasks: [], total: 0, hasMore: false, loadingMore: false })
    await reload()
  }

  function applyLocalUpdate(updater: (tasks: Task[]) => Task[]): void {
    // 只改数据、不动代数：这不是一次新查询，在飞的请求不算过期
    set({ tasks: updater(state.tasks) })
  }

  function setError(message: string | null): void {
    set({ error: message })
  }

  function dispose(): void {
    // 代数 +1：卸载之后回来的在飞请求会被判定为过期，不再写回任何状态
    generation += 1
  }

  return {
    getState,
    subscribe,
    reload,
    loadMore,
    handleExternalChange,
    applyLocalUpdate,
    setError,
    dispose,
  }
}
