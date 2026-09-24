/**
 * 任务列表分页的状态机测试（第二轮整改任务书 §4 / §14.2）。
 *
 * 为什么这些用例必须存在：分页最容易出的错不是"请求发不出去"，
 * 而是**状态之间的配合**——
 *
 * - 条件变了却没把 offset 归零 → 新旧条件的结果混在一页里；
 * - 追加时不去重 → 任务在列表里出现两次；
 * - 空页仍然认为 `hasMore` → 无限重复请求同一页。
 *
 * 这些都不会报错，只会让用户看到"少了/重复了"的列表。
 *
 * 这里把 `./ipc` 整个替换成受控的假实现：store 是纯状态机，
 * 只要 IPC 的契约不变，测试测的就是真实生效的那段代码。
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Task } from './types'

class FakeIpcError extends Error {
  readonly code: string
  readonly hint: string | null
  constructor(code: string, message: string, hint: string | null = null) {
    super(message)
    this.code = code
    this.hint = hint
  }
  userMessage(): string {
    return this.hint ? `${this.message}\n${this.hint}` : this.message
  }
}

const listTasks = vi.fn()
const countTasks = vi.fn()

vi.mock('./ipc', () => ({
  IpcError: FakeIpcError,
  listTasks: (...args: unknown[]) => listTasks(...args),
  countTasks: (...args: unknown[]) => countTasks(...args),
}))

const { useApp, PAGE_SIZE, SEARCH_DEBOUNCE_MS } = await import('./store')

/** 造一批任务（只需要 id，其余字段用最小可用值） */
function makeTasks(from: number, to: number): Task[] {
  const out: Task[] = []
  for (let i = from; i <= to; i++) {
    out.push({ id: `t-${i}`, title: `任务 ${i}`, status: 'todo' } as unknown as Task)
  }
  return out
}

/** 把 store 恢复到干净的初始状态 */
function resetStore() {
  useApp.setState({
    tasks: [],
    progressMap: {},
    loadState: 'idle',
    loadError: null,
    totalCount: 0,
    hasMore: false,
    loadingMore: false,
    nextOffset: 0,
    view: 'all',
    search: '',
    statusFilter: [],
    sortBy: 'manual',
    sortDesc: false,
    overdueOnly: false,
  })
}

beforeEach(() => {
  // 必须用 resetAllMocks 而不是 clearAllMocks：
  // 后者只清调用记录，**不清 `mockResolvedValueOnce` 的排队实现**，
  // 上一个用例没消费完的返回值会漏到下一个用例里（写这个测试时真的踩到了）。
  vi.resetAllMocks()
  resetStore()
})

