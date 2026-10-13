import { useCallback, useEffect, useState } from 'react'
import { invokeData, onDataChanged } from '../lib/data-change'
import { contentError } from '../lib/content-assets'
import { SeriesRuleDialog } from './SeriesRuleDialog'

interface Arrangement {
  seriesId: string
  taskId: string | null
  occurrenceKey: string | null
  title: string
  description: string
  openCount: number
  frequency: 'daily' | 'weekly' | 'monthly' | 'yearly'
  interval: number
}
export type RecurringFrequency = 'daily' | 'weekly' | 'monthly' | 'quarterly' | 'yearly'

export function WeeklyRecurringView({ query, frequency }: { query: string; frequency: RecurringFrequency }) {
  const [items, setItems] = useState<Arrangement[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [editing, setEditing] = useState<Arrangement | null>(null)
  const load = useCallback(async () => {
    setLoading(true)
    try { setItems(await invokeData('recurring_list')); setError('') }
    catch (e) { setError(contentError(e)) }
    finally { setLoading(false) }
  }, [])
  useEffect(() => { void load(); return onDataChanged(['recurrence', 'tasks'], () => { void load() }) }, [load])
  const shown = items.filter(a => {
    const matchesFrequency = frequency === 'quarterly'
      ? a.frequency === 'monthly' && a.interval === 3
      : a.frequency === frequency && (frequency !== 'monthly' || a.interval !== 3)
    return matchesFrequency && `${a.title} ${a.description}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
  })
  const frequencyLabel: Record<RecurringFrequency, string> = {
    daily: '每日', weekly: '每周', monthly: '每月', quarterly: '每季度', yearly: '每年',
  }
  return <section className="weekly-recurring">
    <p className="setgroup__hint">每个重复系列只显示一次，不会把每日生成的任务逐条展开。点击“修改重复规则”可查看或调整重复日期、节假日和结束条件。</p>
    <button className="btn btn--quiet btn--sm" onClick={() => void load()} disabled={loading}>刷新重复安排</button>
    {error && <p className="alert alert--error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {loading && <p>正在读取重复安排…</p>}
    {!loading && !error && !shown.length && <p>没有匹配的{frequencyLabel[frequency]}重复安排。点击右上角“新建”可创建这一类重复任务。</p>}
    {shown.map((a) => <article key={a.seriesId} className="weekly-recurring__item">
      <h3>{a.title}</h3><p>{a.description}</p>
      <p className="setgroup__hint">{a.openCount ? `已生成 ${a.openCount} 次未完成任务` : '当前没有未完成实例'}</p>
      {a.taskId && a.occurrenceKey && <button className="btn btn--quiet btn--sm" onClick={() => setEditing(a)}>修改重复规则</button>}
    </article>)}
    {editing?.taskId && editing.occurrenceKey && <SeriesRuleDialog taskId={editing.taskId} seriesId={editing.seriesId} occurrenceKey={editing.occurrenceKey}
      onClose={() => setEditing(null)} onSaved={(message) => { setNotice(message); void load() }} />}
  </section>
}
