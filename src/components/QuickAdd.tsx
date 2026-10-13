import { ContentEditor } from './ContentEditor'
/**
 * 快速添加表单（§4.4）。
 *
 * 核心要求：自然语言解析的结果**必须在保存前可见可改**。
 * 因此这里把解析命中的日期/标签/优先级显式展示成可撤销的提示条，
 * 用户也可以直接用日期/时间控件覆盖，所见即所存。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { addDays, addWeeks, format, startOfWeek } from 'date-fns'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import { parseQuickInput } from '../lib/nlp'
import { combineDateTime } from '../lib/datetime'
import type { PeriodType, Task } from '../lib/types'
import * as rec from '../lib/recurrence-ipc'
import { RuleEditor } from './RuleEditor'
import type { RuleEditorValue } from './RuleEditor'
import type { TaskCreationContext } from '../lib/task-creation-context'
import { Icon } from './Icons'
import { AiAssistant } from './AiAssistant'

interface QuickAddProps {
  /** 浮窗新建行：选项就地展开，不跳到另一个窗口。 */
  defaultRepeat?: 'none' | 'daily' | 'weekly' | 'monthly' | 'quarterly' | 'yearly'
  compact?: boolean
  onConfigureReminder?: (task: Task) => void
  /** 从今天/明天新建时沿用该视图日期；其它入口仍可不设日期。 */
  defaultPlannedDate?: string
  defaultPeriodType?: PeriodType
  context?: TaskCreationContext
  /** 保存成功后回调，用于刷新列表 */
  onCreated: (t: Task) => void
  onRecurringCreated?: (warning: string | null) => void | Promise<void>
  /** 是否自动聚焦（打开快速添加窗口时为真） */
  autoFocus?: boolean
  /** 取消（关闭） */
  onCancel?: () => void
  /** 可用标签（用于把 #名称 映射成标签 ID） */
  tags?: { id: string; name: string }[]
}

function repeatRrule(repeat: string, custom = 'FREQ=WEEKLY') {
  if (repeat === 'custom') return custom
  if (repeat === 'daily') return 'FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR'
  if (repeat === 'quarterly') return 'FREQ=MONTHLY;INTERVAL=3'
  return `FREQ=${repeat.toUpperCase()}`
}

