/**
 * 新建重复任务对话框（任务书 §5）。
 *
 * 与普通"快速添加"分开：重复任务需要用户看到规则预览与边界策略说明，
 * 塞进一行输入框会让这些关键信息无处安放。
 */

import { useState } from 'react'
import * as rec from '../lib/recurrence-ipc'
import { IpcError } from '../lib/ipc'
import { RuleEditor } from './RuleEditor'
import type { RuleEditorValue } from './RuleEditor'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface RecurringTaskDialogProps {
  onClose: () => void
  onCreated: (seriesId: string, warning?: string | null) => void
}

export function RecurringTaskDialog({ onClose, onCreated }: RecurringTaskDialogProps) {
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [priority, setPriority] = useState(0)
  /** 物化多少天内的实例（给用户一个"提前生成多少"的控制） */
  const [materializeDays, setMaterializeDays] = useState(90)
  const [rule, setRule] = useState<RuleEditorValue>({
    rrule: 'FREQ=WEEKLY',
    dtstartLocal: '',
    hasStartTime: true,
  })
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [titleError, setTitleError] = useState<string | null>(null)

  const create = async () => {
    const t = title.trim()
    if (!t) {
      setTitleError('标题不能为空')
      return
    }
    if (t.length > 500) {
      setTitleError('标题不能超过 500 个字符')
      return
    }
    if (!rule.dtstartLocal) {
      setError('请选择首次发生的日期')
      return
    }

    setSaving(true)
    setError(null)
    try {
      const r = await rec.recurringCreate({
        title: t,
        description: description.trim() || undefined,
        priority: priority || undefined,
        rrule: rule.rrule,
        tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
        dtstartLocal: rule.dtstartLocal,
        hasStartTime: rule.hasStartTime,
        materializeDays,
      })
      onCreated(r.seriesId, r.warning)
      onClose()
    } catch (e) {
      setError(errText(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="rec-title">
      <div className="modal modal--wide">
        <h2 className="modal__title" id="rec-title">
          新建重复任务
        </h2>

        {error && (
          <div className="alert alert--error" role="alert">
            <span className="selectable">{error}</span>
          </div>
        )}

        <div className="formrow">
          <label className="formlabel" htmlFor="rec-name">
            标题 <span className="req">*</span>
          </label>
          <input
            id="rec-name"
            className="input selectable"
            value={title}
            autoFocus
            placeholder="例如：写周报"
            aria-invalid={titleError ? true : undefined}
            onChange={(e) => {
              setTitle(e.target.value)
              if (titleError) setTitleError(null)
            }}
          />
          {titleError && (
            <div className="formerr" role="alert">
              {titleError}
            </div>
          )}
        </div>

        <div className="formrow">
          <label className="formlabel" htmlFor="rec-desc">
            描述
          </label>
          <input
            id="rec-desc"
            className="input selectable"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>

        <div className="formgrid">
          <label className="formrow">
            <span className="formlabel">优先级</span>
            <select
              className="input"
              value={priority}
              onChange={(e) => setPriority(Number(e.target.value))}
            >
              <option value={0}>无</option>
              <option value={1}>低</option>
              <option value={2}>中</option>
              <option value={3}>高</option>
            </select>
          </label>

          <label className="formrow">
            <span className="formlabel">提前生成</span>
            <div className="rulerow">
              <input
                type="number"
                className="input input--tiny"
                min={1}
                max={730}
                value={materializeDays}
                aria-label="提前生成多少天内的实例"
                onChange={(e) =>
                  setMaterializeDays(Math.max(1, Math.min(730, Number(e.target.value) || 90)))
                }
              />
              <span>天内</span>
            </div>
          </label>
        </div>
        <p className="setgroup__hint" style={{ marginTop: -4, marginBottom: 10 }}>
          只提前生成这段时间内实际要做的任务，更远的将来不会一次性写入数据库；
          当你浏览到更晚的日期时会自动补上。
        </p>

        <fieldset className="formfieldset">
          <legend>重复规则</legend>
          <RuleEditor value={rule} onChange={setRule} />
        </fieldset>

        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onClose} disabled={saving}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary"
            disabled={saving || !title.trim() || !rule.dtstartLocal}
            onClick={() => void create()}
          >
            {saving ? '创建中…' : '创建重复任务'}
          </button>
        </div>
      </div>
    </div>
  )
}
