/**
 * 重复规则编辑器（任务书 §5）。
 *
 * ## 设计要点
 *
 * 1. **规则预览必须可见**（§5 明确要求"提供规则预览，例如接下来 10 次发生日期"）。
 *    预览由 Rust 侧计算，因为月末策略、闰年策略都在那里实现——
 *    前端重复实现一份，预览与实际生成就会不一致。
 * 2. **边界策略必须说明**（§5 要求"定义一致的处理策略并展示给用户"）。
 *    一旦规则涉及月末（如每月 31 日），界面直接写出"该月跳过"的约定。
 * 3. 控件随频率变化：按星期只在每周出现，按日期/第几个星期只在每月出现。
 *    不显示无关控件，减少用户误设的机会。
 */

import { useEffect, useMemo, useState } from 'react'
import * as rec from '../lib/recurrence-ipc'
import { IpcError } from '../lib/ipc'
import { format } from 'date-fns'
import type { EndCondition, Freq, SetPos } from '../lib/recurrence-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

export interface RuleEditorValue {
  rrule: string
  /** 首次发生的本地墙上时间 `YYYY-MM-DDTHH:MM:SS` */
  dtstartLocal: string
  hasStartTime: boolean
}

interface RuleEditorProps {
  value: RuleEditorValue
  onChange: (v: RuleEditorValue) => void
  /** 起始日期的初始值（默认今天） */
  defaultDate?: Date
}

const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7]

/** 从 RRULE 字符串反推控件状态（用于编辑已有系列时回显） */
function parseRrule(rrule: string): {
  freq: Freq
  interval: number
  byWeekday: number[]
  weekdaysOnly: boolean
  byMonthday: number[]
  setPos: SetPos | null
} {
  const out = {
    freq: 'weekly' as Freq,
    interval: 1,
    byWeekday: [] as number[],
    weekdaysOnly: false,
    byMonthday: [] as number[],
    setPos: null as SetPos | null,
  }
  const codes: Record<string, number> = { MO: 1, TU: 2, WE: 3, TH: 4, FR: 5, SA: 6, SU: 7 }

  const parts = rrule.split(';').map((s) => s.trim()).filter(Boolean)
  let byday: number[] = []
  let setpos: number | null = null

  for (const p of parts) {
    const [k, v] = p.split('=')
    if (!k || v === undefined) continue
    const key = k.toUpperCase()
    if (key === 'FREQ') {
      const f = v.toLowerCase()
      if (f === 'daily' || f === 'weekly' || f === 'monthly' || f === 'yearly') out.freq = f
    } else if (key === 'INTERVAL') {
      const n = Number(v)
      if (Number.isFinite(n) && n >= 1) out.interval = n
    } else if (key === 'BYDAY') {
      byday = v
        .split(',')
        .map((c) => codes[c.trim().toUpperCase()] ?? 0)
        .filter((n) => n > 0)
    } else if (key === 'BYMONTHDAY') {
      out.byMonthday = v
        .split(',')
        .map((d) => Number(d.trim()))
        .filter((n) => Number.isFinite(n) && n >= 1 && n <= 31)
    } else if (key === 'BYSETPOS') {
      const n = Number(v)
      if (Number.isFinite(n)) setpos = n
    }
  }

  // 周一到周五整体出现时视为"仅工作日"
  if (
    byday.length === 5 &&
    [1, 2, 3, 4, 5].every((d) => byday.includes(d))
  ) {
    out.weekdaysOnly = true
  } else {
    out.byWeekday = byday
  }

  if (setpos !== null && byday.length > 0) {
    out.setPos = { nth: setpos, weekday: byday[0]! }
  }

  return out
}

/** 从 `YYYY-MM-DDTHH:MM:SS` 拆出日期与时间（时间部分为 `00:00:00` 时视为仅日期） */
function splitDtstart(s: string): { date: string; time: string } {
  const m = s.match(/^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/)
  if (!m || !m[1] || !m[2]) return { date: '', time: '' }
  const date = m[1]
  // 时刻为 00:00 时视为"仅日期"——与后端 has_start_time = 0 的约定一致
  const time = m[2] === '00:00' ? '' : m[2]
  return { date, time }
}

