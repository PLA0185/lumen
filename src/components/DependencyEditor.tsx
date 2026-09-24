/**
 * 任务依赖编辑器（任务书 §4.1「支持任务依赖及循环依赖校验」）。
 *
 * 交互要点：
 * - 同时展示两个方向：**前置**（我必须先完成谁）与**后继**（谁在等我）。
 *   只看一个方向会让用户无法判断"这件事卡住了谁"，而后者往往决定优先级。
 * - 前置任务未完成时明确标注"被阻塞"，用颜色与文案双重表达（§3 不依赖颜色单独表达）。
 * - 循环依赖由后端拒绝，这里把错误原文展示给用户，并说明原因。
 * - 选择前置任务时排除自身与已存在的依赖，减少用户犯错的可能。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import * as org from '../lib/organize-ipc'
import * as ipc from '../lib/ipc'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate } from '../lib/request-gate'
import type { DependencyItem } from '../lib/organize-ipc'
import type { Task } from '../lib/types'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

interface DependencyEditorProps {
  taskId: string
  taskTitle: string
}

export function DependencyEditor({ taskId, taskTitle }: DependencyEditorProps) {
  const candidateGate = useMemo(createRequestGate, [])
  const [deps, setDeps] = useState<DependencyItem[]>([])
  const [dependents, setDependents] = useState<DependencyItem[]>([])
  const [candidates, setCandidates] = useState<Task[]>([])
  const [candidateTotal, setCandidateTotal] = useState(0)
  const [candidateLoading, setCandidateLoading] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [picking, setPicking] = useState(false)
  const [search, setSearch] = useState('')
  const [busy, setBusy] = useState(false)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [d, rev] = await Promise.all([
        org.dependencyList(taskId),
        org.dependencyDependents(taskId),
      ])
      setDeps(d)
      setDependents(rev)
      setError(null)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
    }
  }, [taskId])

  useEffect(() => {
    void reload()
    return onDataChanged(['dependencies', 'tasks', 'all'], () => void reload())
  }, [reload])

  const loadCandidates = useCallback(async (offset: number) => {
    const token = candidateGate.begin()
    const query = {
      statuses: ['todo', 'doing', 'waiting'] as Array<'todo' | 'doing' | 'waiting'>,
      search: search.trim() || null,
    }
    setCandidateLoading(true)
    try {
      const [list, count] = await Promise.all([
        ipc.listTasks({ ...query, limit: 50, offset, sortBy: 'due' }),
        ipc.countTasks(query),
      ])
      if (!candidateGate.isCurrent(token)) return
      setCandidates((old) => offset === 0 ? list : [...old, ...list])
      setCandidateTotal(count.total)
      setError(null)
    } catch (e) {
      if (candidateGate.isCurrent(token)) setError(errText(e))
    } finally {
      if (candidateGate.isCurrent(token)) setCandidateLoading(false)
    }
  }, [candidateGate, search])

  useEffect(() => {
    if (!picking) return
    const timer = window.setTimeout(() => void loadCandidates(0), 250)
    return () => { window.clearTimeout(timer); candidateGate.invalidate() }
  }, [candidateGate, loadCandidates, picking])

  useEffect(() => () => candidateGate.dispose(), [candidateGate])

  /** 候选：排除自身、已存在的前置、以及已完成任务（已完成的不构成阻塞） */
  const filtered = useMemo(() => {
    const existing = new Set(deps.map((d) => d.dependsOnId))
    return candidates
      .filter((t) => t.id !== taskId)
      .filter((t) => !existing.has(t.id))
      .filter((t) => t.status !== 'done' && t.status !== 'archived')
  }, [candidates, deps, taskId])

  const add = async (dependsOnId: string) => {
    setBusy(true)
    setError(null)
    try {
      await org.dependencyAdd(taskId, dependsOnId)
      setPicking(false)
      setSearch('')
      await reload()
    } catch (e) {
      // 循环依赖等业务错误原文展示，用户能看懂"为什么被拒绝"
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (dependsOnId: string) => {
    try {
      await org.dependencyRemove(taskId, dependsOnId)
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  const blocked = deps.some((d) => !d.isDone)

  return (
    <div className="deps">
      <div className="subtasks__head">
        <span>依赖</span>
        {blocked && (
          <span className="chip chip--warn" title="存在未完成的前置任务">
            <Icon name="alert" size={13} /> 被阻塞
          </span>
        )}
      </div>

      {loading ? (
        <div className="skeleton" style={{ height: 26 }} />
      ) : (
        <>
          {/* 前置：我必须先完成谁 */}
          {deps.length > 0 && (
            <div className="deps__group">
              <div className="deps__label">需先完成：</div>
              {deps.map((d) => (
                <div
                  key={d.dependsOnId}
                  className={`dependency${d.isDone ? '' : ' dependency--blocking'}`}
                >
                  <span aria-hidden="true"><Icon name={d.isDone ? 'completed' : 'clock'} size={14} /></span>
                  <span className="dependency__title">{d.title}</span>
                  <span className="dependency__state">
                    {d.isDone ? '已完成' : '未完成（阻塞中）'}
                  </span>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    aria-label={`解除对「${d.title}」的依赖`}
                    title="解除此依赖"
                    onClick={() => void remove(d.dependsOnId)}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* 后继：谁在等我 */}
          {dependents.length > 0 && (
            <div className="deps__group">
              <div className="deps__label">
                正在等待「{taskTitle}」的任务（完成本任务可解除它们的阻塞）：
              </div>
              {dependents.map((d) => (
                <div key={d.dependsOnId} className="dependency dependency--info">
                  <span aria-hidden="true">→</span>
                  <span className="dependency__title">{d.title}</span>
                  <span className="dependency__state">{d.isDone ? '已完成' : '等待中'}</span>
                </div>
              ))}
            </div>
          )}

          {deps.length === 0 && dependents.length === 0 && (
            <p className="reminders__empty">
              没有依赖关系。可以指定「必须先完成哪些任务」，用于表达真实的先后顺序。
            </p>
          )}
        </>
      )}

      {!picking ? (
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => setPicking(true)}>
          <Icon name="plus" size={15} /> 添加前置任务
        </button>
      ) : (
        <div className="depspicker">
          <input
            className="input input--compact selectable"
            value={search}
            autoFocus
            placeholder="搜索任务标题…"
            aria-label="搜索要作为前置的任务"
            onChange={(e) => setSearch(e.target.value)}
          />
          <ul className="depspicker__list">
            {filtered.length === 0 ? (
              <li className="depspicker__empty">
                {search.trim()
                  ? '没有匹配的未完成任务'
                  : '没有可添加的任务（已完成的、已作为前置的、以及本任务自身都不会出现在这里）'}
              </li>
            ) : (
              filtered.map((t) => (
                <li key={t.id}>
                  <button
                    type="button"
                    className="depspicker__item"
                    disabled={busy}
                    onClick={() => void add(t.id)}
                  >
                    {t.title}
                  </button>
                </li>
              ))
            )}
          </ul>
          {candidateLoading && <p className="setgroup__hint">正在搜索…</p>}
          <p className="setgroup__hint">已读取 {candidates.length} / {candidateTotal} 条匹配任务</p>
          {candidates.length < candidateTotal && (
            <button type="button" className="btn btn--quiet btn--sm"
              disabled={candidateLoading} onClick={() => void loadCandidates(candidates.length)}>
              加载更多
            </button>
          )}
          <button
            type="button"
            className="btn btn--quiet btn--sm"
            onClick={() => {
              setPicking(false)
              setSearch('')
            }}
          >
            取消
          </button>
        </div>
      )}

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
    </div>
  )
}
