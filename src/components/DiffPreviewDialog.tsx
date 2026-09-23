/**
 * AI 差异预览与确认（任务书 §6 的核心界面）。
 *
 * ## 这个对话框存在的意义
 *
 * §6 的硬要求是：
 * > 所有 AI 输出先显示差异预览，用户选择接受、修改或放弃后才写入数据库。
 *
 * 因此它不是"结果展示"，而是一道**必须经过的确认闸门**：
 * - 每一条改动都逐项列出「改动前 → 改动后」，用户能看清 AI 到底要做什么；
 * - 用户可**逐条勾选**，只接受其中一部分；
 * - 存在校验错误时禁用"写入"，并明确说明原因；
 * - 写入按钮的文案直接写明会发生什么（"写入 3 条改动"），
 *   而不是含糊的"确定"。
 *
 * ## 为什么要展示校验问题与数据范围
 *
 * - 校验问题：AI 可能返回不存在的任务 ID 或无法解析的日期，
 *   这些必须让用户看见（§6 要求"无效输出展示可读错误"）。
 * - 数据范围：§6 要求说明发送给模型的数据范围，用户有权知道
 *   自己被发出去了什么。
 * - token 用量：§6 要求"查看调用错误及估计消耗"。
 */

import { useMemo, useState } from 'react'
import * as ai from '../lib/ai-ipc'
import { IpcError } from '../lib/ipc'
import type { DiffPreview } from '../lib/ai-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface DiffPreviewDialogProps {
  preview: DiffPreview
  onClose: () => void
  onApplied: (created: number, updated: number) => void
  /** 重新生成（把上一次的操作重跑一遍） */
  onRegenerate?: () => void
}

