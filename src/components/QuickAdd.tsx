/**
 * 快速添加表单（§4.4）。
 *
 * 核心要求：自然语言解析的结果**必须在保存前可见可改**。
 * 因此这里把解析命中的日期/标签/优先级显式展示成可撤销的提示条，
 * 用户也可以直接用日期/时间控件覆盖，所见即所存。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { format } from 'date-fns'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import { parseQuickInput } from '../lib/nlp'
import { combineDateTime } from '../lib/datetime'
import type { PeriodType, Task } from '../lib/types'

interface QuickAddProps {
  /** 保存成功后回调，用于刷新列表 */
  onCreated: (t: Task) => void
  /** 是否自动聚焦（打开快速添加窗口时为真） */
  autoFocus?: boolean
  /** 取消（关闭） */
  onCancel?: () => void
  /** 可用标签（用于把 #名称 映射成标签 ID） */
  tags?: { id: string; name: string }[]
}

export function QuickAdd({ onCreated, autoFocus = true, onCancel, tags = [] }: QuickAddProps) {
  const [text, setText] = useState('')
  const [dateStr, setDateStr] = useState('')
  const [timeStr, setTimeStr] = useState('')
  const [priority, setPriority] = useState(0)
  /** 周期跨度：这周/这个月做完就行，不必定到某天 */
  const [periodType, setPeriodType] = useState<PeriodType>('none')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** 用户是否手动改过日期；改过之后不再用解析值覆盖（避免"我改的又被冲掉"） */
  const [dateTouched, setDateTouched] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const parsed = useMemo(() => parseQuickInput(text), [text])

  // 解析结果回填到控件，使用户能直接看到并修改（§4.4）
  useEffect(() => {
    if (dateTouched || !parsed.date) return
    setDateStr(format(parsed.date, 'yyyy-MM-dd'))
    setTimeStr(parsed.hasTime ? format(parsed.date, 'HH:mm') : '')
  }, [parsed.date, parsed.hasTime, dateTouched])

  useEffect(() => {
    if (!parsed.priority) return
    setPriority((p) => (p === 0 ? (parsed.priority as number) : p))
  }, [parsed.priority])

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus()
  }, [autoFocus])

  const reset = useCallback(() => {
    setText('')
    setDateStr('')
    setTimeStr('')
    setPriority(0)
    setPeriodType('none')
    setError(null)
    setDateTouched(false)
  }, [])

  const save = useCallback(async () => {
    const title = (parsed.title || text).trim()
    if (!title) {
      setError('请输入任务标题')
      inputRef.current?.focus()
      return
    }
    setSaving(true)
    setError(null)
    try {
      const dt = dateStr ? combineDateTime(dateStr, timeStr) : null
      const tagIds = parsed.tags
        .map((name) => tags.find((t) => t.name.toLowerCase() === name.toLowerCase())?.id)
        .filter((x): x is string => Boolean(x))

      const task = await ipc.createTask({
        title,
        priority: priority || undefined,
        periodType: periodType === 'none' ? undefined : periodType,
        plannedAt: dt?.utc ?? null,
        hasPlannedTime: dt?.hasTime ?? false,
        tagIds,
      })
      onCreated(task)
      // createTask 的统一 mutation 层已通知所有数据视图。
      reset()
      inputRef.current?.focus()
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      setSaving(false)
    }
    // `periodType` 必须列进来：它参与请求体（周期跨度），
    // 漏掉会让"先选周期再回车"保存成上一次的值。
  }, [parsed, text, dateStr, timeStr, priority, periodType, tags, onCreated, reset])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      void save()
    } else if (e.key === 'Escape' && onCancel) {
      e.preventDefault()
      onCancel()
    }
  }

  const hasParseHints = Boolean(parsed.matched || parsed.tags.length > 0 || parsed.priority)

  return (
    <div className="quickadd">
      <div className="quickadd__row">
        <input
          ref={inputRef}
          className="quickadd__input selectable"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="添加任务，可直接写「明天 10:00 交周报 #工作」"
          aria-label="任务标题，可包含日期、标签与优先级"
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? 'quickadd-error' : undefined}
        />
        <button
          type="button"
          className="btn btn--primary btn--sm"
          onClick={() => void save()}
          disabled={saving || !text.trim()}
        >
          {saving ? '保存中…' : '添加'}
        </button>
        {onCancel && (
          <button type="button" className="btn btn--quiet btn--sm" onClick={onCancel}>
            取消
          </button>
        )}
      </div>

      <div className="quickadd__row quickadd__row--meta">
        <label className="field">
          计划
          <input
            type="date"
            value={dateStr}
            aria-label="计划执行日期"
            onChange={(e) => {
              setDateStr(e.target.value)
              setDateTouched(true)
            }}
          />
        </label>
        <label className="field">
          时间
          <input
            type="time"
            value={timeStr}
            aria-label="计划执行时间（留空表示仅日期）"
            onChange={(e) => {
              setTimeStr(e.target.value)
              setDateTouched(true)
            }}
          />
        </label>
        <label className="field">
          优先级
          <select
            value={priority}
            aria-label="优先级"
            onChange={(e) => setPriority(Number(e.target.value))}
          >
            <option value={0}>无</option>
            <option value={1}>低</option>
            <option value={2}>中</option>
            <option value={3}>高</option>
          </select>
        </label>

        {/* 周期跨度：用于"这周/这个月做完就行"的任务。
            选中后**不需要**填具体日期，因此这里不做联动清空，
            用户想同时指定日期也可以。 */}
        <label className="field">
          周期
          <select
            value={periodType}
            aria-label="周期跨度"
            title="标记为某个周期内完成即可，不必绑定到具体某一天"
            onChange={(e) => setPeriodType(e.target.value as PeriodType)}
          >
            <option value="none">不限</option>
            <option value="day">今日内</option>
            <option value="week">本周内</option>
            <option value="month">本月内</option>
            <option value="quarter">本季度内</option>
            <option value="year">今年内</option>
          </select>
        </label>
        {dateStr && (
          <button
            type="button"
            className="btn btn--quiet btn--sm"
            onClick={() => {
              setDateStr('')
              setTimeStr('')
              setDateTouched(true)
            }}
          >
            清除日期
          </button>
        )}
      </div>

      {/* 解析结果预览：用户在保存前就能看见系统识别到了什么（§4.4） */}
      {hasParseHints && (
        <div className="quickadd__hint">
          识别到：
          {parsed.label && <strong> {parsed.label}</strong>}
          {parsed.tags.map((t) => (
            <span key={t}> #{t}</span>
          ))}
          {parsed.priority ? <span> 优先级 {parsed.priority}</span> : null}
          {parsed.tags.some((t) => !tags.find((x) => x.name.toLowerCase() === t.toLowerCase())) && (
            <span>（灰色标签尚未创建，保存后不会自动建标签）</span>
          )}
        </div>
      )}

      {error && (
        <div className="quickadd__error" id="quickadd-error" role="alert">
          {error}
        </div>
      )}
    </div>
  )
}
