import { useState } from 'react'
import type { Subtask } from '../lib/organize-ipc'
import { subtaskUpdate } from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import { Icon } from './Icons'

/** 已经由父视图批量取得的子任务；简览本身不产生逐卡查询。 */
export function SubtaskPreview({
  items,
  disabled = false,
}: {
  items: Subtask[]
  disabled?: boolean
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
      <ul className="subtask-preview__list" aria-label="子任务简览">
        {items.map((item) => (
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
              disabled={disabled || saving !== null}
              onClick={() => void toggle(item)}
            >
              {item.isDone === 1 && <Icon name="completed" size={9} />}
            </button>
            <span className="subtask-preview__title selectable">
              {item.title}
            </span>
          </li>
        ))}
      </ul>
      {error && (
        <div className="formerr selectable" role="alert">
          {error}
        </div>
      )}
    </div>
  )
}
