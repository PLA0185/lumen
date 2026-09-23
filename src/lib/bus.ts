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
 * 监听"任务数据变了"，回调只会在**别的窗口**发起时触发。
 *
 * 返回取消监听的函数。
 */
export function onTasksChanged(cb: () => void): () => void {
  if (!inTauri()) return () => {}
  let unlisten: (() => void) | undefined
  let disposed = false
  void (async () => {
    try {
      const { listen } = await import('@tauri-apps/api/event')
      const off = await listen<TasksChangedPayload>(EVENT, (e) => {
        if (e.payload?.from === windowLabel()) return
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
