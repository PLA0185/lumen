import { useEffect, useState } from 'react'
import * as rec from '../lib/recurrence-ipc'
import { IpcError } from '../lib/ipc'
import { RuleEditor, type RuleEditorValue } from './RuleEditor'

interface Props {
  taskId: string
  seriesId: string
  occurrenceKey: string
  onClose: () => void
  onSaved: (message: string) => void
}

function localAt(utc: string, tzid: string): string {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: tzid,
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23',
  }).formatToParts(new Date(utc))
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? ''
  return `${get('year')}-${get('month')}-${get('day')}T${get('hour')}:${get('minute')}:${get('second')}`
}

function safeLocalAt(utc: string, tzid: string, fallbackTzid: string): string {
  try { return localAt(utc, tzid) }
  catch { return localAt(utc, fallbackTzid) }
}

function effectiveRuleAt(detail: rec.SeriesDetail, key: string): { rrule: string; tzid: string } {
  let rrule = detail.series.rrule
  let tzid = detail.series.tzid
  for (const segment of [...detail.segments].sort((a, b) => a.ruleVersion - b.ruleVersion)) {
    if (segment.effectiveFromOccurrence > key) continue
    if (segment.newRrule) rrule = segment.newRrule
    if (segment.newTzid) tzid = segment.newTzid
  }
  return { rrule, tzid }
}

export function SeriesRuleDialog({ taskId, seriesId, occurrenceKey, onClose, onSaved }: Props) {
  const [detail, setDetail] = useState<rec.SeriesDetail | null>(null)
  const [scope, setScope] = useState<'this_and_future' | 'whole_series'>('this_and_future')
  const [rrule, setRrule] = useState('')
  const [tzid, setTzid] = useState('')
  const [confirmHistory, setConfirmHistory] = useState(false)
  const [completedBefore, setCompletedBefore] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void Promise.all([rec.recurringGet(seriesId), rec.recurringScopeInfo(taskId)])
      .then(([d, info]) => {
        if (!active) return
        const effective = effectiveRuleAt(d, occurrenceKey)
        setRrule(effective.rrule)
        setTzid(effective.tzid)
        setCompletedBefore(info.completedBefore ?? 0)
        setDetail(d)
      })
      .catch((e) => { if (active) setError(e instanceof IpcError ? e.userMessage() : String(e)) })
    return () => { active = false }
  }, [seriesId, taskId, occurrenceKey])

  const save = async () => {
    if (!rrule || !detail) return
    setBusy(true)
    setError(null)
    try {
      try { new Intl.DateTimeFormat('en', { timeZone: tzid }) }
      catch { throw new Error('时区名称无效。请选择列表中的时区，或输入有效的 IANA 时区名称。') }
      const result = await rec.recurringEditInstance({
        taskId, scope, patch: { tzid }, newRrule: rrule, confirmHistory,
      })
      onSaved(result.message)
      onClose()
    } catch (e) {
      setError(e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      setBusy(false)
    }
  }

  const selectScope = (nextScope: 'this_and_future' | 'whole_series') => {
    setScope(nextScope)
    if (detail) {
      const effective = effectiveRuleAt(detail, nextScope === 'whole_series'
        ? new Date().toISOString() : occurrenceKey)
      setRrule(effective.rrule)
      setTzid(effective.tzid)
    }
  }

  const now = new Date().toISOString()
  const previewKey = detail && scope === 'whole_series'
    ? detail.nextFutureOccurrenceKey ?? (occurrenceKey > now ? occurrenceKey : now)
    : occurrenceKey
  const previousTzid = detail ? effectiveRuleAt(detail, previewKey).tzid : 'UTC'
  const start = detail
    ? safeLocalAt(previewKey, tzid || previousTzid, previousTzid).slice(0, 10)
      + safeLocalAt(previewKey, previousTzid, detail.series.tzid).slice(10)
    : ''
  const ruleValue: RuleEditorValue = {
    rrule,
    dtstartLocal: start,
    hasStartTime: detail?.series.hasStartTime === 1,
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="series-rule-title">
      <div className="modal modal--wide">
        <h2 className="modal__title" id="series-rule-title">修改重复规则</h2>
        {error && <div className="alert alert--error" role="alert">{error}</div>}
        {!detail ? <p>正在读取重复规则…</p> : <>
          {detail.series.terminatedFromOccurrenceKey && (
            <div className="alert alert--warn" role="note">
              {detail.series.terminatedFromOccurrenceKey.startsWith('1970-')
                ? '这个系列已随其项目或分类停止。'
                : `这个系列已经从 ${safeLocalAt(detail.series.terminatedFromOccurrenceKey,
                  tzid || detail.series.tzid, detail.series.tzid).slice(0, 10)} 起停止。`}
              修改规则不会恢复已停止的后续任务。
            </div>
          )}
          <fieldset className="formfieldset">
            <legend>应用范围</legend>
            <label className="checkbox">
              <input type="radio" name="rule-scope" checked={scope === 'this_and_future'}
                onChange={() => selectScope('this_and_future')} />
              从这一次开始修改以后
            </label>
            <label className="checkbox">
              <input type="radio" name="rule-scope" checked={scope === 'whole_series'}
                onChange={() => selectScope('whole_series')} />
              修改整个重复系列
            </label>
            <p className="setgroup__hint">有附件、提醒、子任务或其它个人修改的发生会保留。新规则不再覆盖的日期也不会静默删除这些数据。</p>
          </fieldset>
          <div className="formrow">
            <label className="formlabel" htmlFor="series-timezone">时区</label>
            <input id="series-timezone" className="input" list="series-timezones"
              value={tzid} onChange={(e) => setTzid(e.target.value)}
              aria-describedby="series-timezone-hint" />
            <datalist id="series-timezones">
              <option value="Asia/Shanghai" />
              <option value="America/New_York" />
              <option value="Europe/London" />
              <option value="UTC" />
            </datalist>
            <p className="setgroup__hint" id="series-timezone-hint">改时区后，之后的任务会按新时区的相同本地时刻安排；之前的任务保留。</p>
          </div>
          <RuleEditor key={`${seriesId}-${scope}-${tzid}`} value={ruleValue} fixedStart
            tzid={tzid} onChange={(next) => setRrule(next.rrule)} />
          {completedBefore > 0 && <label className="checkbox">
            <input type="checkbox" checked={confirmHistory}
              onChange={(e) => setConfirmHistory(e.target.checked)} />
            我了解已有 {completedBefore} 条完成记录会保留，规则只重建可安全替换的实例
          </label>}
        </>}
        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onClose} disabled={busy}>取消</button>
          <button type="button" className="btn btn--primary" onClick={() => void save()}
            disabled={!detail || busy || (completedBefore > 0 && !confirmHistory)}>
            {busy ? '保存中…' : '保存重复规则'}
          </button>
        </div>
      </div>
    </div>
  )
}
