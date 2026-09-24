/**
 * 看板分页状态机的测试（第三轮整改任务书 §12 / §13）。
 *
 * 为什么这些用例必须存在：看板是 React 组件，而本仓库没有 jsdom /
 * testing-library（`vite.config.ts` 的 `test.environment` 是 `node`），
 * 写在组件里的分页逻辑**测不到**。所以这段逻辑被抽到 `./board-paging`
 * 这个不依赖 DOM 的状态机里，测试直接驱动它——测的就是组件真正在用的那份代码。
 *
 * 要防的是三类不会报错、只会让用户看到错界面的问题：
 *
 * - 别的窗口删了任务，看板还用旧 offset 翻页 → **漏项**；
 * - 在飞的旧请求回来后写回界面 → 旧数据覆盖新结果、或重复追加；
 * - 取到空页之后仍然认为 `hasMore` → 按钮永远在、点了没反应。
 *
 * 第三轮收口（任务书 §11 / §14 / §15 / §16）又补了两类：
 *
 * - **普通 `reload` 也要作废在飞的 `loadMore`**（拖拽改状态后重载，
 *   旧第二页回来时不得 append）；
 * - **同窗口事件**：主窗口在看板上用 QuickAdd 新建任务时，事件 `from` 与监听
 *   窗口标签都是 `main`，`bus.onTasksChanged` 默认会把它滤掉 → 看板不刷新。
 *   这条链（`bus` 的过滤分支 → `handleExternalChange` → 回到第一页）在这里
 *   真实跑一遍，`bus.ts` 依赖的 Tauri 全局与事件模块由用例替身提供。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createBoardPaging, nextBoardState } from './board-paging'
import type { TasksChangedPayload } from './bus'
import type { Task, TaskQuery } from './types'

/** 用例里的页大小取小值，断言才读得懂（组件里传的是 `PAGE_SIZE`） */
const TEST_PAGE_SIZE = 5

/** 造一批任务（分页逻辑只关心 id 的顺序，其余字段用最小可用值） */
function makeTasks(from: number, to: number): Task[] {
  const out: Task[] = []
  for (let i = from; i <= to; i++) {
    out.push({ id: `t-${i}`, title: `任务 ${i}`, status: 'todo' } as unknown as Task)
  }
  return out
}

/** 造一个可以手动决定何时完成的 promise（用来复现"旧请求后回来"的时序） */
function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}

/**
 * 一个受控的假后端：内在维护一份"数据库里的任务"，`listTasks` 按 offset/limit 切片。
 *
 * 为什么不用 `mockResolvedValueOnce` 逐次排队返回值：这套用例要演的是
 * "别的窗口删掉几条，OFFSET 窗口整体前移"，用一个真实存在的数据集切片，
 * 才真的能暴露漏项——手写返回值时，"漏了哪条"是照着期望编出来的，测不出东西。
 */
function makeHarness(rows: Task[], pageSize = TEST_PAGE_SIZE) {
  const backend = { rows: [...rows] }
  const listTasks = vi.fn(async (query: TaskQuery) => {
    const offset = query.offset ?? 0
    const limit = query.limit ?? backend.rows.length
    return backend.rows.slice(offset, offset + limit)
  })
  const countTasks = vi.fn(async () => ({ total: backend.rows.length }))
  const paging = createBoardPaging({
    listTasks,
    countTasks,
    query: () => ({ statuses: ['todo', 'doing', 'waiting', 'done'], sortBy: 'manual' }),
    pageSize,
    describeError: (e) => String(e),
  })
  return { paging, backend, listTasks, countTasks }
}

