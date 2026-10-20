import type { Task } from '../lib/types'
import { formatTaskTime, isOverdue } from '../lib/datetime'

/** A compact, actionable inspector for the selected day. */
export function TodayOverview({
  tasks,
  totalCount,
  onOpenTask,
}: {
  tasks: Task[]
  totalCount: number
  onOpenTask: (task: Task) => void
}) {
  const overdue = tasks.filter((task) => isOverdue(task))
  const inProgress = tasks.filter((task) => task.status === 'doing')
  const nextUp = [...tasks]
    .filter((task) => task.status !== 'done')
    .sort((a, b) => {
      const priority = (b.priority ?? 0) - (a.priority ?? 0)
      if (priority !== 0) return priority
      return (a.plannedAt ?? a.dueAt ?? '').localeCompare(
        b.plannedAt ?? b.dueAt ?? '',
      )
    })
    .slice(0, 4)

  return (
    <aside className="today-inspector" aria-label="当天任务概览">
      <div className="today-inspector__heading">
        <span className="today-inspector__eyebrow">当天概览</span>
        <strong>
          {totalCount}
          <span> 项</span>
        </strong>
      </div>

      <dl className="today-inspector__metrics">
        <div>
          <dt>逾期</dt>
          <dd className={overdue.length ? 'is-warning' : ''}>
            {overdue.length}
          </dd>
        </div>
        <div>
          <dt>进行中</dt>
          <dd>{inProgress.length}</dd>
        </div>
      </dl>

      <section
        className="today-inspector__section"
        aria-labelledby="today-inspector-next"
      >
        <h2 id="today-inspector-next">优先处理</h2>
        {nextUp.length ? (
          <ul>
            {nextUp.map((task) => (
              <li key={task.id}>
                <button type="button" onClick={() => onOpenTask(task)}>
                  <span
                    className={`today-inspector__priority today-inspector__priority--${task.priority ?? 0}`}
                    aria-hidden="true"
                  />
                  <span className="today-inspector__task">
                    <strong>{task.title}</strong>
                    <small>
                      {formatTaskTime(task) ||
                        (task.priority
                          ? `优先级 ${task.priority}`
                          : '未设置时间')}
                    </small>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="today-inspector__empty">当天没有待处理任务。</p>
        )}
      </section>

      <p className="today-inspector__footnote">
        逾期与进行中数量按当前已加载任务统计。
      </p>
    </aside>
  )
}
