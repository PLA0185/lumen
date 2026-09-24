/**
 * 跨窗口变更广播。
 *
 * ## 为什么需要它
 *
 * 主窗口、悬浮今日窗、快速添加窗是**三个独立的 WebView**，各自持有
 * 自己的 React 状态。没有广播时，在悬浮窗里勾掉一个任务，主窗口要等到
 * 下次手动刷新才会变；主窗口新建的任务，悬浮窗最多要 30 秒才出现。
 * 这种"看到的是旧的"最容易被当成数据没保存。
 *
 * ## 为什么用前端 emit 而不是后端
 *
 * `emit` 会广播给**所有窗口**（包括发送者自己，所以监听时要忽略自己触发的
 * 刷新，否则会来回刷）。由前端在写操作成功后发一条就够，
 * 不需要在每个 Rust 命令里都加一行 AppHandle 事件。
 *
 * 忽略自己这条规则有例外：**自己窗口内就有独立状态**的消费者必须收到
 * 同窗口的变更，否则会停在旧数据上（看板就是这种情况，见 `onTasksChanged`）。
 * 所以这条规则做成参数而不是写死，默认值保持"忽略自己"。
 *
 * ## 事件不可用时
 *
 * 在浏览器里预览（没有 `__TAURI_INTERNALS__`）时静默降级：
 * 仍然可以开发界面，只是没有跨窗口同步。
 */

const EVENT = 'tasks-changed'

/** 事件载荷：谁改的（用来避免自己刷新自己，造成死循环） */
export interface TasksChangedPayload {
  /** 发起变更的窗口标签 */
  from: string
}

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 取当前窗口标签（main / floating / quick-add） */
export function windowLabel(): string {
  try {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__?: {
          metadata?: { currentWebview?: { label?: string }; currentWindow?: { label?: string } }
        }
      }
    ).__TAURI_INTERNALS__
    return (
      internals?.metadata?.currentWebview?.label ??
      internals?.metadata?.currentWindow?.label ??
      'main'
    )
  } catch {
    return 'main'
  }
}

/** 广播"任务数据变了"。写操作成功后调用。 */
export async function notifyTasksChanged(): Promise<void> {
  if (!inTauri()) return
  try {
    const { emit } = await import('@tauri-apps/api/event')
    await emit(EVENT, { from: windowLabel() } satisfies TasksChangedPayload)
  } catch {
    // 广播失败不应影响写操作本身：数据已经落库了
  }
}

/**
 * 监听"任务数据变了"。
 *
 * 默认只回调**别的窗口**发起的变更（`includeSelf` 缺省为 `false`）：
 * 发起方在写操作成功后自己就会刷新，再收一次等于重复请求，
 * 在"写 → 刷新 → 又写"的路径上还可能绕成事件环。
 *
 * `includeSelf: true` 是给**自己窗口内还有一份独立状态**的消费者用的：
 * 看板有自己独立的分页状态（`BoardView` 用的 `board-paging` 状态机），
 * 主窗口在看板上用 QuickAdd 新建任务时，事件 `from` 与监听窗口标签都是 `main`，
 * 默认过滤会把这条事件吞掉 → 看板一直停在旧数据上（漏掉刚建的任务）。
 * 传 `includeSelf: true` 就收得到同窗口的变更，结果由那条独立状态自己去重取。
 * 主列表（`store.handleExternalChange`）继续用默认值，行为完全不变。
 *
 * 返回取消监听的函数。
 */
export function onTasksChanged(cb: () => void, opts?: { includeSelf?: boolean }): () => void {
  const includeSelf = opts?.includeSelf ?? false
  if (!inTauri()) return () => {}
  let unlisten: (() => void) | undefined
  let disposed = false
  void (async () => {
    try {
      const { listen } = await import('@tauri-apps/api/event')
      const off = await listen<TasksChangedPayload>(EVENT, (e) => {
        // 载荷缺失 from 时无法判断来源，按"不是自己"处理：宁可多刷一次，不漏刷
        if (!includeSelf && e.payload?.from === windowLabel()) return
        cb()
      })
      if (disposed) off()
      else unlisten = off
    } catch {
      // 监听不可用时保持"只能靠定时刷新"，不影响核心功能
    }
  })()
  return () => {
    disposed = true
    unlisten?.()
  }
}
