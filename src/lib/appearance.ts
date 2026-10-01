import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'

let windowScale = 1
let zoomRevision = 0
let zoomQueue = Promise.resolve()

export function applyAppearance(): Promise<void> {
  const savedScale = Number(localStorage.getItem('lumen.uiScale') ?? '1')
  const scale = Number.isFinite(savedScale) ? Math.min(1.5, Math.max(0.8, savedScale)) : 1
  const savedFont = Number.parseFloat(localStorage.getItem('lumen.fontSize') ?? '14')
  const font = Number.isFinite(savedFont) ? Math.min(20, Math.max(12, savedFont)) : 14
  document.documentElement.style.setProperty('--font-size-base', `${font}px`)
  document.documentElement.style.setProperty('--ui-scale', String(scale))
  const zoom = Math.round(scale * windowScale * 10000) / 10000
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
  const disposers: Array<() => void> = []
  const refresh = () => { void applyAppearance().catch(onError) }
  const storage = (event: StorageEvent) => {
    if (event.key === null || event.key === 'lumen.uiScale' || event.key === 'lumen.fontSize') refresh()
  }
  if ('__TAURI_INTERNALS__' in window) {
    const win = getCurrentWindow()
    if (win.label === 'main') {
      let dpi = await win.scaleFactor()
      const adapt = (physicalWidth: number) => {
        // Use native window size and OS DPI, so WebView zoom cannot feed back into sizing.
        windowScale = Math.max(0.75, Math.min(1.1, physicalWidth / dpi / 1280))
      }
      disposers.push(await win.onResized(({ payload }) => { adapt(payload.width); refresh() }))
      disposers.push(await win.onScaleChanged(({ payload }) => {
        dpi = payload.scaleFactor
        adapt(payload.size.width)
        refresh()
      }))
      adapt((await win.innerSize()).width)
    }
  } else {
    const resize = () => {
      if (document.body.dataset.window === 'main') windowScale = Math.max(0.75, Math.min(1.1, window.innerWidth / 1280))
      refresh()
    }
    window.addEventListener('resize', resize)
    disposers.push(() => window.removeEventListener('resize', resize))
    resize()
  }
  window.addEventListener('storage', storage)
  disposers.push(() => window.removeEventListener('storage', storage))
  await applyAppearance()
  return () => disposers.forEach(dispose => dispose())
}