describe('nextBoardState（纯函数）', () => {
  it('replace 是整体替换，不是拼接，并按新总数重算 hasMore', () => {
    const prev = { tasks: makeTasks(1, 5), total: 12, hasMore: true }
    const next = nextBoardState(prev, { kind: 'replace', rows: makeTasks(100, 104), total: 5 })

    expect(next.tasks.map((t) => t.id)).toEqual(['t-100', 't-101', 't-102', 't-103', 't-104'])
    expect(next.total).toBe(5)
    expect(next.hasMore).toBe(false)
  })

  it('append 按 id 去重：相邻两页重叠的部分不会出现两次', () => {
    const prev = { tasks: makeTasks(1, 5), total: 12, hasMore: true }
    // 第二页与第一页有 2 条重叠（并发插入会让 OFFSET 窗口漂移）
    const next = nextBoardState(prev, { kind: 'append', rows: makeTasks(4, 10), total: 12 })

    const ids = next.tasks.map((t) => t.id)
    expect(ids).toEqual(['t-1', 't-2', 't-3', 't-4', 't-5', 't-6', 't-7', 't-8', 't-9', 't-10'])
    expect(new Set(ids).size).toBe(ids.length) // 无重复
    expect(next.hasMore).toBe(true)
  })

  it('append 拿到空页时停下，并且不再声称还有更多', () => {
    const prev = { tasks: makeTasks(1, 5), total: 100, hasMore: true }
    const next = nextBoardState(prev, { kind: 'append', rows: [], total: 100 })

    expect(next.tasks).toHaveLength(5) // 已加载的内容不动
    expect(next.hasMore).toBe(false) // 不会无限请求同一页
  })
})

