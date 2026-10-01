import { invoke } from '@tauri-apps/api/core'
import { isTauri } from './ipc'

let pending: Promise<void> = Promise.resolve()

/** Enable the native Alt+Space guard only while the canvas owns keyboard panning. */
export function setCanvasInputActive(active: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve()
  // Order hover/focus/cleanup transitions even when a previous IPC call is slow.
  // Recover the queue after a failure, while returning that failure to its caller.
  const update = pending.catch(() => undefined).then(() => invoke<void>('canvas_input_set_active', { active }))
  pending = update
  return update
}
