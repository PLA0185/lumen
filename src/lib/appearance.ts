import { getCurrentWebview } from '@tauri-apps/api/webview'

let zoomRevision = 0
let zoomQueue = Promise.resolve()

export function applyAppearance(): Promise<void> {
  const savedScale = Number(localStorage.getItem('lumen.uiScale') ?? '1')
  const scale = Number.isFinite(savedScale) ? Math.min(1.5, Math.max(0.8, savedScale)) : 1
  const savedFont = Number.parseFloat(localStorage.getItem('lumen.fontSize') ?? '14')
  const font = Number.isFinite(savedFont) ? Math.min(20, Math.max(12, savedFont)) : 14
  document.documentElement.style.setProperty('--font-size-base', `${font}px`)
  document.documentElement.style.setProperty('--ui-scale', String(scale))
  // The viewport already has its own responsive layout and the WebView applies
  // the operating-system display scale. Only apply the user's explicit setting;
  // shrinking the whole app to fit a narrow window bypasses its CSS breakpoints.
  const zoom = scale
  if (!('__TAURI_INTERNALS__' in window)) {
    document.documentElement.style.zoom = String(zoom)
    document.documentElement.style.height = `${100 / zoom}%`
    return Promise.resolve()
  }
  // Resize/slider bursts only apply the newest requested scale, in order.
  const revision = ++zoomRevision
  const pending = zoomQueue.catch(() => {}).then(async () => {
    if (revision === zoomRevision) await getCurrentWebview().setZoom(zoom)
  })
  zoomQueue = pending
  return pending
}

export async function installAppearance(onError: (error: unknown) => void): Promise<() => void> {
  const refresh = () => { void applyAppearance().catch(onError) }
  const storage = (event: StorageEvent) => {
    if (event.key === null || event.key === 'lumen.uiScale' || event.key === 'lumen.fontSize') refresh()
  }
  window.addEventListener('storage', storage)
  // Re-read the user's saved scale on mount; resizing is handled by CSS
  // breakpoints and must not trigger a second, competing layout system.
  await applyAppearance()
  return () => window.removeEventListener('storage', storage)
}
