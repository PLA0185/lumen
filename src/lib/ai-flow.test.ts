// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { aiGenerateFlow, type ProviderConfig } from './ai-ipc'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
afterEach(() => { Reflect.deleteProperty(window, '__TAURI_INTERNALS__'); vi.clearAllMocks() })
it('流程生成仅发送本次材料引用的资源，且不调用保存命令', async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} })
  const id = '00000000-0000-7000-8000-000000000001'
  const config: ProviderConfig = { provider: 'custom', baseUrl: 'http://localhost:11434/v1', model: 'vision-test', timeoutSeconds: 30, maxOutputTokens: 2048, hasApiKey: true }
  const text = `用户：先核对，再发货。![截图](lumen-asset:${id}) [重复](lumen-asset:${id})`
  vi.mocked(invoke).mockResolvedValue({ title: '流程', steps: [] })
  await aiGenerateFlow(config, text)
  expect(invoke).toHaveBeenCalledOnce()
  expect(invoke).toHaveBeenCalledWith('ai_generate_flow', { config, input: { text, assetIds: [id] } })
})
