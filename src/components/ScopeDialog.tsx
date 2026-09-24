/**
 * 重复任务的范围选择对话框（任务书 §5 核心交互）。
 *
 * ## 为什么必须是模态且不能有默认选中
 *
 * §5 要求用户在编辑重复任务时明确选择「仅此次 / 此次及以后 / 整个系列」。
 * 这里刻意**不预选**任何一项，用户必须主动点选才能确认——
 * 因为任何一种误选都会造成用户没预期的后果（最严重的是"整个系列"
 * 悄悄改掉了全部未来安排）。多一次点击，换一次明确的意图表达。
 *
 * ## 为什么要把影响面写出来
 *
 * 每个选项下方直接显示具体数字（几次发生、几次已完成历史），
 * 而不是"这将影响多个任务"这种含糊说法。用户据此才能判断该选哪个。
 *
 * ## 为什么"整个系列"且影响历史时要二次确认
 *
 * §5 明确要求「如用户选择整个系列且操作会影响历史，须预览影响并要求明确确认」。
 * 因此当 completedBefore > 0 时，勾选框会出现在"整个系列"选项下方，
 * 不勾选就无法确认。
 */

import { useEffect, useState } from 'react'
import * as rec from '../lib/recurrence-ipc'
import { IpcError } from '../lib/ipc'
import type { DeleteMode, EditScope, ScopeInfo } from '../lib/recurrence-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

export type ScopeIntent = 'edit' | 'delete'

interface ScopeDialogProps {
  taskId: string
  taskTitle: string
  intent: ScopeIntent
  onCancel: () => void
  onConfirm: (scope: EditScope, confirmHistory: boolean) => Promise<void> | void
  /** 是否允许选择"此次及以后"（编辑时间时可用；某些操作不适用） */
  allowThisAndFuture?: boolean
  allowWholeSeries?: boolean
  restrictedReason?: string
}

