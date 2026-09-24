/**
 * Provider 默认值的一致性测试（第二轮整改任务书 §14.1）。
 *
 * 背景（真实缺陷）：后端已经把 OpenAI 的默认模型改成**空字符串**
 * （未经验证不填），但前端 `ai-ipc.ts` 里另有一份 `PROVIDER_DEFAULT_MODEL`，
 * OpenAI 那格还写着 `gpt-6-astra`（Azure 的模型 ID）。
 * 用户在界面上切到 OpenAI 后会被自动填入这个未经验证的模型名，
 * 而**保存不会报错**——漂移是静默的。
 *
 * 修复方式是把默认值收敛到后端一处，前端只做映射。因此这里的测试
 * 不再比对"两份常量是否相同"（那是治标），而是锁定映射行为本身：
 * 前端拿到什么就用什么，绝不自己补一个默认值。
 */

import { describe, it, expect } from 'vitest'
import {
  configForProvider,
  configFromDefaults,
  findDefaults,
  type ProviderDefaults,
} from './ai-ipc'

/** 模拟后端 `ai_provider_defaults` 的返回（顺序与 Rust `Provider::ALL` 一致） */
const TABLE: ProviderDefaults[] = [
  {
    provider: 'deep_seek',
    label: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-flash',
    timeoutSeconds: 60,
    maxOutputTokens: 2048,
    dataPolicyNote: '按 DeepSeek 官方政策……',
    modelMustBeChosen: false,
    keyEntry: 'ai-deepseek',
  },
  {
    provider: 'open_ai',
    label: 'OpenAI',
    // 关键：后端刻意留空
    baseUrl: 'https://api.openai.com/v1',
    model: '',
    timeoutSeconds: 60,
    maxOutputTokens: 2048,
    dataPolicyNote: '按 OpenAI 官方政策……',
    modelMustBeChosen: true,
    keyEntry: 'ai-openai',
  },
  {
    provider: 'claude',
    label: 'Anthropic Claude',
    baseUrl: 'https://api.anthropic.com',
    model: 'claude-sonnet-5',
    timeoutSeconds: 60,
    maxOutputTokens: 2048,
    dataPolicyNote: '按 Anthropic 官方政策……',
    modelMustBeChosen: false,
    keyEntry: 'ai-claude',
  },
  {
    provider: 'custom',
    label: '自定义兼容服务',
    baseUrl: '',
    model: '',
    timeoutSeconds: 60,
    maxOutputTokens: 2048,
    dataPolicyNote: '自定义服务的隐私政策由其提供方决定……',
    modelMustBeChosen: true,
    keyEntry: 'ai-custom',
  },
]

describe('Provider 默认值只来自后端', () => {
  it('切换提供商不会带入上一个服务商的模型与地址', () => {
    const fromDeepSeek = configForProvider(TABLE, 'deep_seek')
    expect(fromDeepSeek).toMatchObject({
      baseUrl: 'https://api.deepseek.com',
      model: 'deepseek-flash',
    })

    // 切到 OpenAI：地址换成 OpenAI 的，模型必须是空的
    const toOpenAi = configForProvider(TABLE, 'open_ai')
    expect(toOpenAi?.baseUrl).toBe('https://api.openai.com/v1')
    expect(toOpenAi?.model).toBe('')
    expect(toOpenAi?.model).not.toBe(fromDeepSeek?.model)
  })

  it('后端没给模型时前端就是空的，绝不自己猜一个模型名', () => {
    const c = configForProvider(TABLE, 'open_ai')
    expect(c?.model).toBe('')
    // 这一条挡住的正是 `gpt-6-astra` 那个未经验证的 Azure 模型 ID
    expect(c?.model).not.toContain('gpt')
  })

  it('表里没有的提供商返回 null，而不是退回某个兜底默认值', () => {
    expect(findDefaults(TABLE, 'open_ai')?.label).toBe('OpenAI')
    expect(configForProvider([], 'open_ai')).toBeNull()
  })

  it('已保存密钥的状态在切换提供商时保留', () => {
    expect(configForProvider(TABLE, 'open_ai', true)?.hasApiKey).toBe(true)
    expect(configForProvider(TABLE, 'open_ai')?.hasApiKey).toBe(false)
  })

  it('超时与输出上限也取自后端，而不是前端写死', () => {
    const custom = TABLE.map((d) =>
      d.provider === 'claude' ? { ...d, timeoutSeconds: 120, maxOutputTokens: 4096 } : d,
    )
    const c = configForProvider(custom, 'claude')
    expect(c?.timeoutSeconds).toBe(120)
    expect(c?.maxOutputTokens).toBe(4096)
  })

  it('数据政策文案取自后端（三家姿态不同，不能统一）', () => {
    const notes = TABLE.map((d) => d.dataPolicyNote)
    expect(new Set(notes).size).toBe(TABLE.length)
    expect(findDefaults(TABLE, 'deep_seek')?.dataPolicyNote).toContain('DeepSeek')
  })

  it('configFromDefaults 只做字段搬运，不新增字段', () => {
    const d = findDefaults(TABLE, 'custom')!
    expect(Object.keys(configFromDefaults(d)).sort()).toEqual(
      ['baseUrl', 'hasApiKey', 'maxOutputTokens', 'model', 'provider', 'timeoutSeconds'].sort(),
    )
  })
})
