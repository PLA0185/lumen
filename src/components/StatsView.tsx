/**
 * 统计与成长页（任务书 §7）。
 *
 * ## 一条必须遵守的界面要求
 *
 * §7 要求「清楚标注分母、时间范围和重复实例的计数口径」。
 * 因此本页顶部**常驻**一段口径说明（来自后端 `scope` 字段），
 * 而不是藏在某个提示里——否则"完成率 60%"可以被任何口径解释。
 *
 * ## 另一个必须避免的误导
 *
 * §7 点名「连续完成天数不能因为没有安排任务的一天产生误导」。
 * 因此连续天数卡片下方直接写出规则：完全没有安排的日子不中断、
 * 也不计入；而"有安排但没完成"会中断。
 *
 * ## 百分比为 null 时显示"无数据"
 *
 * 后端在分母为 0 时返回 null 而不是 0%。界面必须如实呈现这个区别：
 * "这周没有安排任务"和"安排了但一项没完成"是两件不同的事。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import * as st from '../lib/stats-ipc'
// 排程冲突检测不依赖 AI（纯规则计算），但它定义在 ai-ipc 中，
// 因为它属于"排程辅助"这一组能力。
import { scheduleConflicts } from '../lib/ai-ipc'
import { IpcError } from '../lib/ipc'
import { onDataChanged } from '../lib/data-change'
import { createRequestGate } from '../lib/request-gate'
import { useApp } from '../lib/store'
import * as focus from '../lib/focus-ipc'
import type { GrowthConfig, GrowthOverview, PeriodStats } from '../lib/stats-ipc'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

/**
 * 专注记录区块（§4.4）。
 *
 * 与统计页放在一起而不是单独一页：专注是"投入"的度量，
 * 与完成数量、耗时并列看才有意义（§7 要求统计基于同一份真实数据）。
 */
function FocusSection() {
  const [summary, setSummary] = useState<focus.FocusSummary | null>(null)

  useEffect(() => {
    let generation = 0
    const reload = () => {
      const token = ++generation
      void focus.focusSummary().then((next) => {
        if (token === generation) setSummary(next)
      }).catch(() => {
        // 专注数据是附加信息，失败不打断统计页
      })
    }
    reload()
    const off = onDataChanged(['focus', 'tasks', 'all'], reload)
    return () => { generation++; off() }
  }, [])

  if (!summary) return null

  return (
    <div className="setgroup">
      <h3 className="setgroup__title">专注记录</h3>
      <div className="statcards">
        <div className="statcard">
          <div className="statcard__label">今日专注</div>
          <div className="statcard__value">{focus.formatHms(summary.todaySeconds)}</div>
          <div className="statcard__sub">共 {summary.todaySessions} 轮（只计已完成的）</div>
        </div>
        <div className="statcard">
          <div className="statcard__label">本周专注</div>
          <div className="statcard__value">{focus.formatHms(summary.weekSeconds)}</div>
          <div className="statcard__sub">本周从周一算起</div>
        </div>
      </div>
      <p className="setgroup__hint">
        只统计「完成并记录」的专注轮次。被放弃或中断的时长不计入，
        以免把离开电脑的时间算成有效投入。
        {summary.activeSession && (
          <>
            <br />
            当前有一轮专注正在进行中（
            {focus.FOCUS_STATE_LABELS[summary.activeSession.state]}）。
          </>
        )}
      </p>
    </div>
  )
}

/** 可选的统计区间 */
const RANGES: Array<{ days: number; label: string }> = [
  { days: 7, label: '最近 7 天' },
  { days: 30, label: '最近 30 天' },
  { days: 90, label: '最近 90 天' },
  { days: 365, label: '最近一年' },
]