describe('列表分页', () => {
  it('reload 只取第一页，并同时拿到总数', async () => {
    listTasks.mockResolvedValue(makeTasks(1, PAGE_SIZE))
    countTasks.mockResolvedValue({ total: 1200 })

    await useApp.getState().reload()

    const s = useApp.getState()
    expect(s.tasks).toHaveLength(PAGE_SIZE)
    expect(s.totalCount).toBe(1200)
    expect(s.hasMore).toBe(true)
    expect(s.nextOffset).toBe(PAGE_SIZE)

    // 分页参数必须显式：整改前这里是写死的 limit: 500 且没有 offset
    expect(listTasks.mock.calls[0]?.[0]).toMatchObject({ limit: PAGE_SIZE, offset: 0 })
    // 总数查询用同一套条件（多传 limit 不影响计数）
    expect(countTasks.mock.calls[0]?.[0]).not.toHaveProperty('offset')
  })

  it('总数不超过一页时不会声称还有更多', async () => {
    listTasks.mockResolvedValue(makeTasks(1, 30))
    countTasks.mockResolvedValue({ total: 30 })

    await useApp.getState().reload()

    expect(useApp.getState().hasMore).toBe(false)
    expect(useApp.getState().totalCount).toBe(30)
  })

  it('loadMore 按 offset 追加下一页，并且不产生重复项', async () => {
    listTasks.mockResolvedValueOnce(makeTasks(1, PAGE_SIZE))
    countTasks.mockResolvedValue({ total: 1200 })
    await useApp.getState().reload()

    // 第二页故意与第一页有 10 条重叠（模拟并发插入导致的边界漂移）
    listTasks.mockResolvedValueOnce(makeTasks(PAGE_SIZE - 9, PAGE_SIZE * 2))
    await useApp.getState().loadMore()

    const s = useApp.getState()
    const ids = s.tasks.map((t) => t.id)
    expect(new Set(ids).size).toBe(ids.length) // 无重复
    expect(s.tasks[0]?.id).toBe('t-1') // 顺序保持：新数据追加在后面
    expect(listTasks.mock.calls[1]?.[0]).toMatchObject({ offset: PAGE_SIZE })
    expect(s.nextOffset).toBe(ids.length)
  })

  it('加载到最后一页后 hasMore 变为 false', async () => {
    listTasks.mockResolvedValueOnce(makeTasks(1, 100))
    countTasks.mockResolvedValue({ total: 150 })
    await useApp.getState().reload()
    expect(useApp.getState().hasMore).toBe(true)

    listTasks.mockResolvedValueOnce(makeTasks(101, 150))
    await useApp.getState().loadMore()

    const s = useApp.getState()
    expect(s.tasks).toHaveLength(150)
    expect(s.hasMore).toBe(false)
  })

  it('没有更多时 loadMore 不再发请求', async () => {
    listTasks.mockResolvedValue(makeTasks(1, 10))
    countTasks.mockResolvedValue({ total: 10 })
    await useApp.getState().reload()

    await useApp.getState().loadMore()
    expect(listTasks).toHaveBeenCalledTimes(1) // 只有 reload 那一次
  })

  it('空页会让分页停下来，不会无限请求同一页', async () => {
    listTasks.mockResolvedValueOnce(makeTasks(1, PAGE_SIZE))
    countTasks.mockResolvedValue({ total: 5000 })
    await useApp.getState().reload()

    // 后端返回空页（例如数据被并发删掉了）
    listTasks.mockResolvedValueOnce([])
    await useApp.getState().loadMore()

    expect(useApp.getState().hasMore).toBe(false)
    listTasks.mockResolvedValueOnce([])
    await useApp.getState().loadMore()
    expect(listTasks).toHaveBeenCalledTimes(2) // 停下来了
  })

  it('筛选条件变化时 offset 归零、列表被整体替换', async () => {
    listTasks.mockResolvedValueOnce(makeTasks(1, PAGE_SIZE))
    countTasks.mockResolvedValue({ total: 1200 })
    await useApp.getState().reload()
    listTasks.mockResolvedValueOnce(makeTasks(PAGE_SIZE + 1, PAGE_SIZE * 2))
    await useApp.getState().loadMore()
    expect(useApp.getState().nextOffset).toBe(PAGE_SIZE * 2)

    // 换搜索词 → 必须从第一页重新开始
    listTasks.mockResolvedValueOnce(makeTasks(9000, 9000))
    countTasks.mockResolvedValueOnce({ total: 1 })
    useApp.setState({ search: '只有一条' })
    await useApp.getState().reload()

    const s = useApp.getState()
    expect(s.tasks.map((t) => t.id)).toEqual(['t-9000']) // 旧结果被替换，不是拼接
    expect(s.nextOffset).toBe(1)
    expect(s.hasMore).toBe(false)
    expect(listTasks.mock.calls.at(-1)?.[0]).toMatchObject({ offset: 0 })
  })

  it('加载下一页失败时给出提示，且不破坏已加载的内容', async () => {
    listTasks.mockResolvedValueOnce(makeTasks(1, PAGE_SIZE))
    countTasks.mockResolvedValue({ total: 1200 })
    await useApp.getState().reload()

    listTasks.mockRejectedValueOnce(new FakeIpcError('database', '读取失败', '请重试'))
    await useApp.getState().loadMore()

    const s = useApp.getState()
    expect(s.tasks).toHaveLength(PAGE_SIZE) // 已加载内容完好
    expect(s.loadingMore).toBe(false) // 不会卡在"加载中"
    expect(s.toasts.at(-1)?.kind).toBe('error')
  })
})

describe('搜索条件变化', () => {
  it('输入搜索词会自动刷新（不必按回车），并且从第一页重新开始', async () => {
    vi.useFakeTimers()
    try {
      listTasks.mockResolvedValue(makeTasks(1, PAGE_SIZE))
      countTasks.mockResolvedValue({ total: 1200 })
      await useApp.getState().reload()
      listTasks.mockResolvedValueOnce(makeTasks(PAGE_SIZE + 1, PAGE_SIZE * 2))
      await useApp.getState().loadMore()
      expect(useApp.getState().nextOffset).toBe(PAGE_SIZE * 2)

      // 输入搜索词：等防抖结束后必须自动重新查询（offset 归零）
      listTasks.mockResolvedValue(makeTasks(9000, 9000))
      countTasks.mockResolvedValue({ total: 1 })
      useApp.getState().setSearch('只有一条')
      vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS + 10)
      await vi.waitFor(() => {
        expect(useApp.getState().tasks.map((t) => t.id)).toEqual(['t-9000'])
      })
      expect(useApp.getState().nextOffset).toBe(1)
      expect(listTasks.mock.calls.at(-1)?.[0]).toMatchObject({
        offset: 0,
        search: '只有一条',
      })
    } finally {
      vi.useRealTimers()
    }
  })
})
