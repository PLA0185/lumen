import { useState } from 'react'
import { subtaskSequenceBlocked } from '../lib/organize-ipc'
import type { Subtask, SubtaskCompletionMode } from '../lib/organize-ipc'
import { subtaskUpdate } from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import { CheckMark } from './CheckMark'
import { formatCompletionTime } from '../lib/datetime'

/** 已经由父视图批量取得的子任务；简览本身不产生逐卡查询。 */
export function SubtaskPreview({
  items,
  disabled = false,
  completionMode = 'free',
}: {
  items: Subtask[]
  disabled?: boolean
  completionMode?: SubtaskCompletionMode
}) {
  const [saving, setSaving] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const toggle = async (item: Subtask) => {
    if (saving || disabled) return
    setSaving(item.id)
    setError(null)
    try {
      await subtaskUpdate(item.id, { isDone: item.isDone !== 1 })
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      setSaving(null)
    }
  }
  return (
    <div className="subtask-preview">
      {completionMode === 'sequential' && (
        <span className="subtask-preview__mode">按顺序完成</span>
      )}
      <ul className="subtask-preview__list" aria-label="子任务简览">
        {items.map((item, index) => {
          const sequenceBlocked = subtaskSequenceBlocked(
            items,
            index,
            completionMode,
          )
          return (
            <li
              key={item.id}
              className={`subtask-preview__item${item.isDone === 1 ? ' subtask-preview__item--done' : ''}`}
            >
              <button
                type="button"
                role="checkbox"
                aria-checked={item.isDone === 1}
                aria-label={`${item.isDone === 1 ? '取消完成' : '完成'}子任务「${item.title}」`}
                className="task__check task__check--sm"
                disabled={disabled || saving !== null || sequenceBlocked}
                title={sequenceBlocked ? '请按列表顺序完成子任务' : undefined}
                onClick={() => void toggle(item)}
              >
                {item.isDone === 1 && <CheckMark size={11} />}
              </button>
              <span className="subtask-preview__title selectable">
                {item.title}
              </span>
              {item.isDone === 1 && (
                <span className="subtask__completed selectable">
                  {formatCompletionTime(item.completedAt)}
                </span>
              )}
            </li>
          )
        })}
      </ul>
      {error && (
        <div className="formerr selectable" role="alert">
          {error}
        </div>
      )}
    </div>
  )
}
