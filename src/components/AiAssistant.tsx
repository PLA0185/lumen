import { useEffect, useRef, useState } from 'react'
import { format } from 'date-fns'
import * as ai from '../lib/ai-ipc'
import { IpcError } from '../lib/ipc'
import { useApp } from '../lib/store'
import { DiffPreviewDialog } from './DiffPreviewDialog'
import { AiPanel } from './AiPanel'
import { Icon } from './Icons'

export function AiAssistant({ compact = false }: { compact?: boolean }) {
  const [config, setConfig] = useState<ai.ProviderConfig | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [settings, setSettings] = useState(false)
  const [text, setText] = useState('')
  const [mode, setMode] = useState<'tasks' | 'plan' | 'review'>('tasks')
  const [horizon, setHorizon] = useState<ai.ReviewHorizon>('daily')
  const [date, setDate] = useState(() => format(new Date(), 'yyyy-MM-dd'))
  const [minutes, setMinutes] = useState(120)
  const [preview, setPreview] = useState<ai.DiffPreview | null>(null)
  const [review, setReview] = useState<ai.ReviewResult | null>(null)
  const [busy, setBusy] = useState(false)
  const running = useRef(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [filename, setFilename] = useState('')
  const pushToast = useApp((s) => s.pushToast)
  const reload = async () => {
    try {
      setConfig(await ai.aiGetConfig())
      setError(null)
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      setLoaded(true)
    }
  }
  useEffect(() => {
    void reload()
  }, [])

  const send = async () => {
    if (running.current || !config) return
    running.current = true
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      if (mode === 'review') {
        setReview(
          await ai.aiReview(config, horizon, date, text.trim() || undefined),
        )
      } else {
        const result =
          mode === 'plan'
            ? await ai.aiPlan(
                config,
                horizon === 'weekly' ? 'weekly' : 'daily',
                minutes,
              )
            : await ai.aiOrganize(config, text)
        setPreview(result)
      }
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      running.current = false
      setBusy(false)
    }
  }
  const importText = async (file: File | undefined) => {
    if (!file) return
    if (file.size > 120_000) {
      setError('文本文件过大，请截取需要处理的段落（最多 20000 字）')
      return
    }
    try {
      const content = await file.text()
      if ([...content].length > 20_000)
        throw new Error('文本超过 20000 字，请分段处理')
      setText(content)
      setFilename(file.name)
      setError(null)
    } catch (e) {
      setError(String(e))
    }
  }
  const canSend =
    loaded &&
    !!config?.hasApiKey &&
    !!config.model.trim() &&
    !busy &&
    (mode !== 'tasks' || !!text.trim())
  return (
    <div className={`ai-assistant${compact ? ' ai-assistant--compact' : ''}`}>
      <div className="ai-assistant__head">
        <div>
          <h2>AI 助手</h2>
          <p>写下你的想法，整理成计划、待办或总结。</p>
        </div>
        <button
          type="button"
          className="btn btn--quiet btn--sm"
          aria-expanded={settings}
          onClick={() => {
            setSettings((v) => !v)
            void reload()
          }}
        >
          {settings ? '返回助手' : '配置 AI'}
        </button>
      </div>
      {settings ? (
        <>
          <AiPanel />
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => {
              setSettings(false)
              void reload()
            }}
          >
            返回助手并刷新配置
          </button>
        </>
      ) : (
        <>
          <div className="segmented" role="tablist" aria-label="AI 助手功能">
            {(
              [
                ['tasks', '文本生成待办'],
                ['plan', '安排已有任务'],
                ['review', '总结与复盘'],
              ] as const
            ).map(([id, label]) => (
              <button
                type="button"
                role="tab"
                key={id}
                aria-selected={mode === id}
                className={`segmented__item${mode === id ? ' segmented__item--on' : ''}`}
                disabled={busy}
                onClick={() => {
                  setMode(id)
                  if (
                    id === 'plan' &&
                    horizon !== 'daily' &&
                    horizon !== 'weekly'
                  )
                    setHorizon('daily')
                  setReview(null)
                }}
              >
                {label}
              </button>
            ))}
          </div>
          {loaded && (!config?.hasApiKey || !config.model.trim()) && (
            <p className="alert alert--warn" role="status">
              {config?.hasApiKey
                ? '请选择模型，再开始使用 AI。'
                : '尚未配置 AI 密钥。点击“配置 AI”选择服务商并保存密钥。'}
            </p>
          )}
          {config?.hasApiKey && (
            <p className="setgroup__hint">
              当前模型：{config.model || '未选择'}
            </p>
          )}
          {mode !== 'plan' && (
            <>
              <textarea
                className="input selectable ai-assistant__input"
                value={text}
                maxLength={20000}
                aria-label="发送给 AI 的文本"
                placeholder={
                  mode === 'tasks'
                    ? '例如：明天下午整理运营表，这周五前完成关键词更新，先检查 CA 和 UK 两部分。'
                    : '可粘贴工作记录、会议内容或补充说明；留空时只总结真实任务统计。'
                }
                disabled={busy}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.ctrlKey && e.key === 'Enter') {
                    e.preventDefault()
                    if (canSend) void send()
                  }
                }}
              />
              <div className="ai-assistant__file">
                <label className="btn btn--quiet btn--sm">
                  导入文本
                  <input
                    type="file"
                    accept=".txt,.md,text/plain,text/markdown"
                    className="sr-only"
                    disabled={busy}
                    onChange={(e) => {
                      void importText(e.target.files?.[0])
                      e.target.value = ''
                    }}
                  />
                </label>
                <span>{filename || '支持 TXT、Markdown，也可以直接粘贴'}</span>
              </div>
            </>
          )}
          {mode !== 'tasks' && (
            <div className="rulerow">
              <label className="field">
                范围
                <select
                  aria-label="AI 时间范围"
                  value={horizon}
                  disabled={busy}
                  onChange={(e) =>
                    setHorizon(e.target.value as ai.ReviewHorizon)
                  }
                >
                  <option value="daily">日总结 / 今日</option>
                  <option value="weekly">周总结 / 本周</option>
                  {mode === 'review' && (
                    <>
                      <option value="monthly">月总结 / 本月</option>
                      <option value="yearly">年总结 / 本年</option>
                    </>
                  )}
                </select>
              </label>
              {mode === 'review' ? (
                <label className="field">
                  选择日期
                  <input
                    type="date"
                    aria-label="总结所在日期"
                    value={date}
                    disabled={busy}
                    onChange={(e) => setDate(e.target.value)}
                  />
                </label>
              ) : (
                <label className="field">
                  每天可用
                  <input
                    type="number"
                    min={1}
                    max={1440}
                    aria-label="每天可用分钟"
                    value={minutes}
                    onChange={(e) => setMinutes(Number(e.target.value))}
                  />
                  分钟
                </label>
              )}
            </div>
          )}
          <p className="ai-assistant__scope">
            {mode === 'review'
              ? '发送所选自然日 / 周 / 月 / 年的任务统计及你填写的文本；不会自动发送任务正文或附件。'
              : mode === 'tasks'
                ? '发送你填写的文本及已有任务摘要，用于安排日期与避免重复；不会自动发送备注或附件。'
                : '发送已有任务摘要和每天可用时间。'}{' '}
            {mode !== 'review' && '生成后可逐项编辑、勾选，再确认保存。'}
          </p>
          <button
            type="button"
            className="btn btn--primary"
            disabled={!canSend || (mode === 'review' && !date)}
            onClick={() => void send()}
          >
            <Icon name="star" size={16} />
            {busy
              ? '正在生成…'
              : mode === 'tasks'
                ? '发送并生成计划待办'
                : mode === 'plan'
                  ? '生成排程建议'
                  : '生成总结'}
          </button>
          {notice && (
            <p role="status" className="alert alert--ok">
              {notice}
            </p>
          )}
          {review && (
            <article className="ai-assistant__result selectable">
              <h3>{review.summary}</h3>
              <div className="ai-assistant__report">{review.text}</div>
              <p className="setgroup__hint">{review.dataScopeNote}</p>
              <button
                type="button"
                className="btn btn--quiet btn--sm"
                onClick={async () => {
                  try {
                    const { writeText } =
                      await import('@tauri-apps/plugin-clipboard-manager')
                    await writeText(review.text)
                    setNotice('总结已复制')
                  } catch (e) {
                    setError(String(e))
                  }
                }}
              >
                复制总结
              </button>
            </article>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="alert alert--error selectable">
          {error}
        </p>
      )}
      {preview && (
        <DiffPreviewDialog
          key={preview.previewId}
          preview={preview}
          onClose={() => setPreview(null)}
          onApplied={(created, updated) => {
            const message = `已创建 ${created} 条任务、更新 ${updated} 条；未设日期的任务可在收件箱查看`
            setNotice(message)
            pushToast('success', message)
          }}
        />
      )}
    </div>
  )
}
