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
}
export function WeeklyRecurringView({ query }: { query: string }) {
  const [items, setItems] = useState<Arrangement[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [editing, setEditing] = useState<Arrangement | null>(null)
  const load = useCallback(async () => {
    setLoading(true)
    try { setItems(await invokeData('recurring_weekly_list')); setError('') }
    catch (e) { setError(contentError(e)) }
    finally { setLoading(false) }
  }, [])
  useEffect(() => { void load(); return onDataChanged(['recurrence', 'tasks'], () => { void load() }) }, [load])
  const shown = items.filter((a) => `${a.title} ${a.description}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  return <section className="weekly-recurring">
    <p className="setgroup__hint">每个重复系列显示一次。这里查看每周指定星期几的安排；点击“修改重复规则”可设置多天、节假日和结束条件。</p>
    <button className="btn btn--quiet btn--sm" onClick={() => void load()} disabled={loading}>刷新重复安排</button>
    {error && <p className="alert alert--error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {loading && <p>正在读取重复安排…</p>}
    {!loading && !error && !shown.length && <p>没有匹配的每周重复安排。点击右上角“新建”选择执行星期。</p>}
    {shown.map((a) => <article key={a.seriesId} className="weekly-recurring__item">
      <h3>{a.title}</h3><p>{a.description}</p>
      <p className="setgroup__hint">{a.openCount ? `已生成 ${a.openCount} 次未完成任务` : '当前没有未完成实例'}</p>
      {a.taskId && a.occurrenceKey && <button className="btn btn--quiet btn--sm" onClick={() => setEditing(a)}>修改重复规则</button>}
    </article>)}
    {editing?.taskId && editing.occurrenceKey && <SeriesRuleDialog taskId={editing.taskId} seriesId={editing.seriesId} occurrenceKey={editing.occurrenceKey}
      onClose={() => setEditing(null)} onSaved={(message) => { setNotice(message); void load() }} />}
  </section>
}