export function QuickAdd({
  compact = false,
  defaultRepeat = 'none',
  onConfigureReminder,
  onCreated,
  onRecurringCreated,
  autoFocus = true,
  onCancel,
  tags = [],
  defaultPlannedDate = '',
  defaultPeriodType = 'none',
  context,
}: QuickAddProps) {
  const [optionsOpen, setOptionsOpen] = useState(!compact)
  const [configureReminder, setConfigureReminder] = useState(false)
  const [text, setText] = useState('')
  const [dateStr, setDateStr] = useState(defaultPlannedDate)
  const [timeStr, setTimeStr] = useState('')
  const [priority, setPriority] = useState(0)
  const [repeat, setRepeat] = useState<string>(defaultRepeat)
  const [rule, setRule] = useState<RuleEditorValue>({
    rrule: repeatRrule(defaultRepeat),
    dtstartLocal: '',
    hasStartTime: false,
  })
  const [description, setDescription] = useState('')
  const [subtasksText, setSubtasksText] = useState('')
  const [subtaskCompletionMode, setSubtaskCompletionMode] = useState<'free' | 'sequential'>('free')
  const [materializeDays, setMaterializeDays] = useState(90)
  const savingRef = useRef(false)
  /** 周期跨度：这周/这个月做完就行，不必定到某天 */
  const [periodType, setPeriodType] = useState<PeriodType>(defaultPeriodType)
  const [periodTouched, setPeriodTouched] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** 用户是否手动改过日期；改过之后不再用解析值覆盖（避免"我改的又被冲掉"） */
  const [dateTouched, setDateTouched] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const draftSnapshot = JSON.stringify({ text, dateStr, timeStr, priority, repeat, rule, description,
    subtasksText, subtaskCompletionMode,
    materializeDays, periodType, periodTouched, dateTouched, configureReminder })
  const latestDraft = useRef(draftSnapshot)
  latestDraft.current = draftSnapshot

  const parsed = useMemo(() => parseQuickInput(text), [text])
  useEffect(() => {
    if (!periodTouched) setPeriodType(defaultPeriodType)
  }, [defaultPeriodType, periodTouched])

  useEffect(() => {
    if (!dateTouched && !parsed.date && repeat === 'none')
      setDateStr(defaultPlannedDate)
  }, [defaultPlannedDate, dateTouched, parsed.date, repeat])

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
    setDateStr(defaultPlannedDate)
    setTimeStr('')
    setPriority(0)
    setPeriodType(defaultPeriodType)
    setPeriodTouched(false)
    setError(null)
    setDateTouched(false)
    setRepeat(defaultRepeat)
    setDescription('')
    setSubtasksText('')
    setSubtaskCompletionMode('free')
    setMaterializeDays(90)
    setConfigureReminder(false)
    setRule({ rrule: repeatRrule(defaultRepeat), dtstartLocal: '', hasStartTime: false })
  }, [defaultPlannedDate, defaultPeriodType, defaultRepeat])

  const save = useCallback(async () => {
    if (savingRef.current) return
    const title = (parsed.title || text).trim()
    if (!title) {
      setError('请输入任务标题')
      inputRef.current?.focus()
      return
    }
    savingRef.current = true
    setSaving(true)
    setError(null)
    try {
      const dt = dateStr ? combineDateTime(dateStr, timeStr) : null
      const parsedTagIds = parsed.tags
        .map(
          (name) =>
            tags.find((t) => t.name.toLowerCase() === name.toLowerCase())?.id,
        )
        .filter((x): x is string => Boolean(x))
      const tagIds = [...new Set([...(context?.tagIds ?? []), ...parsedTagIds])]
      const subtasks = subtasksText.split(/\r?\n/).map((item) => item.trim()).filter(Boolean)

      if (repeat !== 'none') {
        if (!rule.dtstartLocal) throw new Error('请选择首次发生的日期')
        const result = await rec.recurringCreate({
          title,
          projectId: context?.projectId,
          categoryId: context?.categoryId,
          description: description.trim() || undefined,
          priority: priority || undefined,
          tagIds,
          rrule: rule.rrule,
          dtstartLocal: rule.dtstartLocal,
          hasStartTime: rule.hasStartTime,
          materializeDays,
          tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
        }, { subtasks, subtaskCompletionMode })
        await onRecurringCreated?.(result.warning)
        if (result.warning && !onRecurringCreated) setError(result.warning)
      } else {
        const task = await ipc.createTask({
          title,
          description: description.trim() || undefined,
          projectId: context?.projectId,
          categoryId: context?.categoryId,
          priority: priority || undefined,
          periodType: periodType === 'none' ? undefined : periodType,
          plannedAt: dt?.utc ?? null,
          hasPlannedTime: dt?.hasTime ?? false,
          tagIds,
        }, { subtasks, subtaskCompletionMode })
        onCreated(task)
        if (configureReminder) onConfigureReminder?.(task)
      }
      // createTask 的统一 mutation 层已通知所有数据视图。
      if (latestDraft.current === draftSnapshot) reset()
      inputRef.current?.focus()
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      savingRef.current = false
      setSaving(false)
    }
    // `periodType` 必须列进来：它参与请求体（周期跨度），
    // 漏掉会让"先选周期再回车"保存成上一次的值。
  }, [
    parsed,
    text,
    dateStr,
    timeStr,
    priority,
    periodType,
    tags,
    onCreated,
    onRecurringCreated,
    reset,
    repeat,
    rule,
    description,
    subtasksText,
    subtaskCompletionMode,
    materializeDays,
    context,
    configureReminder,
    onConfigureReminder,
    draftSnapshot,
  ])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      void save()
    } else if (e.key === 'Escape' && onCancel) {
      e.preventDefault()
      onCancel()
    }
  }

  const hasParseHints = Boolean(
    parsed.matched || parsed.tags.length > 0 || parsed.priority,
  )

  const selectDatePreset = (date: Date) => {
    setDateStr(format(date, 'yyyy-MM-dd'))
    setDateTouched(true)
  }

  return (
    <div className={`quickadd${compact ? ' quickadd--compact' : ''}`}>
      {context && (
        <p className="setgroup__hint" role="note">
          新建到：{context.label}
        </p>
      )}
      <div className="quickadd__row">
        {compact && (
          <span className="floating__draft-check" aria-hidden="true" />
        )}
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
        {compact && (
          <button
            type="button"
            className="icon-btn"
            aria-label="新任务选项"
            title="编辑日期、时间、优先级和重复规则"
            aria-expanded={optionsOpen}
            onClick={() => setOptionsOpen((v) => !v)}
          >
            <Icon name="edit" size={17} />
          </button>
        )}
        {compact && onConfigureReminder && (
          <button
            type="button"
            className="icon-btn"
            aria-label="新任务提醒"
            title="添加后设置提醒"
            aria-pressed={configureReminder}
            disabled={repeat !== 'none'}
            onClick={() => {
              setConfigureReminder((v) => !v)
              setOptionsOpen(true)
            }}
          >
            <Icon name="clock" size={17} />
          </button>
        )}
        {onCancel && (
          <button
            type="button"
            className="btn btn--quiet btn--sm"
            onClick={onCancel}
          >
            取消
          </button>
        )}
      </div>

      {optionsOpen && (
        <div className="quickadd__row quickadd__row--meta">
          {repeat === 'none' && (
            <label className="field">
              计划日期
              <input
                type="date"
                value={dateStr}
                aria-label="计划执行日期"
                title="选择任务安排的具体日期"
                onChange={(e) => {
                  setDateStr(e.target.value)
                  setDateTouched(true)
                }}
              />
            </label>
          )}
          {repeat === 'none' && (
            <label className="field">
              时间
              <input
                type="time"
                value={timeStr}
                aria-label="计划执行时间（留空表示仅日期）"
                title="选择具体执行时间；留空表示仅安排日期"
                onChange={(e) => {
                  setTimeStr(e.target.value)
                  setDateTouched(true)
                }}
              />
            </label>
          )}
          {repeat === 'none' && (
            <div className="quickadd__date-presets" role="group" aria-label="计划日期快捷选择">
              <span>快捷选择</span>
              <button type="button" className="btn btn--quiet btn--sm" onClick={() => selectDatePreset(new Date())}>
                今天
              </button>
              <button type="button" className="btn btn--quiet btn--sm" onClick={() => selectDatePreset(addDays(new Date(), 1))}>
                明天
              </button>
              <button
                type="button"
                className="btn btn--quiet btn--sm"
                title="设为下周一；之后可以在日期控件里改成下周其他日期"
                onClick={() => selectDatePreset(startOfWeek(addWeeks(new Date(), 1), { weekStartsOn: 1 }))}
              >
                下周
              </button>
            </div>
          )}
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
          {
            <label className="field">
              周期
              <select
                value={periodType}
                aria-label="周期跨度"
                title="标记为某个周期内完成即可，不必绑定到具体某一天"
                onChange={(e) => {
                  setPeriodTouched(true)
                  setPeriodType(e.target.value as PeriodType)
                }}
              >
                <option value="none">不限</option>
                <option value="day">今日内</option>
                <option value="week">本周内</option>
                <option value="month">本月内</option>
                <option value="quarter">本季度内</option>
                <option value="year">今年内</option>
              </select>
            </label>
          }
          <label className="field">
            重复
            <select
              aria-label="任务重复"
              value={repeat}
              onChange={(e) => {
                const next = e.target.value
                setRepeat(next)
                if (next !== 'none')
                  setRule({
                    rrule: repeatRrule(next, rule.rrule),
                    dtstartLocal:
                      rule.dtstartLocal ||
                      `${dateStr || format(new Date(), 'yyyy-MM-dd')}T${timeStr || '00:00'}:00`,
                    hasStartTime: rule.dtstartLocal
                      ? rule.hasStartTime
                      : Boolean(timeStr),
                  })
              }}
            >
              <option value="none">不重复</option>
              <option value="daily">每天</option>
              <option value="weekly">每周</option>
              <option value="monthly">每月</option>
              <option value="quarterly">每季度</option>
              <option value="yearly">每年</option>
              <option value="custom">自定义规则</option>
            </select>
          </label>
          {repeat === 'none' && dateStr && (
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
      )}

      {optionsOpen && onConfigureReminder && repeat === 'none' && (
        <label className="checkbox">
          <input
            type="checkbox"
            checked={configureReminder}
            onChange={(e) => setConfigureReminder(e.target.checked)}
          />
          添加后设置提醒
        </label>
      )}
      {optionsOpen && (
        <label className="formrow">
          <span className="formlabel">任务描述</span>
          <ContentEditor
            className="input input--area selectable"
            rows={3}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            aria-label="任务描述"
            placeholder="要完成什么、交付要求、注意事项…可分行填写"
          />
        </label>
      )}
      {optionsOpen && (
        <div className="quickadd__subtasks">
          <label className="formrow">
            <span className="formlabel">子任务（按行填写，顺序从上到下）</span>
            <textarea
              className="input input--area selectable"
              rows={3}
              value={subtasksText}
              onChange={(e) => setSubtasksText(e.target.value)}
              aria-label="子任务列表，每行一项"
              placeholder={'例如：\n准备资料\n完成核对\n提交结果'}
            />
          </label>
          <label className="field">
            子任务完成方式
            <select
              value={subtaskCompletionMode}
              aria-label="子任务完成方式"
              onChange={(e) => setSubtaskCompletionMode(e.target.value as 'free' | 'sequential')}
            >
              <option value="free">自由完成</option>
              <option value="sequential">按顺序完成</option>
            </select>
          </label>
        </div>
      )}
      {repeat !== 'none' && (
        <fieldset className="formfieldset quickadd__repeat">
          <legend>重复规则</legend>
          <RuleEditor
            key={repeat}
            value={rule}
            onChange={setRule}
            periodType={periodType}
          />
          <label className="field">
            提前生成
            <input
              type="number"
              min={1}
              max={730}
              value={materializeDays}
              onChange={(e) =>
                setMaterializeDays(
                  Math.max(1, Math.min(730, Number(e.target.value) || 90)),
                )
              }
              aria-label="提前生成多少天内的实例"
            />
            天内
          </label>
        </fieldset>
      )}

      {/* 解析结果预览：用户在保存前就能看见系统识别到了什么（§4.4） */}
      {hasParseHints && (
        <div className="quickadd__hint">
          识别到：
          {parsed.label && <strong> {parsed.label}</strong>}
          {parsed.tags.map((t) => (
            <span key={t}> #{t}</span>
          ))}
          {parsed.priority ? <span> 优先级 {parsed.priority}</span> : null}
          {parsed.tags.some(
            (t) => !tags.find((x) => x.name.toLowerCase() === t.toLowerCase()),
          ) && <span>（灰色标签尚未创建，保存后不会自动建标签）</span>}
        </div>
      )}

      {error && (
        <div className="quickadd__error" id="quickadd-error" role="alert">
          {error}
        </div>
      )}
      {!compact && (
        <div className="quickadd__ai">
          <AiAssistant compact creationDefaults={{ plannedDate: defaultPlannedDate, periodType: defaultPeriodType, ...context }} />
        </div>
      )}
    </div>
  )
}
