import * as updater from './update-ipc'

export type UpdatePhase = 'idle' | 'checking' | 'latest' | 'available' | 'installing' | 'failed'

export interface UpdateState {
  phase: UpdatePhase
  info: updater.UpdateInfo | null
  error: string | null
  progress: updater.UpdateProgress | null
  checkedAt: string | null
}

const initialState: UpdateState = {
  phase: 'idle',
  info: null,
  error: null,
  progress: null,
  checkedAt: null,
}

let state = initialState
let updateHandle: updater.UpdateHandle = null
let checkPromise: Promise<void> | null = null
let installPromise: Promise<void> | null = null
const listeners = new Set<() => void>()

function publish(next: UpdateState) {
  state = next
  for (const listener of listeners) listener()
}

export function getUpdateState(): UpdateState {
  return state
}

export function subscribeUpdateState(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function timeNow(): string {
  return new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
}

/** The update check and its handle outlive Settings/About tab unmounts. */
export function ensureUpdateChecked(): Promise<void> {
  if (state.phase !== 'idle') return checkPromise ?? Promise.resolve()
  return checkForUpdates()
}

export function checkForUpdates(): Promise<void> {
  if (installPromise) return Promise.resolve()
  if (checkPromise) return checkPromise

  publish({ ...state, phase: 'checking', info: null, error: null, progress: null })
  checkPromise = (async () => {
    try {
      const result = await updater.checkUpdate()
      updateHandle = result.handle
      publish({
        ...state,
        phase: result.info ? 'available' : 'latest',
        info: result.info,
        error: null,
        progress: null,
      })
    } catch (error) {
      updateHandle = null
      publish({ ...state, phase: 'failed', error: errorText(error), progress: null })
    } finally {
      publish({ ...state, checkedAt: timeNow() })
      checkPromise = null
    }
  })()
  return checkPromise
}

export function installAvailableUpdate(): Promise<void> {
  if (installPromise) return installPromise
  const handle = updateHandle
  if (!handle) {
    publish({ ...state, phase: 'failed', error: '更新信息已失效，请重新检查更新。' })
    return Promise.resolve()
  }

  publish({ ...state, phase: 'installing', error: null, progress: null })
  installPromise = (async () => {
    try {
      await updater.downloadAndInstall(handle, progress => {
        publish({ ...state, phase: 'installing', progress })
      })
      publish({
        ...state,
        phase: 'failed',
        error: '更新已下载完成，安装程序应当已经启动。若界面没有关闭，请手动重启 Lumen。',
      })
    } catch (error) {
      publish({ ...state, phase: 'failed', error: errorText(error) })
    } finally {
      installPromise = null
    }
  })()
  return installPromise
}

export function reportUpdateFailure(message: string): void {
  publish({ ...state, phase: 'failed', error: message })
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