export function ScopeDialog({
  taskId,
  taskTitle,
  intent,
  onCancel,
  onConfirm,
  allowThisAndFuture = true,
  allowWholeSeries = true,
  restrictedReason,
}: ScopeDialogProps) {
  const [info, setInfo] = useState<ScopeInfo | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  /** 刻意不预选：用户必须主动表达意图 */
  const [scope, setScope] = useState<EditScope | null>(null)
  const [confirmHistory, setConfirmHistory] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      try {
        setInfo(await rec.recurringScopeInfo(taskId))
      } catch (e) {
        setLoadError(errText(e))
      }
    })()
  }, [taskId])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])

  /** 会影响历史时，必须额外确认 */
  const affectsHistory = (info?.completedBefore ?? 0) > 0
  const needHistoryCheck = (scope === 'whole_series' || scope === 'this_and_future') && affectsHistory
  const canConfirm = scope !== null && (!needHistoryCheck || confirmHistory) && !busy

  const submit = async () => {
    if (!scope) return
    setBusy(true)
    setError(null)
    try {
      await onConfirm(scope, needHistoryCheck ? confirmHistory : false)
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const isDelete = intent === 'delete'
  const title = isDelete ? '删除重复任务' : '修改重复任务'

  /** 每个选项的影响说明 */
  const notes = info?.notes
  const total = info?.totalInstances ?? 0
  const done = info?.completed ?? 0

  const options: Array<{
    value: EditScope
    label: string
    desc: string
    disabled: boolean
    disabledReason?: string
  }> = [
    {
      value: 'this_only',
      label: isDelete ? '仅删除这一次' : '仅修改这一次',
      desc:
        notes?.thisOnly ??
        '只影响这一次发生，同系列的其它发生完全不受影响。',
      disabled: false,
    },
    {
      value: 'this_and_future',
      label: isDelete ? '删除这一次及以后' : '修改这一次及以后',
      desc:
        notes?.thisAndFuture ??
        '这一次以及之后尚未完成的发生都会按新设置重算。',
      disabled: !allowThisAndFuture,
      disabledReason: allowThisAndFuture
        ? undefined
        : (restrictedReason ?? '该操作不适用于「此次及以后」。'),
    },
    {
      value: 'whole_series',
      label: isDelete ? '删除整个系列' : '修改整个系列',
      desc:
        notes?.wholeSeries ??
        '修改系列的基础设置，影响所有未完成的发生。',
      disabled: !allowWholeSeries,
      disabledReason: allowWholeSeries ? undefined : (restrictedReason ?? '此次改期只支持「仅此次」。'),
    },
  ]

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="scope-title">
      <div className="modal">
        <h2 className="modal__title" id="scope-title">
          {title}
        </h2>
        <p className="modal__text">
          「{taskTitle}」是重复任务的一次发生。
          {isDelete
            ? '请选择删除范围——不同范围影响的发生数量差别很大。'
            : '请选择修改范围——不同范围影响的发生数量差别很大。'}
        </p>

        {loadError ? (
          <div className="alert alert--error" role="alert">
            <span className="selectable">{loadError}</span>
          </div>
        ) : info === null ? (
          <p className="modal__text">正在统计影响范围…</p>
        ) : (
          <>
            {/* 系列概况：让用户对"一共有多少次"有概念 */}
            <div className="scopeinfo">
              <span>
                这是第 <strong>{info.occurrenceIndex ?? '?'}</strong> 次发生
              </span>
              <span>
                系列共 <strong>{total}</strong> 次
              </span>
              <span>
                已完成 <strong>{done}</strong> 次
              </span>
              {(info.exceptions ?? 0) > 0 && (
                <span title="被单独修改过的次数，改规则时会被保护">
                  单独改过 <strong>{info.exceptions}</strong> 次
                </span>
              )}
              {(info.segments ?? 0) > 0 && (
                <span title="从某次起改用过新规则">
                  规则已分段 <strong>{info.segments}</strong> 次
                </span>
              )}
            </div>

            <div className="radio-group">
              {options.map((o) => (
                <label
                  key={o.value}
                  className={`radio${o.disabled ? ' radio--disabled' : ''}`}
                  title={o.disabledReason}
                >
                  <input
                    type="radio"
                    name="scope"
                    value={o.value}
                    checked={scope === o.value}
                    disabled={o.disabled}
                    onChange={() => {
                      setScope(o.value)
                      setConfirmHistory(false)
                    }}
                  />
                  <span>
                    <strong>{o.label}</strong>
                    <br />
                    <span className="radio__hint">{o.disabled ? o.disabledReason : o.desc}</span>
                  </span>
                </label>
              ))}
            </div>

            {/* 修改未来或整个系列时，若存在更早的已完成发生，必须明确确认。 */}
            {needHistoryCheck && (
              <label className="checkbox histcheck">
                <input
                  type="checkbox"
                  checked={confirmHistory}
                  onChange={(e) => setConfirmHistory(e.target.checked)}
                />
                <span>
                  我了解这会影响到 <strong>{info.completedBefore}</strong> 个已完成的历史记录。
                  程序会保留它们的完成状态与完成时间，但系列的共同设置会被更新。
                </span>
              </label>
            )}
          </>
        )}

        {error && (
          <div className="alert alert--error" role="alert" style={{ marginTop: 10 }}>
            <span className="selectable">{error}</span>
          </div>
        )}

        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button
            type="button"
            className={isDelete ? 'btn btn--danger-solid' : 'btn btn--primary'}
            disabled={!canConfirm}
            title={scope === null ? '请先选择一个范围' : undefined}
            onClick={() => void submit()}
          >
            {busy ? '处理中…' : isDelete ? '确认删除' : '确认修改'}
          </button>
        </div>
        {scope === null && !loadError && info !== null && (
          <p className="setgroup__hint" style={{ textAlign: 'right', marginTop: 6 }}>
            请先选择上面的一个范围
          </p>
        )}
      </div>
    </div>
  )
}

/** 把 DeleteMode 与 EditScope 统一（两者取值相同，语义一致） */
export function scopeToDeleteMode(scope: EditScope): DeleteMode {
  return scope
}