export function DiffPreviewDialog({
  preview,
  onClose,
  onApplied,
  onRegenerate,
}: DiffPreviewDialogProps) {
  // 默认全选：AI 的建议通常是要用的，但用户可以取消任意一条。
  // 用 Set 而不是数组，勾选判断是 O(1)。
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(preview.items.map((_, i) => i)),
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showRaw, setShowRaw] = useState(false)

  const counts = useMemo(() => ai.countIssues(preview.issues), [preview.issues])
  const applyCheck = useMemo(() => ai.canApply(preview, selected.size), [preview, selected])

  /** 按条目下标取它相关的校验问题 */
  const issuesFor = (index: number) =>
    preview.issues.filter((i) => i.index === index + 1)

  const toggle = (i: number) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(i)) next.delete(i)
      else next.add(i)
      return next
    })
  }

  const toggleAll = () => {
    if (selected.size === preview.items.length) setSelected(new Set())
    else setSelected(new Set(preview.items.map((_, i) => i)))
  }

  const apply = async () => {
    setBusy(true)
    setError(null)
    try {
      // 全部选中时传 null，让后端走"全部接受"路径（语义更明确）
      const indices =
        selected.size === preview.items.length ? null : Array.from(selected).sort((a, b) => a - b)
      const r = await ai.aiApply(preview.previewId, indices)
      onApplied(r.created, r.updated)
      onClose()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const discard = async () => {
    try {
      await ai.aiDiscard(preview.previewId)
    } catch {
      // 放弃失败不影响关闭：预览本来就会过期
    }
    onClose()
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="diff-title">
      <div className="modal modal--wide">
        <h2 className="modal__title" id="diff-title">
          {preview.capability}　<span className="chip chip--muted">{preview.summary}</span>
        </h2>

        <p className="modal__text">
          以下是 AI 提出的改动。<strong>在你确认之前，数据库没有任何改动。</strong>
          请逐条核对——你可以只接受其中一部分。
        </p>

        {/* ---------------- 校验问题 ---------------- */}
        {preview.issues.length > 0 && (
          <div
            className={`alert ${counts.errors > 0 ? 'alert--error' : 'alert--warn'}`}
            role="alert"
          >
            <div>
              <strong>
                {counts.errors > 0
                  ? `有 ${counts.errors} 处问题导致无法写入`
                  : `有 ${counts.warnings} 处提示`}
              </strong>
              <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
                {preview.issues.slice(0, 8).map((iss, i) => (
                  <li key={i}>
                    {iss.index > 0 && <span>第 {iss.index} 条：</span>}
                    {iss.message}
                  </li>
                ))}
                {preview.issues.length > 8 && (
                  <li>…另有 {preview.issues.length - 8} 处，可展开原始输出查看</li>
                )}
              </ul>
            </div>
          </div>
        )}

        {error && (
          <div className="alert alert--error" role="alert">
            <span className="selectable">{error}</span>
          </div>
        )}

        {/* ---------------- 逐条差异 ---------------- */}
        {preview.items.length === 0 ? (
          <div className="alert alert--warn" role="note">
            <span>
              AI 没有提出任何改动。可以点「重新生成」再试一次，
              或在设置中换用能力更强的模型。
            </span>
          </div>
        ) : (
          <>
            <div className="diffhead">
              <label className="checkbox" style={{ margin: 0 }}>
                <input
                  type="checkbox"
                  checked={selected.size === preview.items.length}
                  ref={(el) => {
                    if (el) {
                      el.indeterminate =
                        selected.size > 0 && selected.size < preview.items.length
                    }
                  }}
                  onChange={toggleAll}
                />
                全选（已选 {selected.size} / {preview.items.length}）
              </label>
            </div>

            <ul className="difflist">
              {preview.items.map((it, i) => {
                const itemIssues = issuesFor(i)
                const hasError = itemIssues.some((x) => x.level === 'error')
                const on = selected.has(i)
                return (
                  <li
                    key={i}
                    className={`diffitem${on ? ' diffitem--on' : ''}${
                      hasError ? ' diffitem--error' : ''
                    }`}
                  >
                    <label className="diffitem__check">
                      <input
                        type="checkbox"
                        checked={on}
                        disabled={hasError}
                        onChange={() => toggle(i)}
                        aria-label={`接受第 ${i + 1} 条：${it.title}`}
                      />
                    </label>

                    <div className="diffitem__body">
                      <div className="diffitem__head">
                        <span
                          className={`chip ${
                            it.action === 'create'
                              ? 'chip--ok'
                              : it.action === 'reschedule'
                                ? 'chip--warn'
                                : 'chip--muted'
                          }`}
                        >
                          {ai.ACTION_LABELS[it.action]}
                        </span>
                        <span className="diffitem__title">{it.title}</span>
                      </div>

                      {/* 字段级改动：逐项列出前→后，让用户看清 AI 到底改了什么 */}
                      {/* 注意用 dl/dt/dd 而不是 table：这里是「字段名 → 值」的键值对，
                          不是表格数据，用定义列表语义更准确 */}
                      {it.changes.length > 0 && (
                        <dl className="difftable">
                          {it.changes.map((c, ci) => (
                            <div className="difftable__row" key={ci}>
                              <dt>{c.label}</dt>
                              <dd>
                                {c.before != null && c.before !== '' ? (
                                  <>
                                    <span className="diffold">{c.before}</span>
                                    <span className="diffarrow">→</span>
                                  </>
                                ) : null}
                                <span className="diffnew">{c.after ?? '（清空）'}</span>
                              </dd>
                            </div>
                          ))}
                        </dl>
                      )}

                      {it.note && <div className="diffitem__note">AI 说明：{it.note}</div>}

                      {itemIssues.length > 0 && (
                        <div className="diffitem__issues">
                          {itemIssues.map((x, xi) => (
                            <div key={xi} className={x.level === 'error' ? 'formerr' : 'setgroup__hint'}>
                              {x.level === 'error' ? '无法写入：' : '提示：'}
                              {x.message}
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </li>
                )
              })}
            </ul>
          </>
        )}

        {/* ---------------- 数据范围与消耗（§6 要求） ---------------- */}
        <div className="diffmeta">
          <div className="diffmeta__row">
            <strong>发送范围：</strong>
            {preview.dataScopeNote}
          </div>
          {preview.usage && (
            <div className="diffmeta__row">
              <strong>本次消耗：</strong>
              输入 {preview.usage.inputTokens ?? '?'} tokens、输出{' '}
              {preview.usage.outputTokens ?? '?'} tokens
              {preview.usage.cacheHitTokens != null && (
                <>（缓存命中 {preview.usage.cacheHitTokens}）</>
              )}
              。费用由你自己的服务商账户产生。
            </div>
          )}
          <button
            type="button"
            className="btn btn--quiet btn--sm"
            aria-expanded={showRaw}
            onClick={() => setShowRaw((v) => !v)}
          >
            {showRaw ? '隐藏原始输出' : '查看模型原始输出'}
          </button>
          {showRaw && <pre className="diffraw selectable">{preview.raw}</pre>}
        </div>

        {/* ---------------- 操作 ---------------- */}
        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={() => void discard()} disabled={busy}>
            放弃
          </button>
          {onRegenerate && (
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => {
                void ai.aiDiscard(preview.previewId).catch(() => {})
                onRegenerate()
                onClose()
              }}
              disabled={busy}
              title="丢弃这份结果并重新请求一次"
            >
              重新生成
            </button>
          )}
          <button
            type="button"
            className="btn btn--primary"
            disabled={busy || !applyCheck.ok}
            title={applyCheck.reason}
            onClick={() => void apply()}
          >
            {busy
              ? '写入中…'
              : applyCheck.ok
                ? `写入 ${selected.size} 条改动`
                : (applyCheck.reason ?? '无法写入')}
          </button>
        </div>
      </div>
    </div>
  )
}