describe('看板分页与跨窗口变化', () => {
  it('别的窗口删掉任务后重载第一页，再继续翻页：无重复、无遗漏、total 正确', async () => {
    const { paging, backend } = makeHarness(makeTasks(1, 12))

    await paging.reload()
    expect(paging.getState().tasks.map((t) => t.id)).toEqual([
      't-1',
      't-2',
      't-3',
      't-4',
      't-5',
    ])
    expect(paging.getState().total).toBe(12)
    expect(paging.getState().hasMore).toBe(true)

    // 先翻一页，让看板处于"已经加载了 10 条"的状态
    await paging.loadMore()
    expect(paging.getState().tasks).toHaveLength(10)

    // 另一个窗口删掉最前面的 3 条：OFFSET 分页的窗口整体前移。
    // 此时若继续用 offset 10 翻页，就会漏掉原来的第 11、12 条（现在的第 9、10 条）。
    backend.rows = backend.rows.filter((t) => !['t-1', 't-2', 't-3'].includes(t.id))
    await paging.handleExternalChange()

    const afterChange = paging.getState()
    expect(afterChange.tasks.map((t) => t.id)).toEqual([
      't-4',
      't-5',
      't-6',
      't-7',
      't-8',
    ]) // 回到第一页，offset 归零
    expect(afterChange.total).toBe(9)

    // 继续翻页直到加载完
    await paging.loadMore()

    const s = paging.getState()
    const ids = s.tasks.map((t) => t.id)
    expect(new Set(ids).size).toBe(ids.length) // 无重复
    expect(ids).toEqual(backend.rows.map((t) => t.id)) // 无遗漏：与库里现存的任务完全一致
    expect(s.total).toBe(9)
    expect(s.hasMore).toBe(false)
  })

  it('收到跨窗口变化时立刻清空已加载的页（offset 归零、计数重置）', async () => {
    const { paging } = makeHarness(makeTasks(1, 12))
    await paging.reload()
    await paging.loadMore()
    expect(paging.getState().tasks).toHaveLength(10)

    // 不 await：看的是"作废那一刻"的状态
    const pending = paging.handleExternalChange()
    expect(paging.getState().tasks).toEqual([])
    expect(paging.getState().total).toBe(0)
    expect(paging.getState().hasMore).toBe(false)
    expect(paging.getState().loadingMore).toBe(false)

    await pending
    expect(paging.getState().tasks).toHaveLength(TEST_PAGE_SIZE) // 又从第一页开始
  })

  it('loadMore 在飞时发生跨窗口变化：旧页结果必须被丢弃，且不会卡在「正在加载…」', async () => {
    const { paging, backend, listTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()

    // 让第二页的请求挂在半空中
    const slowPage = deferred<Task[]>()
    listTasks.mockImplementationOnce(() => slowPage.promise)
    const pending = paging.loadMore()
    await vi.waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))
    expect(paging.getState().loadingMore).toBe(true)

    // 别的窗口把后面的任务都删了，看板作废在飞请求并重载第一页
    backend.rows = makeTasks(1, 4)
    await paging.handleExternalChange()
    expect(paging.getState().tasks).toHaveLength(4)

    // 旧的那一页现在才回来：必须被丢弃，不能追加到新结果后面
    slowPage.resolve(makeTasks(6, 10))
    await pending

    const s = paging.getState()
    expect(s.tasks.map((t) => t.id)).toEqual(['t-1', 't-2', 't-3', 't-4'])
    expect(s.total).toBe(4)
    expect(s.loadingMore).toBe(false) // 不会卡在「正在加载…」
  })

  it('countTasks 与 listTasks 之间发生跨窗口变化：这一次翻页整份作废', async () => {
    const { paging, backend, listTasks, countTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()
    expect(listTasks).toHaveBeenCalledTimes(1) // 只有 reload 那次取第一页

    // 让"刷新真实总数"这一步挂住：翻页卡在还没发出 listTasks 的位置
    const slowCount = deferred<{ total: number }>()
    countTasks.mockImplementationOnce(() => slowCount.promise)
    const pending = paging.loadMore()
    await vi.waitFor(() => expect(countTasks).toHaveBeenCalledTimes(2))

    // 期间别的窗口改了数据 → 作废在飞请求，并从第一页重取
    backend.rows = makeTasks(100, 104)
    await paging.handleExternalChange()
    const callsAfterChange = listTasks.mock.calls.length

    // 旧的那次计数现在才回来：不能再用旧 offset 发一次列表请求
    slowCount.resolve({ total: 12 })
    await pending

    expect(listTasks.mock.calls.length).toBe(callsAfterChange)
    const s = paging.getState()
    expect(s.tasks.map((t) => t.id)).toEqual(['t-100', 't-101', 't-102', 't-103', 't-104'])
    expect(s.loadingMore).toBe(false)
  })

  it('取到空页就停下，不会无限请求同一页', async () => {
    const { paging, listTasks, countTasks } = makeHarness(makeTasks(1, 6))
    await paging.reload()
    expect(paging.getState().hasMore).toBe(true)

    // 计数说还有 100 条（快照过期），但这一页是空的（数据已被并发删掉）
    countTasks.mockResolvedValueOnce({ total: 100 })
    listTasks.mockResolvedValueOnce([])
    await paging.loadMore()

    expect(paging.getState().hasMore).toBe(false)

    const calls = listTasks.mock.calls.length
    await paging.loadMore() // 再点一次
    expect(listTasks.mock.calls.length).toBe(calls) // 不会再发请求
  })

  it('翻页失败时给出错误、已加载内容完好、不会卡在「正在加载…」', async () => {
    const { paging, listTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()

    listTasks.mockRejectedValueOnce(new Error('读取失败'))
    await paging.loadMore()

    const s = paging.getState()
    expect(s.tasks).toHaveLength(TEST_PAGE_SIZE) // 已加载的内容不动
    expect(s.loadingMore).toBe(false)
    expect(s.error).toBe('Error: 读取失败')
  })

  it('过期的 loadMore 不会擦掉新一代正在飞的「加载更多」', async () => {
    const { paging, backend, listTasks } = makeHarness(makeTasks(1, 20))
    await paging.reload() // 第一页 5 条，total 20

    // 第一次翻页挂在半空中
    const slowOld = deferred<Task[]>()
    listTasks.mockImplementationOnce(() => slowOld.promise)
    const oldCall = paging.loadMore()
    await vi.waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))

    // 别的窗口改了数据：作废在飞请求并重载第一页
    backend.rows = makeTasks(1, 20)
    await paging.handleExternalChange()
    expect(paging.getState().hasMore).toBe(true)

    // 用户又点了一次「加载更多」，这一次也挂在半空
    const slowNew = deferred<Task[]>()
    listTasks.mockImplementationOnce(() => slowNew.promise)
    const newCall = paging.loadMore()
    await vi.waitFor(() => expect(listTasks).toHaveBeenCalledTimes(4))
    expect(paging.getState().loadingMore).toBe(true)

    // 旧的那次现在才回来：结果丢弃，并且不能把新一代的「正在加载…」复位
    // （按钮若被复位就会重新可点，用户能对同一段 offset 再发一次请求）
    slowOld.resolve(makeTasks(6, 10))
    await oldCall
    expect(paging.getState().loadingMore).toBe(true)
    expect(paging.getState().tasks).toHaveLength(TEST_PAGE_SIZE)

    // 新一代正常完成
    slowNew.resolve(makeTasks(6, 10))
    await newCall
    expect(paging.getState().tasks).toHaveLength(10)
    expect(paging.getState().loadingMore).toBe(false)
  })

  it('卸载（dispose）之后回来的在飞请求不再写回状态', async () => {
    const { paging, backend, listTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()

    const slowPage = deferred<Task[]>()
    listTasks.mockImplementationOnce(() => slowPage.promise)
    const pending = paging.loadMore()
    await vi.waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))

    paging.dispose() // 组件卸载
    backend.rows = makeTasks(1, 2)
    slowPage.resolve(makeTasks(6, 10))
    await pending

    const s = paging.getState()
    expect(s.tasks).toHaveLength(TEST_PAGE_SIZE) // 没有被追加
    expect(s.tasks.map((t) => t.id)).toEqual(['t-1', 't-2', 't-3', 't-4', 't-5'])
  })
})