export function StatsView() {
  const gate = useMemo(createRequestGate, [])
  const pushToast = useApp((s) => s.pushToast)
  const [days, setDays] = useState(30)
  const [stats, setStats] = useState<PeriodStats | null>(null)
  const [growth, setGrowth] = useState<GrowthOverview | null>(null)
  const [growthCfg, setGrowthCfg] = useState<GrowthConfig | null>(null)
  const [goals, setGoals] = useState<st.GoalProgress[]>([])
  const [conflicts, setConflicts] = useState<Array<{ day: string; kind: string; message: string }>>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // 新建目标
  const [newGoal, setNewGoal] = useState('')
  const [newTarget, setNewTarget] = useState(10)

  const reload = useCallback(async () => {
    const token = gate.begin()
    setLoading(true)
    try {
      const [p, g, cfg, gs, cf] = await Promise.all([
        st.statsPeriod(days),
        st.statsGrowth(),
        st.growthGetConfig(),
        st.goalsList(),
        // 冲突检测失败不应让整页报错——它是附加信息
        scheduleConflicts().catch(() => []),
      ])
      if (!gate.isCurrent(token)) return
      setStats(p)
      setGrowth(g)
      setGrowthCfg(cfg)
      setGoals(gs)
      setConflicts(cf)
      setError(null)
    } catch (e) {
      if (gate.isCurrent(token)) setError(errText(e))
    } finally {
      if (gate.isCurrent(token)) setLoading(false)
    }
  }, [days, gate])

  useEffect(() => {
    void reload()
    const off = onDataChanged(['tasks', 'focus', 'stats', 'all'], () => void reload())
    return () => {
      off()
      gate.invalidate()
    }
  }, [gate, reload])

  useEffect(() => () => gate.dispose(), [gate])

  /** 趋势图的最大值，用于计算柱高 */
  const maxDaily = useMemo(() => {
    if (!stats) return 1
    return Math.max(
      1,
      ...stats.daily.map((d) => Math.max(d.planned, d.completed)),
    )
  }, [stats])

  const saveGrowth = async (patch: Partial<GrowthConfig>) => {
    if (!growthCfg) return
    const next = { ...growthCfg, ...patch }
    setGrowthCfg(next)
    try {
      await st.growthSetConfig(next)
      setGrowth(await st.statsGrowth())
    } catch (e) {
      setError(errText(e))
    }
  }

  const createGoal = async () => {
    const t = newGoal.trim()
    if (!t) return
    try {
      await st.goalCreate(t, newTarget)
      setNewGoal('')
      setGoals(await st.goalsList())
      pushToast('success', `已创建目标「${t}」`)
    } catch (e) {
      setError(errText(e))
    }
  }

  if (loading && !stats) {
    return (
      <div className="stats">
        <div className="skeleton" style={{ height: 200 }} />
      </div>
    )
  }

  if (!stats) {
    return (
      <div className="state">
        <div className="state__inner">
          <div className="state__icon" aria-hidden="true">
            <Icon name="alert" size={30} strokeWidth={1.5} />
          </div>
          <div className="state__title">统计加载失败</div>
          <div className="state__text selectable">{error ?? '未知错误'}</div>
          <div className="state__actions">
            <button type="button" className="btn btn--primary" onClick={() => void reload()}>
              重试
            </button>
          </div>
        </div>
      </div>
    )
  }

  const est = st.describeEstimateRatio(stats.estimateRatio)

  return (
    <div className="stats">
      {/* ---------------- 区间选择与口径说明 ---------------- */}
      <div className="stats__bar">
        <div className="segmented" role="radiogroup" aria-label="统计区间">
          {RANGES.map((r) => (
            <button
              key={r.days}
              type="button"
              role="radio"
              aria-checked={days === r.days}
              className={`segmented__item${days === r.days ? ' segmented__item--on' : ''}`}
              onClick={() => setDays(r.days)}
            >
              {r.label}
            </button>
          ))}
        </div>
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => void reload()}>
          刷新
        </button>
      </div>

      {/* 口径说明必须常驻可见，而不是藏在提示里 */}
      <p className="stats__scope">
        <strong>口径说明：</strong>
        {stats.scope}
        <br />
        当前区间：{stats.startDate} 至 {stats.endDate}（共 {stats.days} 天）
      </p>

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}

      {/* ---------------- 冲突提示（不依赖 AI） ---------------- */}
      {conflicts.length > 0 && (
        <div className="alert alert--warn" role="note">
          <div>
            <strong>排程提示：</strong>
            <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
              {conflicts.slice(0, 5).map((c, i) => (
                <li key={`${c.day}-${i}`}>{c.message}</li>
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* ---------------- 核心指标 ---------------- */}
      <div className="statcards">
        <div className="statcard">
          <div className="statcard__label">完成数量</div>
          <div className="statcard__value">{stats.completedTotal}</div>
          <div className="statcard__sub">
            区间内计划 {stats.plannedTotal} 项
            {stats.dueTotal > 0 && <>　截止 {stats.dueTotal} 项</>}
          </div>
        </div>

        <div className="statcard">
          <div className="statcard__label">完成率</div>
          <div className="statcard__value">
            {st.formatPercent(stats.completionRate)}
          </div>
          <div className="statcard__sub">
            {stats.completionRate == null
              ? '本区间没有安排任务，因此没有可计算的分母'
              : `分母＝区间内计划或截止的任务数（${stats.plannedTotal}）`}
          </div>
        </div>

        <div className="statcard">
          <div className="statcard__label">逾期</div>
          <div className="statcard__value">
            {stats.overdueTotal}
            <span className="statcard__unit">
              　{st.formatPercent(stats.overdueRate)}
            </span>
          </div>
          <div className="statcard__sub">
            {stats.dueTotal > 0
              ? `分母＝区间内到期的任务数（${stats.dueTotal}）`
              : '本区间没有到期的任务'}
          </div>
        </div>

        <div className="statcard">
          <div className="statcard__label">投入时间</div>
          <div className="statcard__value">{st.formatMinutes(stats.actualMinutes)}</div>
          <div className="statcard__sub">
            {stats.avgMinutesPerTask != null
              ? `平均每项 ${st.formatMinutes(stats.avgMinutesPerTask)}`
              : '暂无已完成任务的耗时记录'}
          </div>
        </div>
      </div>

      {/* ---------------- 耗时估算 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">耗时估算</h3>
        <table className="kvtable">
          <tbody>
            <tr>
              <th>预计合计</th>
              <td>{st.formatMinutes(stats.estimatedMinutes)}</td>
            </tr>
            <tr>
              <th>实际合计</th>
              <td>{st.formatMinutes(stats.actualMinutes)}</td>
            </tr>
            <tr>
              <th>估算准确度</th>
              <td>
                {est.text}
                <div className="setgroup__hint">{est.hint}</div>
              </td>
            </tr>
            <tr>
              <th>期间新建</th>
              <td>
                {stats.createdTotal} 项
                <div className="setgroup__hint">
                  与「区间内计划」不同：这里统计的是创建时间落在区间内的任务
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      {/* ---------------- 每日趋势（纯 CSS 柱状图，避免图表库绑定） ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">每日趋势</h3>
        <p className="setgroup__desc">
          浅色柱是当天计划数，深色柱是当天完成数。没有安排的日子柱高为 0。
        </p>
        <div className="chart">
          {stats.daily.map((d) => (
            <div className="chart__col" key={d.date} title={`${d.date}：计划 ${d.planned}，完成 ${d.completed}，逾期 ${d.overdue}`}>
              <div className="chart__bars">
                <div
                  className="chart__bar chart__bar--planned"
                  style={{ height: `${(d.planned / maxDaily) * 100}%` }}
                />
                <div
                  className="chart__bar chart__bar--done"
                  style={{ height: `${(d.completed / maxDaily) * 100}%` }}
                />
              </div>
              <div className="chart__label">{st.shortDate(d.date)}</div>
            </div>
          ))}
        </div>
      </div>

      {/* ---------------- 分类与项目占比 ---------------- */}
      <div className="stats__cols">
        <div className="setgroup">
          <h3 className="setgroup__title">分类占比</h3>
          {stats.byCategory.length === 0 ? (
            <p className="setgroup__hint">本区间没有已完成的任务，或任务未设置分类。</p>
          ) : (
            <ul className="shares">
              {stats.byCategory.map((c) => {
                const total = stats.byCategory.reduce((a, b) => a + b.count, 0) || 1
                const pct = Math.round((c.count / total) * 100)
                return (
                  <li key={c.categoryId ?? 'none'}>
                    <span
                      className="orgrow__swatch"
                      style={{ background: c.color ?? 'var(--c-border-strong)' }}
                      aria-hidden="true"
                    />
                    <span className="shares__name">{c.name}</span>
                    <span className="shares__bar">
                      <span className="shares__fill" style={{ width: `${pct}%` }} />
                    </span>
                    <span className="shares__count">
                      {c.count}（{pct}%）
                    </span>
                  </li>
                )
              })}
            </ul>
          )}
          <p className="setgroup__hint">分母为区间内已完成且设置了分类的任务总数。</p>
        </div>

        <div className="setgroup">
          <h3 className="setgroup__title">项目分布</h3>
          {stats.byProject.length === 0 ? (
            <p className="setgroup__hint">本区间没有已完成的任务，或任务未归属项目。</p>
          ) : (
            <ul className="shares">
              {stats.byProject.map((p) => {
                const total = stats.byProject.reduce((a, b) => a + b.count, 0) || 1
                const pct = Math.round((p.count / total) * 100)
                return (
                  <li key={p.projectId ?? 'none'}>
                    <span
                      className="orgrow__swatch"
                      style={{ background: p.color ?? 'var(--c-border-strong)' }}
                      aria-hidden="true"
                    />
                    <span className="shares__name">{p.name}</span>
                    <span className="shares__bar">
                      <span className="shares__fill" style={{ width: `${pct}%` }} />
                    </span>
                    <span className="shares__count">
                      {p.count}（{pct}%）
                    </span>
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      </div>

      {/* ---------------- 连续天数 ---------------- */}
      {growthCfg?.showStreak && (
        <div className="setgroup">
          <h3 className="setgroup__title">连续完成</h3>
          <div className="statcards">
            <div className="statcard">
              <div className="statcard__label">当前连续</div>
              <div className="statcard__value">
                {stats.streak.activeStreak}
                <span className="statcard__unit"> 天</span>
              </div>
            </div>
            <div className="statcard">
              <div className="statcard__label">历史最长</div>
              <div className="statcard__value">
                {stats.streak.longestActiveStreak}
                <span className="statcard__unit"> 天</span>
              </div>
            </div>
            <div className="statcard">
              <div className="statcard__label">完美连续</div>
              <div className="statcard__value">
                {stats.streak.perfectStreak}
                <span className="statcard__unit"> 天</span>
              </div>
              <div className="statcard__sub">当天有安排且全部完成才算</div>
            </div>
          </div>
          <p className="setgroup__hint">{stats.streak.note}</p>
        </div>
      )}

      {/* ---------------- 个人目标 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">个人目标</h3>
        <p className="setgroup__desc">
          进度按「目标创建之后完成的任务数」计算——这样新建一个目标时不会
          因为历史完成量而立刻显示达成。
        </p>

        <div className="organize__new">
          <input
            className="input selectable"
            value={newGoal}
            placeholder="目标名称，例如：本月完成 30 项"
            aria-label="目标名称"
            onChange={(e) => setNewGoal(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void createGoal()
            }}
          />
          <label className="field">
            目标数量
            <input
              type="number"
              className="input input--compact"
              min={1}
              max={100000}
              value={newTarget}
              aria-label="目标数量"
              onChange={(e) => setNewTarget(Math.max(1, Number(e.target.value) || 1))}
            />
          </label>
          <button
            type="button"
            className="btn btn--primary"
            disabled={!newGoal.trim()}
            onClick={() => void createGoal()}
          >
            创建目标
          </button>
        </div>

        {goals.length === 0 ? (
          <p className="setgroup__hint">还没有目标。设定一个可量化的目标能帮你保持节奏。</p>
        ) : (
          <ul className="goallist">
            {goals.map((g) => (
              <li key={g.id} className="goalrow">
                <span className="goalrow__title">
                  {g.title}
                  {g.achieved && (
                    <span className="chip chip--ok" title="已达成">
                      已达成
                    </span>
                  )}
                </span>
                <span className="shares__bar">
                  <span className="shares__fill" style={{ width: `${g.percent}%` }} />
                </span>
                <span className="goalrow__meta">
                  {g.doneCount}/{g.targetCount}（{g.percent}%）
                  {g.dueDate && <>　截止 {g.dueDate}</>}
                </span>
                <button
                  type="button"
                  className="icon-btn icon-btn--danger"
                  aria-label={`删除目标 ${g.title}`}
                  onClick={async () => {
                    if (!window.confirm(`删除目标「${g.title}」？\n\n这不会影响任何任务。`)) return
                    try {
                      await st.goalDelete(g.id)
                      setGoals(await st.goalsList())
                    } catch (e) {
                      setError(errText(e))
                    }
                  }}
                >
                  <Icon name="close" size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* ---------------- 专注记录（§4.4） ---------------- */}
      <FocusSection />

      {/* ---------------- 成长与游戏化 ---------------- */}
      <div className="setgroup">
        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">启用经验值与成就</div>
            <div className="setrow__desc">
              §7 的可选反馈功能，<strong>默认关闭</strong>。经验值按完成项数计算，
              与耗时无关——按耗时给经验会诱导虚报工时，让统计失真。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '启用'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${growthCfg?.gamificationEnabled === v ? ' segmented__item--on' : ''}`}
                aria-pressed={growthCfg?.gamificationEnabled === v}
                onClick={() => void saveGrowth({ gamificationEnabled: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">显示连续天数</div>
            <div className="setrow__desc">关闭后统计页不显示连续完成卡片。</div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '显示'],
                [false, '隐藏'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${growthCfg?.showStreak === v ? ' segmented__item--on' : ''}`}
                aria-pressed={growthCfg?.showStreak === v}
                onClick={() => void saveGrowth({ showStreak: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {growthCfg && (
          <div className="formgrid">
            <label className="formrow">
              <span className="formlabel">每日目标（项）</span>
              <input
                type="number"
                className="input"
                min={1}
                max={100}
                value={growthCfg.dailyGoal}
                onChange={(e) =>
                  void saveGrowth({ dailyGoal: Math.max(1, Math.min(100, Number(e.target.value) || 1)) })
                }
              />
            </label>
            <label className="formrow">
              <span className="formlabel">每周目标（项）</span>
              <input
                type="number"
                className="input"
                min={1}
                max={500}
                value={growthCfg.weeklyGoal}
                onChange={(e) =>
                  void saveGrowth({ weeklyGoal: Math.max(1, Math.min(500, Number(e.target.value) || 1)) })
                }
              />
            </label>
          </div>
        )}

        {growth?.enabled && (
          <>
            <div className="statcards" style={{ marginTop: 12 }}>
              <div className="statcard">
                <div className="statcard__label">等级</div>
                <div className="statcard__value">
                  Lv.{growth.level.level}
                  <span className="statcard__unit">　{growth.level.title}</span>
                </div>
                <div className="statcard__sub">
                  {growth.level.xpInLevel}/{growth.level.xpForNext} 经验
                </div>
                <span className="shares__bar" style={{ marginTop: 6 }}>
                  <span className="shares__fill" style={{ width: `${growth.level.progress}%` }} />
                </span>
              </div>
              <div className="statcard">
                <div className="statcard__label">今日</div>
                <div className="statcard__value">
                  {growth.todayDone}
                  <span className="statcard__unit"> / {growth.dailyGoal}</span>
                </div>
                <div className="statcard__sub">
                  {growth.todayDone >= growth.dailyGoal ? '已达成今日目标' : '继续加油'}
                </div>
              </div>
              <div className="statcard">
                <div className="statcard__label">本周</div>
                <div className="statcard__value">
                  {growth.weekDone}
                  <span className="statcard__unit"> / {growth.weeklyGoal}</span>
                </div>
                <div className="statcard__sub">
                  {growth.weekDone >= growth.weeklyGoal ? '已达成本周目标' : '本周从周一算起'}
                </div>
              </div>
            </div>

            <h4 className="setgroup__title" style={{ marginTop: 14 }}>
              成就
            </h4>
            <ul className="achlist">
              {growth.achievements.map((a) => (
                <li key={a.id} className={`achrow${a.achieved ? ' achrow--done' : ''}`}>
                  <span className="achrow__icon" aria-hidden="true">
                    <Icon name={a.achieved ? 'star' : 'completed'} size={17} />
                  </span>
                  <span className="achrow__body">
                    <span className="achrow__name">{a.name}</span>
                    <span className="achrow__desc">{a.description}</span>
                  </span>
                  <span className="achrow__progress">
                    {a.progress}/{a.target}
                  </span>
                </li>
              ))}
            </ul>
            <p className="setgroup__hint">
              所有成就都基于真实完成记录计算，没有"点一下就能拿到"的空成就。
            </p>
          </>
        )}
      </div>
    </div>
  )
}