export function RuleEditor({ value, onChange, defaultDate }: RuleEditorProps) {
  // 初始状态从传入的 value 反推，这样编辑已有系列时能正确回显；
  // 新建时 value 是默认值，行为不变。
  const initial = useMemo(() => parseRrule(value.rrule), [value.rrule])
  const initialDt = useMemo(() => splitDtstart(value.dtstartLocal), [value.dtstartLocal])

  const [freq, setFreq] = useState<Freq>(initial.freq)
  const [interval, setInterval] = useState(initial.interval)
  const [byWeekday, setByWeekday] = useState<number[]>(initial.byWeekday)
  const [weekdaysOnly, setWeekdaysOnly] = useState(initial.weekdaysOnly)
  const [monthMode, setMonthMode] = useState<'day' | 'setpos'>(
    initial.setPos ? 'setpos' : 'day',
  )
  const [byMonthday, setByMonthday] = useState<number[]>(initial.byMonthday)
  const [setPos, setSetPos] = useState<SetPos | null>(initial.setPos)
  const [endKind, setEndKind] = useState<EndCondition['kind']>('never')
  const [endDate, setEndDate] = useState(() =>
    format(new Date(Date.now() + 90 * 86400000), 'yyyy-MM-dd'),
  )
  const [endCount, setEndCount] = useState(10)

  const [dateStr, setDateStr] = useState(
    () => initialDt.date || format(defaultDate ?? new Date(), 'yyyy-MM-dd'),
  )
  const [timeStr, setTimeStr] = useState(initialDt.time)

  const [preview, setPreview] = useState<string[]>([])
  const [previewError, setPreviewError] = useState<string | null>(null)

  /** 组装结束条件 */
  const end: EndCondition = useMemo(() => {
    switch (endKind) {
      case 'until':
        return { kind: 'until', date: endDate }
      case 'count':
        return { kind: 'count', count: endCount }
      default:
        return { kind: 'never' }
    }
  }, [endKind, endDate, endCount])

  /** 组装 RRULE 并上报给父组件 */
  const rrule = useMemo(() => {
    return rec.buildRrule({
      freq,
      interval,
      byWeekday,
      weekdaysOnly,
      byMonthday: monthMode === 'day' ? byMonthday : undefined,
      setPos: monthMode === 'setpos' ? setPos : null,
      end,
    })
  }, [freq, interval, byWeekday, weekdaysOnly, monthMode, byMonthday, setPos, end])

  const dtstartLocal = timeStr ? `${dateStr}T${timeStr}:00` : `${dateStr}T00:00:00`
  const hasStartTime = Boolean(timeStr)

  useEffect(() => {
    onChange({ rrule, dtstartLocal, hasStartTime })
    // onChange 由父组件用 useCallback 稳定；这里只依赖三个值
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rrule, dtstartLocal, hasStartTime])

  /** 拉取预览（后端计算，保证与真实生成一致） */
  useEffect(() => {
    let cancelled = false
    const t = window.setTimeout(() => {
      void (async () => {
        try {
          const list = await rec.recurringPreview({
            rrule,
            dtstartLocal,
            hasStartTime,
            count: 10,
          })
          if (!cancelled) {
            setPreview(list)
            setPreviewError(null)
          }
        } catch (e) {
          if (!cancelled) {
            setPreview([])
            setPreviewError(errText(e))
          }
        }
      })()
    }, 250) // 轻微防抖，避免连续调整控件时频繁请求
    return () => {
      cancelled = true
      window.clearTimeout(t)
    }
  }, [rrule, dtstartLocal, hasStartTime])

  const toggleWeekday = (w: number) => {
    setWeekdaysOnly(false)
    setByWeekday((prev) => (prev.includes(w) ? prev.filter((x) => x !== w) : [...prev, w].sort()))
  }

  const toggleMonthday = (d: number) => {
    setByMonthday((prev) => (prev.includes(d) ? prev.filter((x) => x !== d) : [...prev, d].sort()))
  }

  const needsEdgeNote = rec.touchesMonthEdge({
    freq,
    byMonthday: monthMode === 'day' ? byMonthday : undefined,
    setPos: monthMode === 'setpos' ? setPos : null,
  })

  return (
    <div className="ruleeditor">
      {/* ---------------- 频率与间隔 ---------------- */}
      <div className="formrow">
        <span className="formlabel">重复方式</span>
        <div className="rulerow">
          <span>每</span>
          <input
            type="number"
            className="input input--tiny"
            min={1}
            max={365}
            value={interval}
            aria-label="间隔数量"
            onChange={(e) => setInterval(Math.max(1, Math.min(365, Number(e.target.value) || 1)))}
          />
          <select
            className="input input--compact"
            value={freq}
            aria-label="重复频率"
            onChange={(e) => setFreq(e.target.value as Freq)}
          >
            <option value="daily">天</option>
            <option value="weekly">周</option>
            <option value="monthly">月</option>
            <option value="yearly">年</option>
          </select>
        </div>
      </div>

      {/* ---------------- 每周：星期选择 ---------------- */}
      {freq === 'weekly' && (
        <div className="formrow">
          <span className="formlabel">在这些天</span>
          <div className="weekpicker">
            {WEEKDAYS.map((w) => {
              const on = !weekdaysOnly && byWeekday.includes(w)
              return (
                <button
                  key={w}
                  type="button"
                  className={`weekbtn${on ? ' weekbtn--on' : ''}${
                    weekdaysOnly && w <= 5 ? ' weekbtn--implied' : ''
                  }`}
                  aria-pressed={on}
                  aria-label={`周${rec.WEEKDAY_LABELS[w]}`}
                  onClick={() => toggleWeekday(w)}
                >
                  {rec.WEEKDAY_LABELS[w]}
                </button>
              )
            })}
          </div>
          <label className="checkbox" style={{ marginTop: 6, marginBottom: 0 }}>
            <input
              type="checkbox"
              checked={weekdaysOnly}
              onChange={(e) => {
                setWeekdaysOnly(e.target.checked)
                if (e.target.checked) setByWeekday([])
              }}
            />
            仅工作日（周一至周五）
          </label>
          {!weekdaysOnly && byWeekday.length === 0 && (
            <p className="setgroup__hint" style={{ marginTop: 4 }}>
              未选择具体星期时，将沿用起始日期所在的星期几。
            </p>
          )}
        </div>
      )}

      {/* ---------------- 每月：按日期 / 第几个星期 ---------------- */}
      {freq === 'monthly' && (
        <div className="formrow">
          <span className="formlabel">在每月的</span>
          <div className="segmented" role="radiogroup" aria-label="每月重复方式">
            <button
              type="button"
              role="radio"
              aria-checked={monthMode === 'day'}
              className={`segmented__item${monthMode === 'day' ? ' segmented__item--on' : ''}`}
              onClick={() => setMonthMode('day')}
            >
              具体日期
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={monthMode === 'setpos'}
              className={`segmented__item${monthMode === 'setpos' ? ' segmented__item--on' : ''}`}
              onClick={() => {
                setMonthMode('setpos')
                if (!setPos) setSetPos({ nth: 1, weekday: 1 })
              }}
            >
              第几个星期几
            </button>
          </div>

          {monthMode === 'day' ? (
            <>
              <div className="daypicker">
                {Array.from({ length: 31 }, (_, i) => i + 1).map((d) => (
                  <button
                    key={d}
                    type="button"
                    className={`daybtn${byMonthday.includes(d) ? ' daybtn--on' : ''}`}
                    aria-pressed={byMonthday.includes(d)}
                    aria-label={`${d} 日`}
                    onClick={() => toggleMonthday(d)}
                  >
                    {d}
                  </button>
                ))}
                {/*
                  "每月最后一天"用 RFC 5545 的负数写法（BYMONTHDAY=-1）。
                  后端本来就该支持它，但此前解析器只接受正数，导致这条最常见的
                  需求既选不出来也存不下——所以这里补一个明确的入口。
                */}
                <button
                  type="button"
                  className={`daybtn daybtn--wide${byMonthday.includes(-1) ? ' daybtn--on' : ''}`}
                  aria-pressed={byMonthday.includes(-1)}
                  aria-label="每月最后一天"
                  title="无论大小月都落在当月最后一天（2 月是 28/29 日）"
                  onClick={() => toggleMonthday(-1)}
                >
                  最后一天
                </button>
              </div>
              {byMonthday.length === 0 && (
                <p className="setgroup__hint" style={{ marginTop: 4 }}>
                  未选择日期时，将沿用起始日期的日（例如起始日是 15 日，则每月 15 日）。
                </p>
              )}
            </>
          ) : (
            <div className="rulerow">
              <select
                className="input input--compact"
                value={setPos?.nth ?? 1}
                aria-label="第几个"
                onChange={(e) =>
                  setSetPos((p) => ({ nth: Number(e.target.value), weekday: p?.weekday ?? 1 }))
                }
              >
                <option value={1}>第 1 个</option>
                <option value={2}>第 2 个</option>
                <option value={3}>第 3 个</option>
                <option value={4}>第 4 个</option>
                <option value={5}>第 5 个</option>
                <option value={-1}>最后一个</option>
              </select>
              <select
                className="input input--compact"
                value={setPos?.weekday ?? 1}
                aria-label="星期几"
                onChange={(e) =>
                  setSetPos((p) => ({ nth: p?.nth ?? 1, weekday: Number(e.target.value) }))
                }
              >
                {WEEKDAYS.map((w) => (
                  <option key={w} value={w}>
                    周{rec.WEEKDAY_LABELS[w]}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
      )}

      {/* ---------------- 结束条件 ---------------- */}
      <div className="formrow">
        <span className="formlabel">结束</span>
        <div className="rulerow">
          <select
            className="input input--compact"
            value={endKind}
            aria-label="结束条件"
            onChange={(e) => setEndKind(e.target.value as EndCondition['kind'])}
          >
            <option value="never">永不结束</option>
            <option value="until">直到某日</option>
            <option value="count">重复若干次后结束</option>
          </select>
          {endKind === 'until' && (
            <input
              type="date"
              className="input input--compact"
              value={endDate}
              aria-label="结束日期"
              onChange={(e) => setEndDate(e.target.value)}
            />
          )}
          {endKind === 'count' && (
            <>
              <input
                type="number"
                className="input input--tiny"
                min={1}
                max={999}
                value={endCount}
                aria-label="重复次数"
                onChange={(e) => setEndCount(Math.max(1, Math.min(999, Number(e.target.value) || 1)))}
              />
              <span>次</span>
            </>
          )}
        </div>
      </div>

      {/* ---------------- 首次发生时间 ---------------- */}
      <div className="formrow">
        <span className="formlabel">
          首次发生 <span className="req">*</span>
        </span>
        <div className="dtd">
          <input
            type="date"
            className="input"
            value={dateStr}
            aria-label="首次发生日期"
            onChange={(e) => setDateStr(e.target.value)}
          />
          <input
            type="time"
            className="input"
            value={timeStr}
            aria-label="首次发生时刻（留空表示仅日期）"
            onChange={(e) => setTimeStr(e.target.value)}
          />
        </div>
        <p className="setgroup__hint" style={{ marginTop: 4 }}>
          留空时刻表示全天任务，不会被视为当天零点到期。
        </p>
      </div>

      {/* ---------------- 边界策略说明（§5 要求展示给用户） ---------------- */}
      {needsEdgeNote && (
        <div className="alert alert--warn" role="note" style={{ marginTop: 8 }}>
          <span>
            <strong>关于不存在的日期：</strong>
            当某个重复日在该月不存在时（例如每月 31 日遇到只有 30 天的月份），
            <strong>该月会跳过这一次</strong>，不会自动挪到月底。
            闰年的 2 月 29 日会正常发生。
          </span>
        </div>
      )}

      {/* ---------------- 规则预览（§5 明确要求） ---------------- */}
      <div className="rulepreview">
        <div className="rulepreview__head">
          <span>接下来 10 次发生</span>
          <code className="rulepreview__rrule selectable">{rrule}</code>
        </div>
        {previewError ? (
          <div className="formerr" role="alert">
            {previewError}
          </div>
        ) : preview.length === 0 ? (
          <p className="setgroup__hint">该规则不会产生任何发生，请检查设置。</p>
        ) : (
          <ol className="rulepreview__list">
            {preview.map((s, i) => (
              <li key={`${i}-${s}`}>{s}</li>
            ))}
          </ol>
        )}
      </div>
    </div>
  )
}
