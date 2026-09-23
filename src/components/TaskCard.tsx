/**
 * 任务卡片（§4.1 / §3）。
 *
 * 视觉要求：明确的完成状态、截止时间与优先级提示；
 * 悬停与键盘焦点都要有反馈；交互元素必须有可识别的无障碍名称。
 *
 * 卡片可展开，展开后显示提醒、子任务与依赖——这三项都需要较多空间，
 * 塞进折叠行会让列表变得难以扫视（§3「视觉参照成熟桌面工具的清晰度和克制感」）。
 */

import { useEffect, useState } from 'react'
import type { Task } from '../lib/types'
import { PERIOD_BADGES, PERIOD_LABELS } from '../lib/types'
import { formatTaskTime, isOverdue } from '../lib/datetime'
import { ReminderEditor } from './ReminderEditor'
import { SubtaskList } from './SubtaskList'
import { DependencyEditor } from './DependencyEditor'
import { AttachmentList } from './AttachmentList'
import { FocusPanel } from './FocusPanel'
import * as rec from '../lib/recurrence-ipc'
import type { ScopeInfo } from '../lib/recurrence-ipc'

/**
 * 重复系列信息块（§5）。
 *
 * 显示这条任务的系列规则、是第几次、以及"跳过这一次"入口。
 * 关于"跳过"与"删除"的区别，界面必须说清：
 * 跳过 = 这一次不发生但系列继续；删除整个系列才会终止后续。
 */
function SeriesInfo({ taskId }: { taskId: string }) {
  const [info, setInfo] = useState<ScopeInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      try {
        setInfo(await rec.recurringScopeInfo(taskId))
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      }
    })()
  }, [taskId])

  if (!info?.isRecurring) return null

  const skip = async () => {
    if (
      !window.confirm(
        '跳过这一次？\n\n这一次将不再发生，但重复系列会继续按规则产生后续的发生。\n' +
          '（若要终止整个系列，请使用编辑或删除功能。）',
      )
    ) {
      return
    }
    setBusy(true)
    setError(null)
    try {
      const r = await rec.recurringSkipOccurrence(taskId)
      setNotice(r.message)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="seriesinfo">
      <div className="seriesinfo__row">
        <span className="seriesinfo__rule" title="这是重复任务的一次发生">
          ↻ 第 {info.occurrenceIndex ?? '?'} 次发生
        </span>
        <span>
          系列共 <strong>{info.totalInstances ?? 0}</strong> 次
        </span>
        <span>
          已完成 <strong>{info.completed ?? 0}</strong> 次
        </span>
        {info.isException && (
          <span className="chip chip--warn" title="这一次被单独修改过，改系列规则时会保护它">
            已单独修改
          </span>
        )}
        {(info.segments ?? 0) > 0 && (
          <span className="chip chip--muted" title="该系列从某次起改用过新规则">
            规则已分段 {info.segments} 次
          </span>
        )}
        <button
          type="button"
          className="btn btn--quiet btn--sm"
          disabled={busy}
          title="只让这一次不发生，系列继续"
          onClick={() => void skip()}
        >
          {busy ? '处理中…' : '跳过这一次'}
        </button>
      </div>
      {notice && (
        <div className="seriesinfo__segments" role="status">
          {notice}
        </div>
      )}
      {error && (
        <div className="formerr" role="alert">
          {error}
        </div>
      )}
    </div>
  )
}

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
  /** 打开完整编辑表单（§4.1 字段集） */
  onEdit?: (task: Task) => void
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
  onEdit,
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
            {/* 周期跨度徽标：让用户一眼看出这是"这周做完就行"而不是某天的具体安排 */}
            {task.periodType && task.periodType !== 'none' && (
              <span
                className="badge badge--period"
                title={`周期任务：${PERIOD_LABELS[task.periodType]}完成即可，不绑定到具体某一天`}
              >
                {PERIOD_BADGES[task.periodType]}
              </span>
            )}
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
                title="编辑任务详情"
                aria-label={`编辑「${task.title}」`}
                onClick={() => onEdit?.(task)}
              >
                ✎
              </button>
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
          {isRecurring && <SeriesInfo taskId={task.id} />}
          <SubtaskList taskId={task.id} />
          <AttachmentList taskId={task.id} />
          {/* 专注计时绑定到这个任务：结束后时长累加到 actual_minutes */}
          <div className="focusblock">
            <div className="subtasks__head">
              <span>专注</span>
            </div>
            <FocusPanel taskId={task.id} taskTitle={task.title} compact />
          </div>
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
