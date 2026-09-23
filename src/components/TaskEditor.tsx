/**
 * 任务编辑对话框（任务书 §4.1 完整字段集）。
 *
 * ## 关键设计：三个时间字段必须分得清
 *
 * §4.1 明确要求「计划时间、截止时间、提醒时间是不同字段」，
 * 且「表单、任务卡片、筛选器、AI 排程和统计要使用同一套含义」。
 * 因此本表单把它们分区呈现，并各自带一句说明：
 *
 * - **计划时间**：决定任务出现在「今天」和日历的哪一天
 * - **截止时间**：只用于逾期判断，可以为空
 * - **提醒**：在任务卡片展开区单独设置（不混进这个表单）
 *
 * 每个时间都可独立选择「仅日期」或「精确到时刻」——用时间输入框是否填值
 * 来表达，而不是让用户在两个模式间切换（§4.3 禁止把全天任务当作凌晨到期）。
 *
 * ## Markdown 备注
 *
 * 用 `react-markdown` + `rehype-sanitize` 渲染预览。默认白名单即 GitHub
 * 的渲染白名单，非白名单内容一律丢弃，因此不会执行注入的脚本（§10）。
 */

import { useEffect, useMemo, useState } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize from 'rehype-sanitize'
import * as ipc from '../lib/ipc'
import * as org from '../lib/organize-ipc'
import { IpcError } from '../lib/ipc'
import { combineDateTime, fromUtcIso, toDateInput, toTimeInput } from '../lib/datetime'
import type { PeriodType, Task, TaskStatus } from '../lib/types'
import { PERIOD_LABELS } from '../lib/types'
import type { Category, ProjectWithCount, TagWithCount } from '../lib/organize-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

/** 状态选项（§4.1 至少五种） */
const STATUS_OPTIONS: { value: TaskStatus; label: string }[] = [
  { value: 'todo', label: '待办' },
  { value: 'doing', label: '进行中' },
  { value: 'waiting', label: '等待' },
  { value: 'done', label: '已完成' },
  { value: 'archived', label: '已归档' },
]

interface TaskEditorProps {
  task: Task
  onClose: () => void
  onSaved: (t: Task) => void
}

