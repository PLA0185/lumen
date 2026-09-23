/**
 * AI 面板（任务书 §6）。
 *
 * 包含两部分：
 * 1. **配置**：提供商、Base URL、模型、密钥、超时、输出上限、连接测试。
 * 2. **能力入口**：整理、拆解、排程、复盘。四项能力全部**只产生预览**，
 *    真正的写入必须由用户在 `DiffPreviewDialog` 中确认。
 *
 * 关于密钥：界面上只显示"已配置 / 未配置"，输入框默认留空表示
 * "不改动已保存的密钥"。密钥通过 keyring 存进 Windows 凭据管理器，
 * **不会**出现在数据库、备份或日志里（§6 / §10）。
 */

import { useCallback, useEffect, useState } from 'react'
import * as ai from '../lib/ai-ipc'
import { IpcError } from '../lib/ipc'
import { useApp } from '../lib/store'
import { DiffPreviewDialog } from './DiffPreviewDialog'
import type { AiProvider, DiffPreview, ProviderConfig, ReviewResult } from '../lib/ai-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

export function AiPanel() {
  const pushToast = useApp((s) => s.pushToast)
  const [cfg, setCfg] = useState<ProviderConfig | null>(null)
  const [apiKey, setApiKey] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [models, setModels] = useState<string[]>([])
  const [modelHint, setModelHint] = useState<string | null>(null)

  // 能力输入
  const [organizeText, setOrganizeText] = useState('')
  const [minutesPerDay, setMinutesPerDay] = useState(120)
  const [preview, setPreview] = useState<DiffPreview | null>(null)
  const [lastAction, setLastAction] = useState<(() => void) | null>(null)
  const [review, setReview] = useState<ReviewResult | null>(null)

  const reload = useCallback(async () => {
    try {
      const c = await ai.aiGetConfig()
      setCfg(c ?? defaultConfig('deep_seek'))
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const defaultConfig = (p: AiProvider): ProviderConfig => ({
    provider: p,
    baseUrl: ai.PROVIDER_DEFAULT_BASE[p],
    model: ai.PROVIDER_DEFAULT_MODEL[p],
    timeoutSeconds: 60,
    maxOutputTokens: 2048,
    hasApiKey: false,
  })

  const patch = (p: Partial<ProviderConfig>) => {
    setCfg((c) => (c ? { ...c, ...p } : c))
  }

  /** 切换提供商时同步默认值，避免残留上一个服务商的地址与模型名 */
  const switchProvider = (p: AiProvider) => {
    patch({
      provider: p,
      baseUrl: ai.PROVIDER_DEFAULT_BASE[p],
      model: ai.PROVIDER_DEFAULT_MODEL[p],
    })
    setModels([])
    setModelHint(null)
  }

  const save = async () => {
    if (!cfg) return
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      // 空字符串表示"不改动"；用户明确点清除时才传空串过去
      const saved = await ai.aiSetConfig(cfg, apiKey.trim() ? apiKey.trim() : null)
      setCfg(saved)
      setApiKey('')
      setNotice(
        saved.hasApiKey
          ? '已保存。密钥存储在 Windows 凭据管理器中，不会写入数据库或备份。'
          : '已保存配置。尚未设置 API Key。',
      )
      pushToast('success', 'AI 配置已保存')
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const clearKey = async () => {
    if (!cfg) return
    if (!window.confirm('清除已保存的 API Key？\n\n清除后 AI 功能将不可用，需要重新填写。')) return
    try {
      await ai.aiClearKey(cfg.provider)
      await reload()
      pushToast('success', '已清除密钥')
    } catch (e) {
      setError(errText(e))
    }
  }

  const test = async () => {
    if (!cfg) return
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const msg = await ai.aiTestConnection(cfg)
      setNotice(msg)
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const loadModels = async () => {
    if (!cfg) return
    setBusy(true)
    setModelHint(null)
    try {
      const r = await ai.aiListModels(cfg)
      setModels(r.models)
      if (!r.ok) {
        setModelHint(
          `${r.reason ?? '无法获取模型列表'}${r.hint ? `\n${r.hint}` : ''}\n${
            r.fallback ?? '可直接手动填写模型名称。'
          }`,
        )
      } else if (r.models.length === 0) {
        setModelHint('服务商没有返回任何模型，可直接手动填写模型名称。')
      }
    } catch (e) {
      setModelHint(errText(e))
    } finally {
      setBusy(false)
    }
  }

  /** 统一的"生成预览"包装：记录重试动作以便对话框内"重新生成" */
  const runPreview = async (fn: () => Promise<DiffPreview>, retry: () => void) => {
    setBusy(true)
    setError(null)
    try {
      const p = await fn()
      setPreview(p)
      setLastAction(() => retry)
      // 有 error 时也展示预览——用户需要看到具体是什么问题（§6）
      if (p.items.length === 0) {
        pushToast('info', 'AI 没有提出可执行的改动，可在预览中查看原因')
      }
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const canUse = Boolean(cfg?.hasApiKey)

  return (
    <>
      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable" style={{ whiteSpace: 'pre-line' }}>
            {error}
          </span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      )}
      {notice && (
        <div className="alert alert--ok" role="status">
          <span className="selectable" style={{ whiteSpace: 'pre-line' }}>
            {notice}
          </span>
        </div>
      )}

      {/* ---------------- 配置 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">AI 服务配置</h3>
        <p className="setgroup__desc">
          配置完全可选。<strong>不配置时，任务管理、重复规则、提醒、统计等全部功能都能正常使用。</strong>
          AI 只用于辅助整理、拆解、排程与复盘。
        </p>

        {cfg && (
          <>
            <div className="formgrid">
              <label className="formrow">
                <span className="formlabel">服务商</span>
                <select
                  className="input"
                  value={cfg.provider}
                  onChange={(e) => switchProvider(e.target.value as AiProvider)}
                >
                  {(Object.keys(ai.PROVIDER_LABELS) as AiProvider[]).map((p) => (
                    <option key={p} value={p}>
                      {ai.PROVIDER_LABELS[p]}
                    </option>
                  ))}
                </select>
              </label>

              <label className="formrow">
                <span className="formlabel">模型</span>
                <input
                  className="input selectable"
                  value={cfg.model}
                  list="ai-models"
                  placeholder="模型 ID"
                  onChange={(e) => patch({ model: e.target.value })}
                />
                <datalist id="ai-models">
                  {models.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </label>
            </div>

            <div className="formrow">
              <span className="formlabel">Base URL</span>
              <input
                className="input selectable"
                value={cfg.baseUrl}
                placeholder="https://…"
                onChange={(e) => patch({ baseUrl: e.target.value })}
              />
              <p className="setgroup__hint" style={{ marginTop: 4 }}>
                注意：DeepSeek 的地址<strong>不含 /v1</strong>（官方文档未提供该路径）。
                非本机地址必须使用 https，否则密钥会在网络上明文传输。
              </p>
            </div>

            <div className="formrow">
              <span className="formlabel">
                API Key
                {cfg.hasApiKey && <span className="chip chip--ok">已配置</span>}
              </span>
              <input
                type="password"
                className="input selectable"
                value={apiKey}
                placeholder={cfg.hasApiKey ? '已保存（留空表示不修改）' : '粘贴你的 API Key'}
                autoComplete="off"
                onChange={(e) => setApiKey(e.target.value)}
              />
              <p className="setgroup__hint" style={{ marginTop: 4 }}>
                密钥保存在 Windows 凭据管理器中，<strong>不写入数据库、不进入备份、不记入日志</strong>。
              </p>
            </div>

            <div className="formgrid">
              <label className="formrow">
                <span className="formlabel">超时（秒）</span>
                <input
                  type="number"
                  className="input"
                  min={5}
                  max={600}
                  value={cfg.timeoutSeconds}
                  onChange={(e) =>
                    patch({ timeoutSeconds: Number(e.target.value) || 60 })
                  }
                />
              </label>
              <label className="formrow">
                <span className="formlabel">单次输出上限（tokens）</span>
                <input
                  type="number"
                  className="input"
                  min={1}
                  max={32000}
                  value={cfg.maxOutputTokens}
                  onChange={(e) =>
                    patch({ maxOutputTokens: Number(e.target.value) || 2048 })
                  }
                />
              </label>
            </div>
            <p className="setgroup__hint">
              输出上限用于控制单次调用的规模。API 费用由你自己的服务商账户产生，
              Lumen 无法代你计费，也无法限制你的账户支出。
            </p>

            <div className="setactions">
              <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void save()}>
                {busy ? '处理中…' : '保存配置'}
              </button>
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy || !cfg.hasApiKey}
                onClick={() => void test()}
                title={cfg.hasApiKey ? undefined : '请先保存 API Key'}
              >
                测试连接
              </button>
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy || !cfg.hasApiKey}
                onClick={() => void loadModels()}
              >
                获取模型列表
              </button>
              {cfg.hasApiKey && (
                <button type="button" className="btn btn--danger" disabled={busy} onClick={() => void clearKey()}>
                  清除密钥
                </button>
              )}
            </div>

            {modelHint && (
              <p className="setgroup__hint" style={{ whiteSpace: 'pre-line' }}>
                {modelHint}
              </p>
            )}

            {/* 数据政策提示：三家姿态不同，文案由后端给出（§6） */}
            <div className="alert alert--warn" role="note" style={{ marginTop: 10 }}>
              <span>
                <strong>数据使用提示：</strong>
                {providerPolicyNote(cfg.provider)}
              </span>
            </div>
          </>
        )}
      </div>

      {/* ---------------- 能力 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">AI 能力</h3>
        <p className="setgroup__desc">
          以下每一项都<strong>只生成预览</strong>。你需要在预览里逐条核对并确认后，
          改动才会写入数据库。AI 无法自行删除、完成或改写你的任何任务。
        </p>

        {!canUse && (
          <div className="alert alert--warn" role="note">
            <span>请先在上方配置并保存 API Key，之后这里的功能才可用。</span>
          </div>
        )}

        {/* ---- 整理 ---- */}
        <div className="formrow">
          <span className="formlabel">把一段文字整理成任务</span>
          <textarea
            className="input input--area selectable"
            rows={4}
            value={organizeText}
            placeholder={'粘贴一段会议记录、待办想法或聊天内容，例如：\n下周三前把季度报告初稿发给我\n周五下午和设计确认改版方案'}
            aria-label="需要整理的文本"
            disabled={!canUse || busy}
            onChange={(e) => setOrganizeText(e.target.value)}
          />
          <div className="setactions">
            <button
              type="button"
              className="btn btn--primary"
              disabled={!canUse || busy || !organizeText.trim()}
              onClick={() =>
                void runPreview(
                  () => ai.aiOrganize(cfg!, organizeText),
                  () =>
                    void runPreview(
                      () => ai.aiOrganize(cfg!, organizeText),
                      () => {},
                    ),
                )
              }
            >
              {busy ? '生成中…' : '整理成候选任务'}
            </button>
            <span className="setgroup__hint" style={{ margin: 0 }}>
              默认<strong>不发送</strong>任务备注正文与附件内容。
            </span>
          </div>
        </div>

        {/* ---- 排程 ---- */}
        <div className="formrow" style={{ marginTop: 14 }}>
          <span className="formlabel">生成每日安排</span>
          <div className="rulerow">
            <label className="field">
              每天可用
              <input
                type="number"
                className="input input--compact"
                min={15}
                max={1440}
                value={minutesPerDay}
                aria-label="每天可用分钟数"
                disabled={!canUse || busy}
                onChange={(e) => setMinutesPerDay(Math.max(15, Number(e.target.value) || 120))}
              />
              分钟
            </label>
            <button
              type="button"
              className="btn btn--ghost"
              disabled={!canUse || busy}
              onClick={() =>
                void runPreview(
                  () => ai.aiPlan(cfg!, 'daily', minutesPerDay),
                  () => void runPreview(() => ai.aiPlan(cfg!, 'daily', minutesPerDay), () => {}),
                )
              }
            >
              安排今天
            </button>
            <button
              type="button"
              className="btn btn--ghost"
              disabled={!canUse || busy}
              onClick={() =>
                void runPreview(
                  () => ai.aiPlan(cfg!, 'weekly', minutesPerDay),
                  () => void runPreview(() => ai.aiPlan(cfg!, 'weekly', minutesPerDay), () => {}),
                )
              }
            >
              安排本周
            </button>
          </div>
          <p className="setgroup__hint">
            排程会遵守截止时间与任务依赖。若某条被排到截止时间之后，预览里会给出警告。
          </p>
        </div>

        {/* ---- 复盘 ---- */}
        <div className="formrow" style={{ marginTop: 14 }}>
          <span className="formlabel">复盘</span>
          <div className="setactions">
            <button
              type="button"
              className="btn btn--ghost"
              disabled={!canUse || busy}
              onClick={async () => {
                setBusy(true)
                setError(null)
                try {
                  setReview(await ai.aiReview(cfg!, 'daily'))
                } catch (e) {
                  setError(errText(e))
                } finally {
                  setBusy(false)
                }
              }}
            >
              今日复盘
            </button>
            <button
              type="button"
              className="btn btn--ghost"
              disabled={!canUse || busy}
              onClick={async () => {
                setBusy(true)
                setError(null)
                try {
                  setReview(await ai.aiReview(cfg!, 'weekly'))
                } catch (e) {
                  setError(errText(e))
                } finally {
                  setBusy(false)
                }
              }}
            >
              本周复盘
            </button>
          </div>
          <p className="setgroup__hint">
            复盘<strong>只发送聚合后的统计数据</strong>（数量、完成率、耗时），
            不发送任何任务标题或正文，也不会写入任何数据。
          </p>

          {review && (
            <div className="reviewbox">
              <div className="reviewbox__head">{review.summary}</div>
              <div className="reviewbox__text selectable">{review.text}</div>
              <div className="setgroup__hint">{review.dataScopeNote}</div>
              {review.usage && (
                <div className="setgroup__hint">
                  消耗：输入 {review.usage.inputTokens ?? '?'} / 输出{' '}
                  {review.usage.outputTokens ?? '?'} tokens
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* 差异预览确认闸门 */}
      {preview && (
        <DiffPreviewDialog
          preview={preview}
          onClose={() => setPreview(null)}
          onRegenerate={lastAction ?? undefined}
          onApplied={(created, updated) => {
            pushToast(
              'success',
              `已写入：新增 ${created} 项${updated > 0 ? `、修改 ${updated} 项` : ''}`,
            )
            void useApp.getState().reload()
            void useApp.getState().refreshOverview()
          }}
        />
      )}
    </>
  )
}

/**
 * 各服务商的数据使用政策提示。
 *
 * 三家姿态不一致，因此**不能统一文案**（这也是后端 `data_policy_note`
 * 的分支理由）。这里在前端重复一份简短版本，是为了在用户填写配置时
 * 就能看到，而不必等到调用时。
 */
function providerPolicyNote(p: AiProvider): string {
  switch (p) {
    case 'deep_seek':
      return '按 DeepSeek 官方政策，API 输入与输出默认会被用于改进其服务，需要在账户设置中主动关闭才能退出；数据存储在中国境内。发送敏感内容前请自行评估。'
    case 'open_ai':
      return '按 OpenAI 官方政策，自 2023-03-01 起 API 数据默认不用于训练（除非主动加入），滥用监控日志保留约 30 天。'
    case 'claude':
      return '按 Anthropic 官方政策，未经明确许可不会将数据用于训练，默认不保留对话内容；部分模型保留 30 天且不适用零数据保留。'
    default:
      return '自定义服务的隐私政策由其提供方决定，Lumen 无法代为说明。请自行确认该服务如何处理你的数据。'
  }
}
