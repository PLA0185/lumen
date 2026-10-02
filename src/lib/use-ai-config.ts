import { useCallback, useEffect, useRef, useState } from 'react'
import { aiGetConfig, type ProviderConfig } from './ai-ipc'
import { onDataChanged } from './data-change'
import { IpcError } from './ipc'

/** Saved configuration shared by AI entry points; late reads cannot overwrite newer ones. */
export function useAiConfig() {
  const [config, setConfig] = useState<ProviderConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [configError, setConfigError] = useState('')
  const generation = useRef(0)
  const reload = useCallback(async () => {
    const request = ++generation.current
    setLoading(true)
    setConfigError('')
    try {
      const saved = await aiGetConfig()
      if (request === generation.current) setConfig(saved)
    } catch (e) {
      if (request === generation.current) {
        setConfig(null)
        setConfigError(e instanceof IpcError ? e.userMessage() : String(e))
      }
    } finally {
      if (request === generation.current) setLoading(false)
    }
  }, [])
  useEffect(() => {
    const requests = generation
    const refresh = () => { void reload() }
    const off = onDataChanged(['aiConfig'], refresh)
    window.addEventListener('focus', refresh)
    refresh()
    return () => { off(); window.removeEventListener('focus', refresh); requests.current++ }
  }, [reload])
  return { config, loading, configError, reload }
}
