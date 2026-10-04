import { useCallback, useEffect, useMemo, useState } from 'react'
import * as ipc from '../lib/ipc'
import * as org from '../lib/organize-ipc'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate, runLatestRequest } from '../lib/request-gate'
import type { Task } from '../lib/types'
import type { TaskCreationContext } from '../lib/task-creation-context'
import { TaskCard } from './TaskCard'

export function ContextTaskList({context, onEdit, onToggle, onDelete, onDuplicate}: {
  context: TaskCreationContext
  onEdit: (task: Task) => void
  onToggle: (id:string, done:boolean) => void
  onDelete: (id:string) => void
  onDuplicate: (task:Task) => void
}) {
  const gate = useMemo(createRequestGate, [])
  const [rows, setRows] = useState<Task[]>([])
  const [progress, setProgress] = useState<Record<string, org.SubtaskProgress>>({})
  const [total, setTotal] = useState(0)
  const [limit, setLimit] = useState(100)
  const [error, setError] = useState<string>()
  const [loading, setLoading] = useState(false)
  const reload = useCallback(async () => {
    setLoading(true)
    await runLatestRequest(gate, async () => {
      const query = { groupRecurring:true, projectId:context.projectId, categoryId:context.categoryId, tagIds:context.tagIds,
        statuses:['todo','doing','waiting','done'] as Task['status'][], sortBy:'created' as const, sortDesc:true }
      const count = await ipc.countTasks(query)
      const tasks: Task[] = []
      for (let offset = 0; offset < Math.min(limit, count.total); offset += 100) {
        tasks.push(...await ipc.listTasks({...query, limit:100, offset}))
      }
      const childProgress: org.SubtaskProgress[] = []
      for (let offset = 0; offset < tasks.length; offset += 1000) {
        childProgress.push(...await org.subtaskProgressBatch(tasks.slice(offset,offset+1000).map(t => t.id)))
      }
      return {tasks, count, childProgress}
    }, {apply: ({tasks, count, childProgress}) => {
      setRows(tasks); setTotal(count.total); setProgress(Object.fromEntries(childProgress.map(p => [p.taskId,p]))); setError(undefined)
    }, reject: e => setError(e instanceof ipc.IpcError ? e.userMessage() : String(e)), finish: () => setLoading(false)})
  }, [gate, context, limit])
  useEffect(() => {
    gate.activate(); void reload()
    const off = onDataChanged(['tasks','organization','subtasks','recurrence','all'], () => void reload())
    return () => { off(); gate.dispose() }
  }, [gate, reload])
  return <section className="organize context-task-list" aria-label={`${context.label}的任务`}>
    <h3>{context.label} · {total} 项任务</h3>
    {error && <div className="alert alert--error" role="alert">{error}<button className="btn btn--sm" onClick={() => void reload()}>重试</button></div>}
    {!loading && !error && rows.length === 0 && <p className="setgroup__hint">还没有任务，点击上方对应归属的加号新建。</p>}
    {rows.map(task => <TaskCard key={task.id} task={task} progress={progress[task.id]}
      onEdit={onEdit} onToggle={onToggle} onDelete={onDelete} onDuplicate={onDuplicate} />)}
    {rows.length < total && <button type="button" className="btn btn--sm" disabled={loading}
      onClick={() => setLimit(n => n + 100)}>加载更多</button>}
  </section>
}
