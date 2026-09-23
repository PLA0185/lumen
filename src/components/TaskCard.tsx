/**
 * 任务卡片（§4.1 / §3）。
 *
 * 视觉要求：明确的完成状态、截止时间与优先级提示；
 * 悬停与键盘焦点都要有反馈；交互元素必须有可识别的无障碍名称。
 *
 * 卡片可展开，展开后显示提醒、子任务与依赖——这三项都需要较多空间，
 * 塞进折叠行会让列表变得难以扫视（§3「视觉参照成熟桌面工具的清晰度和克制感」）。
 */

import { useState } from 'react'
import type { Task } from '../lib/types'
import { formatTaskTime, isOverdue } from '../lib/datetime'
import { ReminderEditor } from './ReminderEditor'
import { SubtaskList } from './SubtaskList'
import { DependencyEditor } from './DependencyEditor'

/** 优先级文案（§4.1 四级） */
const PRIORITY_LABEL: Record<number, string> = {
  0: '无优先级',
  1: '低优先级',
  2: '中优先级',
  3: '高优先级',
}

interface TaskCardProps {
  task: Task
  onToggle: (id: string, done: boolean) => void
  onDelete: (id: string) => void
  onRestore?: (id: string) => void
  onPurge?: (id: string) => void
  /** 回收站视图下显示恢复/永久删除而非完成/删除 */
  mode?: 'normal' | 'trash'
  /** 子任务进度（由列表页批量获取，避免每张卡片各查一次） */
  progress?: { total: number; done: number; percent: number | null }
}

export function TaskCard({
  task,
  onToggle,
  onDelete,
  onRestore,
  onPurge,
  mode = 'normal',
  progress,
}: TaskCardProps) {
  const done = task.status === 'done'
  const overdue = isOverdue(task)
  const timeText = formatTaskTime(task)
  const inTrash = mode === 'trash'
  const isRecurring = task.seriesId !== null
  const [expanded, setExpanded] = useState(false)

  const hasDetail = !inTrash

  return (
    <li
      className={[
        'task',
        done ? 'task--done' : '',
        overdue ? 'task--overdue' : '',
        task.isPinned ? 'task--pinned' : '',
        expanded ? 'task--expanded' : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div className="task__main">
        {!inTrash && (
          <button
            type="button"
            role="checkbox"
            aria-checked={done}
            aria-label={done ? `将「${task.title}」标记为未完成` : `将「${task.title}」标记为已完成`}
            className="task__check"
            onClick={() => onToggle(task.id, !done)}
          >
            {done && (
              <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
                <path
                  d="M2.5 6.2l2.3 2.3L9.5 3.8"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            )}
          </button>
        )}

        <div className="task__body" onClick={() => hasDetail && setExpanded((v) => !v)}>
          <div className="task__title">{task.title}</div>

          {task.description && <div className="task__desc">{task.description}</div>}

          <div className="task__meta">
            {task.priority > 0 && (
              <>
                <span
                  className={`prio prio--${task.priority}`}
                  role="img"
                  aria-label={PRIORITY_LABEL[task.priority]}
                />
                <span>{PRIORITY_LABEL[task.priority]}</span>
              </>
            )}

            {overdue && <span className="badge badge--overdue">已逾期</span>}
            {done && <span className="badge badge--done">已完成</span>}
            {isRecurring && (
              <span className="badge badge--recurring" title="这是重复任务的一次发生">
                ↻ 重复
              </span>
            )}

            {timeText && <span>{timeText}</span>}

            {task.estimatedMinutes != null && task.estimatedMinutes > 0 && (
              <span title="预计耗时">约 {formatMinutes(task.estimatedMinutes)}</span>
            )}

            {/* 子任务进度：只在确实有子任务时显示，避免"0/0"噪音 */}
            {progress && progress.total > 0 && (
              <span className="task__progress" title={`已完成 ${progress.done} / ${progress.total}`}>
                <span className="progress progress--inline">
                  <span
                    className="progress__bar"
                    style={{ width: `${progress.percent ?? 0}%` }}
                  />
                </span>
                {progress.done}/{progress.total}
              </span>
            )}

            {inTrash && task.deletedAt && (
              <span>删除于 {new Date(task.deletedAt).toLocaleString('zh-CN')}</span>
            )}
          </div>
        </div>

        <div className="task__actions">
          {inTrash ? (
            <>
              <button
                type="button"
                className="icon-btn"
                title="恢复到任务列表"
                aria-label={`恢复「${task.title}」`}
                onClick={() => onRestore?.(task.id)}
              >
                ↺
              </button>
              <button
                type="button"
                className="icon-btn icon-btn--danger"
                title="永久删除（不可恢复）"
                aria-label={`永久删除「${task.title}」`}
                onClick={() => onPurge?.(task.id)}
              >
                ✕
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                className="icon-btn"
                aria-expanded={expanded}
                title={expanded ? '收起详情' : '展开详情（提醒、子任务、依赖）'}
                aria-label={`${expanded ? '收起' : '展开'}「${task.title}」的详情`}
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded ? '▴' : '▾'}
              </button>
              <button
                type="button"
                className="icon-btn icon-btn--danger"
                title="移入回收站"
                aria-label={`将「${task.title}」移入回收站`}
                onClick={() => onDelete(task.id)}
              >
                ✕
              </button>
            </>
          )}
        </div>
      </div>

      {expanded && hasDetail && (
        <div className="task__detail">
          <SubtaskList taskId={task.id} />
          <ReminderEditor
            taskId={task.id}
            hasPlanned={Boolean(task.plannedAt)}
            hasDue={Boolean(task.dueAt)}
            taskDone={done || task.status === 'archived'}
          />
          <DependencyEditor taskId={task.id} taskTitle={task.title} />
        </div>
      )}
    </li>
  )
}

/** 分钟 → 可读文本 */
function formatMinutes(m: number): string {
  if (m < 60) return `${m} 分钟`
  const h = Math.floor(m / 60)
  const rest = m % 60
  return rest === 0 ? `${h} 小时` : `${h} 小时 ${rest} 分`
}