export function TaskEditor({ task, onClose, onSaved }: TaskEditorProps) {
  // 基本信息
  const [title, setTitle] = useState(task.title)
  const [description, setDescription] = useState(task.description)
  const [noteMd, setNoteMd] = useState(task.noteMd)
  const [linkUrl, setLinkUrl] = useState(task.linkUrl ?? '')
  const [status, setStatus] = useState<TaskStatus>(task.status)
  const [priority, setPriority] = useState(task.priority)
  const [estimated, setEstimated] = useState(task.estimatedMinutes?.toString() ?? '')
  const [actual, setActual] = useState(task.actualMinutes.toString())

  // 归属
  const [projectId, setProjectId] = useState(task.projectId ?? '')
  const [categoryId, setCategoryId] = useState(task.categoryId ?? '')
  const [tagIds, setTagIds] = useState<string[]>([])

  // 三个时间字段（各自独立）
  const [plannedDate, setPlannedDate] = useState('')
  const [plannedTime, setPlannedTime] = useState('')
  const [dueDate, setDueDate] = useState('')
  const [dueTime, setDueTime] = useState('')

  // 标记
  const [isPinned, setIsPinned] = useState(task.isPinned === 1)
  const [isFavorite, setIsFavorite] = useState(task.isFavorite === 1)
  /** 周期跨度：表达"这周/这个月做完就行"，不绑定具体日期 */
  const [periodType, setPeriodType] = useState<PeriodType>(task.periodType ?? 'none')

  // 选项数据
  const [projects, setProjects] = useState<ProjectWithCount[]>([])
  const [categories, setCategories] = useState<Category[]>([])
  const [tags, setTags] = useState<TagWithCount[]>([])

  const [showPreview, setShowPreview] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  // 载入选项与初始值
  useEffect(() => {
    void (async () => {
      try {
        const [p, c, t, mine] = await Promise.all([
          org.projectList(true),
          org.categoryList(),
          org.tagList(),
          org.taskTagsGet(task.id),
        ])
        setProjects(p)
        setCategories(c)
        setTags(t)
        setTagIds(mine.map((x) => x.id))
      } catch (e) {
        setError(errText(e))
      }
    })()
  }, [task.id])

  // 时间字段初始化：把 UTC 转成本地日期/时间控件值
  useEffect(() => {
    const p = fromUtcIso(task.plannedAt)
    setPlannedDate(toDateInput(p))
    setPlannedTime(task.hasPlannedTime === 1 ? toTimeInput(p) : '')

    const d = fromUtcIso(task.dueAt)
    setDueDate(toDateInput(d))
    setDueTime(task.hasDueTime === 1 ? toTimeInput(d) : '')
  }, [task])

  /** Esc 关闭（§3 关键操作可用键盘完成） */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  /** 客户端校验，减少一次往返就能告诉用户哪里填错了 */
  const validate = (): boolean => {
    const errs: Record<string, string> = {}
    if (!title.trim()) errs.title = '标题不能为空'
    else if (title.trim().length > 500) errs.title = '标题不能超过 500 个字符'

    if (linkUrl.trim()) {
      try {
        const u = new URL(linkUrl.trim())
        // 只允许 http/https，避免把 javascript: 之类的伪协议交给系统打开
        if (u.protocol !== 'http:' && u.protocol !== 'https:') {
          errs.linkUrl = '链接必须以 http:// 或 https:// 开头'
        }
      } catch {
        errs.linkUrl = '链接格式不正确'
      }
    }

    const est = estimated.trim() ? Number(estimated) : null
    if (est !== null && (!Number.isFinite(est) || est < 0)) {
      errs.estimated = '预计耗时必须是非负数字'
    }
    const act = actual.trim() ? Number(actual) : 0
    if (!Number.isFinite(act) || act < 0) {
      errs.actual = '实际耗时必须是非负数字'
    }

    setFieldErrors(errs)
    return Object.keys(errs).length === 0
  }

  const save = async () => {
    if (!validate()) return
    setSaving(true)
    setError(null)
    try {
      const planned = plannedDate ? combineDateTime(plannedDate, plannedTime) : null
      const due = dueDate ? combineDateTime(dueDate, dueTime) : null

      // 注意 UpdateTaskInput 的语义：
      //   undefined = 不修改；clearXxx = 清空。
      // 因此用户删掉日期时必须走 clearXxx，而不是传 null。
      const patch: Parameters<typeof ipc.updateTask>[1] = {
        title: title.trim(),
        description,
        noteMd,
        status,
        priority,
        estimatedMinutes: estimated.trim() ? Number(estimated) : null,
        actualMinutes: actual.trim() ? Number(actual) : 0,
        isPinned,
        isFavorite,
        periodType,
      }

      if (planned) {
        patch.plannedAt = planned.utc
        patch.hasPlannedTime = planned.hasTime
      } else {
        patch.clearPlannedAt = true
      }

      if (due) {
        patch.dueAt = due.utc
        patch.hasDueTime = due.hasTime
      } else {
        patch.clearDueAt = true
      }

      if (linkUrl.trim()) patch.linkUrl = linkUrl.trim()
      else patch.clearLink = true

      if (projectId) patch.projectId = projectId
      else patch.clearProject = true

      if (categoryId) patch.categoryId = categoryId
      else patch.clearCategory = true

      const saved = await ipc.updateTask(task.id, patch)
      // 标签用整体替换语义，单独调用
      await org.taskTagsSet(task.id, tagIds)
      onSaved(saved)
      onClose()
    } catch (e) {
      setError(errText(e))
    } finally {
      setSaving(false)
    }
  }

  const preview = useMemo(
    () =>
      noteMd.trim() ? (
        <div className="mdpreview selectable">
          <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
            {noteMd}
          </Markdown>
        </div>
      ) : (
        <p className="setgroup__hint">还没有填写备注。</p>
      ),
    [noteMd],
  )

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="editor-title">
      <div className="modal modal--wide">
        <h2 className="modal__title" id="editor-title">
          编辑任务
        </h2>

        {error && (
          <div className="alert alert--error" role="alert">
            <span className="selectable">{error}</span>
          </div>
        )}

        {/* ---------------- 基本信息 ---------------- */}
        <div className="formrow">
          <label className="formlabel" htmlFor="ed-title">
            标题 <span className="req">*</span>
          </label>
          <input
            id="ed-title"
            className="input selectable"
            value={title}
            autoFocus
            aria-invalid={fieldErrors.title ? true : undefined}
            aria-describedby={fieldErrors.title ? 'ed-title-err' : undefined}
            onChange={(e) => setTitle(e.target.value)}
          />
          {fieldErrors.title && (
            <div className="formerr" id="ed-title-err" role="alert">
              {fieldErrors.title}
            </div>
          )}
        </div>

        <div className="formrow">
          <label className="formlabel" htmlFor="ed-desc">
            描述
          </label>
          <textarea
            id="ed-desc"
            className="input input--area selectable"
            rows={2}
            value={description}
            placeholder="一句话说明这件事要做什么"
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>

        <div className="formrow">
          <label className="formlabel" htmlFor="ed-link">
            链接
          </label>
          <input
            id="ed-link"
            className="input selectable"
            value={linkUrl}
            placeholder="https://…"
            aria-invalid={fieldErrors.linkUrl ? true : undefined}
            onChange={(e) => setLinkUrl(e.target.value)}
          />
          {fieldErrors.linkUrl && (
            <div className="formerr" role="alert">
              {fieldErrors.linkUrl}
            </div>
          )}
        </div>

        <div className="formgrid">
          <label className="formrow">
            <span className="formlabel">状态</span>
            <select
              className="input"
              value={status}
              onChange={(e) => setStatus(e.target.value as TaskStatus)}
            >
              {STATUS_OPTIONS.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>

          <label className="formrow">
            <span className="formlabel">优先级</span>
            <select
              className="input"
              value={priority}
              onChange={(e) => setPriority(Number(e.target.value))}
            >
              <option value={0}>无</option>
              <option value={1}>低</option>
              <option value={2}>中</option>
              <option value={3}>高</option>
            </select>
          </label>

          <label className="formrow">
            <span className="formlabel">项目</span>
            <select
              className="input"
              value={projectId}
              onChange={(e) => setProjectId(e.target.value)}
            >
              <option value="">（无项目）</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                  {p.isArchived === 1 ? '（已归档）' : ''}
                </option>
              ))}
            </select>
          </label>

          <label className="formrow">
            <span className="formlabel">分类</span>
            <select
              className="input"
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
            >
              <option value="">（无分类）</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
        </div>

        {/* ---------------- 时间：三个字段各自独立 ---------------- */}
        <fieldset className="formfieldset">
          <legend>时间</legend>
          <p className="setgroup__hint" style={{ marginTop: 0 }}>
            这三个时间含义不同：<strong>计划时间</strong>决定任务出现在「今天」和日历的哪一天；
            <strong>截止时间</strong>只用于判断是否逾期；<strong>提醒</strong>在任务卡片展开区单独设置。
            只填日期表示全天，不算作凌晨到期。
          </p>

          <div className="formgrid">
            <div className="formrow">
              <span className="formlabel">计划时间</span>
              <div className="dtd">
                <input
                  type="date"
                  className="input"
                  value={plannedDate}
                  aria-label="计划日期"
                  onChange={(e) => setPlannedDate(e.target.value)}
                />
                <input
                  type="time"
                  className="input"
                  value={plannedTime}
                  aria-label="计划时刻（留空表示仅日期）"
                  onChange={(e) => setPlannedTime(e.target.value)}
                />
                {(plannedDate || plannedTime) && (
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="清空计划时间"
                    title="清空"
                    onClick={() => {
                      setPlannedDate('')
                      setPlannedTime('')
                    }}
                  >
                    ✕
                  </button>
                )}
              </div>
            </div>

            <div className="formrow">
              <span className="formlabel">截止时间</span>
              <div className="dtd">
                <input
                  type="date"
                  className="input"
                  value={dueDate}
                  aria-label="截止日期"
                  onChange={(e) => setDueDate(e.target.value)}
                />
                <input
                  type="time"
                  className="input"
                  value={dueTime}
                  aria-label="截止时刻（留空表示仅日期）"
                  onChange={(e) => setDueTime(e.target.value)}
                />
                {(dueDate || dueTime) && (
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="清空截止时间"
                    title="清空"
                    onClick={() => {
                      setDueDate('')
                      setDueTime('')
                    }}
                  >
                    ✕
                  </button>
                )}
              </div>
            </div>
          </div>
        </fieldset>

        {/* ---------------- 耗时 ---------------- */}
        <div className="formgrid">
          <div className="formrow">
            <label className="formlabel" htmlFor="ed-est">
              预计耗时（分钟）
            </label>
            <input
              id="ed-est"
              type="number"
              min={0}
              className="input"
              value={estimated}
              onChange={(e) => setEstimated(e.target.value)}
            />
            {fieldErrors.estimated && (
              <div className="formerr" role="alert">
                {fieldErrors.estimated}
              </div>
            )}
          </div>

          <div className="formrow">
            <label className="formlabel" htmlFor="ed-act">
              实际耗时（分钟）
            </label>
            <input
              id="ed-act"
              type="number"
              min={0}
              className="input"
              value={actual}
              onChange={(e) => setActual(e.target.value)}
            />
            {fieldErrors.actual && (
              <div className="formerr" role="alert">
                {fieldErrors.actual}
              </div>
            )}
          </div>
        </div>

        {/* ---------------- 标签 ---------------- */}
        <div className="formrow">
          <span className="formlabel">标签</span>
          {tags.length === 0 ? (
            <p className="setgroup__hint" style={{ margin: 0 }}>
              还没有标签。可在「标签」页创建。
            </p>
          ) : (
            <div className="tagpicker">
              {tags.map((t) => {
                const on = tagIds.includes(t.id)
                return (
                  <button
                    key={t.id}
                    type="button"
                    className={`tagtoggle${on ? ' tagtoggle--on' : ''}`}
                    aria-pressed={on}
                    onClick={() =>
                      setTagIds((prev) => (on ? prev.filter((x) => x !== t.id) : [...prev, t.id]))
                    }
                  >
                    {t.color && (
                      <span
                        className="orgrow__swatch orgrow__swatch--round"
                        style={{ background: t.color }}
                        aria-hidden="true"
                      />
                    )}
                    {t.name}
                  </button>
                )
              })}
            </div>
          )}
        </div>

        {/* ---------------- 周期跨度 ---------------- */}
        <div className="formrow">
          <span className="formlabel">周期跨度</span>
          <div className="periodpick">
            {(
              [
                'none',
                'day',
                'week',
                'month',
                'quarter',
                'year',
              ] as PeriodType[]
            ).map((p) => (
              <button
                key={p}
                type="button"
                className={`tagtoggle${periodType === p ? ' tagtoggle--on' : ''}`}
                aria-pressed={periodType === p}
                onClick={() => setPeriodType(p)}
              >
                {p === 'none' ? '不限' : `${PERIOD_LABELS[p]}内完成`}
              </button>
            ))}
          </div>
          <p className="setgroup__hint" style={{ marginTop: 5 }}>
            周期跨度用于「这件事这周做完就行，不用定到某一天」这类任务。
            它<strong>不替代</strong>计划时间与截止时间：计划时间决定它出现在哪一天、
            日历显示在哪里；截止时间决定何时算逾期；周期只是声明一个柔性的完成窗口。
            <br />
            标记周期且<strong>不填计划时间</strong>的任务不会出现在「今天」视图里，
            只会出现在对应的「周期任务」视图中——避免每天弹出提醒。
          </p>
        </div>

        {/* ---------------- 标记 ---------------- */}
        <div className="formrow formrow--inline">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={isPinned}
              onChange={(e) => setIsPinned(e.target.checked)}
            />
            置顶（在列表中排在前面）
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={isFavorite}
              onChange={(e) => setIsFavorite(e.target.checked)}
            />
            收藏
          </label>
        </div>

        {/* ---------------- Markdown 备注 ---------------- */}
        <div className="formrow">
          <div className="formlabel formlabel--row">
            <span>Markdown 备注</span>
            <button
              type="button"
              className="btn btn--quiet btn--sm"
              aria-pressed={showPreview}
              onClick={() => setShowPreview((v) => !v)}
            >
              {showPreview ? '编辑' : '预览'}
            </button>
          </div>
          {showPreview ? (
            preview
          ) : (
            <textarea
              className="input input--area input--code selectable"
              rows={6}
              value={noteMd}
              placeholder={'支持 Markdown，例如：\n- [ ] 待办项\n**加粗**、`代码`、[链接](https://…)'}
              aria-label="Markdown 备注"
              onChange={(e) => setNoteMd(e.target.value)}
            />
          )}
        </div>

        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={onClose} disabled={saving}>
            取消
          </button>
          <button
            type="button"
            className="btn btn--primary"
            onClick={() => void save()}
            disabled={saving}
          >
            {saving ? '保存中…' : '保存'}
          </button>
        </div>
      </div>
    </div>
  )
}
