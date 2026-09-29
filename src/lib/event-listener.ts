import { listen, type Event } from '@tauri-apps/api/event'

/** React cleanup is synchronous; native listener registration is asynchronous. */
export function listenEvent<T>(
  name: string,
  callback: (event: Event<T>) => void,
  onError?: (error: unknown) => void,
): () => void {
  let disposed = false
  let unlisten: (() => void) | undefined
  void listen<T>(name, (event) => {
    if (!disposed) callback(event)
  })
    .then((off) => {
      if (disposed) off()
      else unlisten = off
    })
    .catch((error: unknown) => {
      if (!disposed) onError?.(error)
    })
  return () => {
    disposed = true
    unlisten?.()
    unlisten = undefined
  }
}