describe('普通 reload 与在飞的翻页', () => {
  it('普通 reload 会作废在飞的 loadMore：旧第二页不 append', async () => {
    const { paging, backend, listTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()
    expect(paging.getState().tasks).toHaveLength(TEST_PAGE_SIZE)

    // 用户点了「加载更多」，这一页挂在半空中
    const slowPage = deferred<Task[]>()
    listTasks.mockImplementationOnce(() => slowPage.promise)
    const pending = paging.loadMore()
    await vi.waitFor(() => expect(listTasks).toHaveBeenCalledTimes(2))
    expect(paging.getState().loadingMore).toBe(true)

    // 期间发生了一次**普通**重载：BoardView 里拖拽改状态成功后走的就是这条路径
    // （不是 handleExternalChange）。它同样必须作废在飞的翻页——
    // 否则旧第二页回来时会被去重后追加进新结果，界面上就多出一条过时任务。
    backend.rows = makeTasks(100, 104)
    await paging.reload()
    expect(paging.getState().tasks.map((t) => t.id)).toEqual([
      't-100',
      't-101',
      't-102',
      't-103',
      't-104',
    ])
    const callsAfterReload = listTasks.mock.calls.length

    // 旧第二页现在才回来：整份丢弃
    slowPage.resolve(makeTasks(6, 10))
    await pending

    const s = paging.getState()
    expect(s.tasks.map((t) => t.id)).toEqual(['t-100', 't-101', 't-102', 't-103', 't-104'])
    expect(s.total).toBe(5)
    expect(s.loadingMore).toBe(false) // 不会卡在「正在加载…」
    expect(listTasks.mock.calls.length).toBe(callsAfterReload) // 也不会补发一次请求
  })
})

/**
 * bus + 看板的联动（任务书 §11 / §16）。
 *
 * `bus.ts` 在 vitest 的 node 环境里本来跑不起来（没有 `window`，
 * `@tauri-apps/api/event` 也只在 WebView 里可用），所以这里用 `vi.stubGlobal`
 * 补一个最小的 Tauri 窗口环境、用 `vi.doMock` 把事件模块换成内存实现：
 * 跑的是 `bus.ts` **真实的**过滤分支，而不是把它的逻辑在测试里重抄一遍。
 *
 * 替身的 `emit` 会把事件也送给自己窗口的监听者——真实的 Tauri `emit` 就是这个
 * 行为（见 `bus.ts` 顶部注释），也正是"需要自我过滤 / 需要 includeSelf"的由来。
 */
describe('同窗口事件与 includeSelf（bus + 看板）', () => {
  let handlers: ((e: { payload: TasksChangedPayload }) => void)[] = []
  let emitted: TasksChangedPayload[] = []

  /** 从某个窗口发一条事件（送给当前窗口的全部监听者） */
  function emitFrom(from: string): void {
    const payload: TasksChangedPayload = { from }
    for (const h of [...handlers]) h({ payload })
  }

  /** 等 bus 注册好监听：它内部是异步 import，注册发生在微任务之后 */
  async function waitForListener(): Promise<void> {
    await vi.waitFor(() => {
      expect(handlers).toHaveLength(1)
    })
  }

  beforeEach(() => {
    handlers = []
    emitted = []
    // bus.ts 用 `__TAURI_INTERNALS__` 判断"在不在 Tauri 里"，并从里面取窗口标签
    // 判断"这条事件是不是自己发的"。当前窗口固定为主窗口 main。
    vi.stubGlobal('window', {
      __TAURI_INTERNALS__: { metadata: { currentWebview: { label: 'main' } } },
    })
    vi.doMock('@tauri-apps/api/event', () => ({
      emit: async (_event: string, payload: TasksChangedPayload) => {
        emitted.push(payload)
        emitFrom(payload.from) // 真实 emit 也会送回发送者自己的窗口
      },
      listen: async (
        _event: string,
        handler: (e: { payload: TasksChangedPayload }) => void,
      ) => {
        handlers.push(handler)
        return () => {
          handlers = handlers.filter((h) => h !== handler)
        }
      },
    }))
  })

  afterEach(() => {
    vi.doUnmock('@tauri-apps/api/event')
    vi.unstubAllGlobals()
  })

  it('默认忽略本窗口自己发出的事件，别的窗口的事件仍然回调', async () => {
    const bus = await import('./bus')
    const cb = vi.fn()
    const off = bus.onTasksChanged(cb)
    await waitForListener()

    // 载荷里的 from 就是过滤依据：先确认它确实带的是本窗口标签
    await bus.notifyTasksChanged()
    expect(emitted).toEqual([{ from: 'main' }])
    expect(cb).not.toHaveBeenCalled() // ← 自己发的事件被忽略（主列表依赖这条）

    emitFrom('floating')
    expect(cb).toHaveBeenCalledTimes(1)

    off()
    emitFrom('floating')
    expect(cb).toHaveBeenCalledTimes(1) // 退订之后不再回调
  })

  it('includeSelf=true 时，本窗口自己发出的事件也会回调', async () => {
    const bus = await import('./bus')
    const cb = vi.fn()
    bus.onTasksChanged(cb, { includeSelf: true })
    await waitForListener()

    await bus.notifyTasksChanged() // 主窗口自己发的
    expect(cb).toHaveBeenCalledTimes(1)

    emitFrom('quick-add') // 别的窗口照旧回调
    expect(cb).toHaveBeenCalledTimes(2)
  })

  it('同窗口事件（includeSelf）会让看板刷新：作废在飞翻页并回到第一页', async () => {
    const bus = await import('./bus')
    const { paging, backend } = makeHarness(makeTasks(1, 12))
    await paging.reload()
    await paging.loadMore()
    expect(paging.getState().tasks).toHaveLength(10)
    expect(paging.getState().total).toBe(12)

    // 这就是 BoardView 里接监听的那一行
    const off = bus.onTasksChanged(() => void paging.handleExternalChange(), {
      includeSelf: true,
    })
    await waitForListener()

    // 主窗口自己在看板上用 QuickAdd 新建了一条：数据变了，事件由 main 发出
    backend.rows = [...backend.rows, ...makeTasks(13, 13)]
    await bus.notifyTasksChanged()

    await vi.waitFor(() => expect(paging.getState().total).toBe(13))
    const s = paging.getState()
    expect(s.tasks.map((t) => t.id)).toEqual(['t-1', 't-2', 't-3', 't-4', 't-5']) // 回到第一页
    expect(s.loadingMore).toBe(false)
    off()
  })

  it('默认监听下同窗口事件不会刷新看板（主列表要的正是这个行为）', async () => {
    const bus = await import('./bus')
    const { paging, backend, listTasks } = makeHarness(makeTasks(1, 12))
    await paging.reload()
    await paging.loadMore()
    expect(paging.getState().tasks).toHaveLength(10)

    bus.onTasksChanged(() => void paging.handleExternalChange()) // 不传 includeSelf
    await waitForListener()
    const calls = listTasks.mock.calls.length

    backend.rows = [...backend.rows, ...makeTasks(13, 13)]
    await bus.notifyTasksChanged()
    await new Promise((resolve) => setTimeout(resolve, 0)) // 放行可能存在的异步回调

    expect(listTasks.mock.calls.length).toBe(calls) // 没有重新查询
    expect(paging.getState().tasks).toHaveLength(10) // 也没有被清空
    expect(paging.getState().total).toBe(12)
  })
})
