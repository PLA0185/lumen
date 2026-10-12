import { useEffect, useMemo, useRef, useState } from 'react'
import { addDays, format, startOfWeek } from 'date-fns'
import * as ipc from '../lib/ipc'
import { todayRange } from '../lib/datetime'

interface Props {
  selectedDate: string
  onChange: (date: string) => void
}

const weekdays = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
const today = () => format(new Date(), 'yyyy-MM-dd')

export function TodayDateSelector({ selectedDate, onChange }: Props) {
  const selectedDay = useMemo(() => new Date(`${selectedDate}T00:00:00`), [selectedDate])
  const dates = useMemo(() => {
    const monday = startOfWeek(selectedDay, { weekStartsOn: 1 })
    return Array.from({ length: 7 }, (_, i) => addDays(monday, i))
  }, [selectedDay])
  const [counts, setCounts] = useState<Array<number | null>>(() => Array(7).fill(null))
  const request = useRef(0)

  useEffect(() => {
    const current = ++request.current
    setCounts(Array(7).fill(null))
    void Promise.all(dates.map(async date => {
      const range = todayRange(date)
      return ipc.countTasks({
        statuses: ['todo', 'doing', 'waiting'],
        plannedFrom: range.start,
        plannedTo: range.end,
      }).then(result => result.total)
    })).then(next => {
      if (request.current === current) setCounts(next)
    }).catch(() => {
      if (request.current === current) setCounts(Array(7).fill(null))
    })
    return () => { request.current += 1 }
  }, [dates])

  const moveWeek = (offset: number) => onChange(format(addDays(selectedDay, offset * 7), 'yyyy-MM-dd'))
  const isToday = selectedDate === today()

  return <section className="today-date-strip" aria-label="按日期查看任务">
    <nav className="today-date-strip__week" aria-label="一周日期">
      <button type="button" aria-label="上一周" onClick={() => moveWeek(-1)}>‹</button>
      {dates.map((date, index) => {
        const value = format(date, 'yyyy-MM-dd')
        const active = selectedDate === value
        return <button
          key={value}
          type="button"
          className={active ? 'is-selected' : ''}
          aria-pressed={active}
          aria-label={`${weekdays[index]} ${format(date, 'yyyy年M月d日')}${counts[index] ? `，${counts[index]}项待办` : ''}`}
          onClick={() => onChange(value)}
        >
          <span>{weekdays[index]}</span>
          <strong>{format(date, 'MM-dd')}</strong>
          {counts[index] !== null && counts[index]! > 0 && <i aria-hidden="true" />}
        </button>
      })}
      <button type="button" aria-label="下一周" onClick={() => moveWeek(1)}>›</button>
    </nav>
    <div className="today-date-strip__summary">
      <span>{isToday ? '今天' : format(selectedDay, 'yyyy年M月d日')} · {counts[dates.findIndex(d => format(d, 'yyyy-MM-dd') === selectedDate)] ?? '…'} 项未完成</span>
      {!isToday && <button type="button" onClick={() => onChange(today())}>回到今天</button>}
    </div>
  </section>
}
