/**
 * 应用外壳：侧边栏 + 顶栏 + 内容区。
 *
 * 任务书 §3 要求：空列表、加载中、联网失败、保存失败都有明确状态；
 * 界面不出现假按钮或占位功能。因此未实现的视图显示"尚未实现"的
 * 明确卡片（含当前阶段），并提供一个指向设置的引导，
 * 而不是渲染一个看起来能用、实际无反应的界面。
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useApp } from './lib/store'
import { Sidebar, VIEW_META } from './components/Sidebar'
import { TaskCard } from './components/TaskCard'
import { QuickAdd } from './components/QuickAdd'
import { OrganizeView } from './components/OrganizeView'
import { SettingsView } from './components/SettingsView'
import { CalendarView } from './components/CalendarView'
import { BoardView } from './components/BoardView'
import { TaskEditor } from './components/TaskEditor'
import { bucketOf } from './lib/datetime'
import type { Task, ViewId } from './lib/types'

/** 具备真实实现的视图（其余显示"尚未实现"，杜绝假界面） */
const IMPLEMENTED_VIEWS = new Set<ViewId>([
  'today',
  'tomorrow',
  'week',
  'inbox',
  'all',
  'completed',
  'trash',
  'projects',
  'tags',
  'settings',
  'calendar',
  'board',
])

/** 使用组织管理界面的视图（项目与分类、标签） */
const ORGANIZE_VIEWS = new Set<ViewId>(['projects', 'tags'])

export default function App() {
  const {
    tasks,
    progressMap,
    loadState,
    loadError,
    overview,
    appInfo,
    view,
    search,
    sortBy,
    sortDesc,
    overdueOnly,
    toasts,
    init,
    reload,
    setView,
    setSearch,
    setSort,
    setOverdueOnly,
    toggleDone,
    remove,
    restore,
    purge,
    purgeAll,
    dismissToast,
    pushToast,
  } = useApp()

  const [showQuickAdd, setShowQuickAdd] = useState(false)
  /** 正在编辑的任务（null 表示编辑对话框关闭） */
  const [editing, setEditing] = useState<Task | null>(null)

  // ------------------------------ 启动 ------------------------------
  useEffect(() => {
    void init()
  }, [init])

  // --------------------- 后端事件（托盘菜单等） ---------------------
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen<string>('navigate', (e) => {
          const v = e.payload as ViewId
          if (v) setView(v)
        })
      } catch {
        // 非 Tauri 环境下忽略
      }
    })()
    return () => unlisten?.()
  }, [setView])

  // --------------------------- 键盘快捷键 ---------------------------
  // §4.4 要求关键操作可用键盘完成。此处提供应用内快捷键；
  // 全局（系统级）快捷键在阶段 4 通过 Rust 侧注册。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey
      if (!mod) return
      if (e.key === 'n') {
        e.preventDefault()
        setShowQuickAdd(true)
      } else if (e.key === 'f') {
        e.preventDefault()
        document.querySelector<HTMLInputElement>('.search__input')?.focus()
      } else if (e.key === 'r') {
        e.preventDefault()
        void reload()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [reload])

  const onCreated = useCallback(
    async (_t: Task) => {
      pushToast('success', '任务已创建')
      await reload()
      await useApp.getState().refreshOverview()
    },
    [reload, pushToast],
  )

  const meta = VIEW_META[view] ?? { title: '任务', subtitle: '' }

  // 侧边栏计数：只展示有真实数据的项，不硬编码假数字
  const counts = useMemo(() => {
    const c: Partial<Record<ViewId, number>> = {}
    if (overview) {
      c.today = overview.openTotal
    }
    c.trash = appInfo?.trashCount ?? 0
    return c
  }, [overview, appInfo])

  const sortedTasks = useMemo(() => {
    if (view !== 'today' && view !== 'tomorrow') return tasks
    // 今天/明天视图按桶排序，让逾期项与有具体时刻的项先出现
    const order = { overdue: 0, today: 1, tomorrow: 2, thisWeek: 3, later: 4, unscheduled: 5 }
    return [...tasks].sort((a, b) => order[bucketOf(a)] - order[bucketOf(b)])
  }, [tasks, view])

  return (
    <div className="app">
      <Sidebar current={view} onSelect={setView} counts={counts} version={appInfo?.version} />

      <div className="main">
        <header className="topbar">
          <div className="topbar__heading">
            <h1 className="topbar__title">{meta.title}</h1>
            <div className="topbar__subtitle">{meta.subtitle}</div>
          </div>

          <div className="topbar__actions">
            <div className="search">
              <span className="search__icon" aria-hidden="true">
                ⌕
              </span>
              <input
                className="search__input selectable"
                type="search"
                value={search}
                placeholder="搜索标题、描述、备注、项目、标签"
                aria-label="搜索任务"
                onChange={(e) => setSearch(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void reload()
                }}
              />
              {search && (
                <button
                  type="button"
                  className="search__clear"
                  aria-label="清除搜索"
                  onClick={() => {
                    setSearch('')
                    void reload()
                  }}
                >
                  ✕
                </button>
              )}
            </div>

            <button
              type="button"
              className={`btn btn--ghost btn--sm${overdueOnly ? ' btn--filter-on' : ''}`}
              aria-pressed={overdueOnly}
              onClick={() => setOverdueOnly(!overdueOnly)}
              title="只看已逾期任务"
            >
              逾期
            </button>

            <select
              className="btn btn--ghost btn--sm"
              value={`${sortBy}:${sortDesc ? 'desc' : 'asc'}`}
              aria-label="排序方式"
              onChange={(e) => {
                const [by, dir] = e.target.value.split(':')
                setSort(by as 'manual' | 'due' | 'priority' | 'created' | 'planned' | 'title', dir === 'desc')
              }}
            >
              <option value="manual:asc">手动排序</option>
              <option value="due:asc">按截止时间（近→远）</option>
              <option value="due:desc">按截止时间（远→近）</option>
              <option value="priority:desc">按优先级（高→低）</option>
              <option value="priority:asc">按优先级（低→高）</option>
              <option value="created:desc">按创建时间（新→旧）</option>
              <option value="created:asc">按创建时间（旧→新）</option>
              <option value="planned:asc">按计划时间（近→远）</option>
              <option value="title:asc">按标题</option>
            </select>

            <button
              type="button"
              className="btn btn--primary btn--sm"
              onClick={() => setShowQuickAdd((v) => !v)}
              title="新建任务（Ctrl+N）"
            >
              ＋ 新建
            </button>
          </div>
        </header>

        <main className="content">
          {showQuickAdd && IMPLEMENTED_VIEWS.has(view) && (
            <div style={{ maxWidth: 900, margin: '0 auto 12px' }}>
              <QuickAdd
                onCreated={onCreated}
                onCancel={() => setShowQuickAdd(false)}
                autoFocus
              />
            </div>
          )}

          {/* 回收站工具栏 */}
          {view === 'trash' && tasks.length > 0 && (
            <div style={{ maxWidth: 900, margin: '0 auto 12px', display: 'flex', gap: 8 }}>
              <button
                type="button"
                className="btn btn--danger btn--sm"
                onClick={() => {
                  if (window.confirm(`确定永久删除回收站中的全部 ${tasks.length} 项任务吗？此操作不可撤销。`)) {
                    void purgeAll()
                  }
                }}
              >
                清空回收站（{tasks.length}）
              </button>
            </div>
          )}

          <TaskArea
            view={view}
            tasks={sortedTasks}
            progressMap={progressMap}
            loadState={loadState}
            loadError={loadError}
            search={search}
            onToggle={toggleDone}
            onDelete={remove}
            onRestore={restore}
            onPurge={purge}
            onEdit={setEditing}
            onRetry={reload}
            onNew={() => setShowQuickAdd(true)}
            onGoSettings={() => setView('settings')}
          />
        </main>
      </div>

      {/* 完整编辑表单（§4.1 字段集） */}
      {editing && (
        <TaskEditor
          task={editing}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            pushToast('success', '已保存')
            await reload()
            await useApp.getState().refreshOverview()
          }}
        />
      )}

      {/* 提示条（§3 保存失败等要有明确反馈） */}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast--${t.kind}`}>
            <span aria-hidden="true">{t.kind === 'success' ? '✓' : t.kind === 'error' ? '⚠' : 'ℹ'}</span>
            <span style={{ flex: 1 }}>{t.text}</span>
            <button
              type="button"
              className="icon-btn"
              aria-label="关闭提示"
              onClick={() => dismissToast(t.id)}
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}

// =============================================================================
// 任务区域：负责"空 / 加载 / 失败 / 未实现"四种明确状态（§3）
// =============================================================================

interface TaskAreaProps {
  view: ViewId
  tasks: Task[]
  /** 任务 ID → 子任务进度（列表页批量取得） */
  progressMap: Record<string, { total: number; done: number; percent: number | null }>
  loadState: string
  loadError: string | null
  search: string
  onToggle: (id: string, done: boolean) => void
  onDelete: (id: string) => void
  onRestore: (id: string) => void
  onPurge: (id: string) => void
  /** 打开完整编辑表单 */
  onEdit: (task: Task) => void
  onRetry: () => void
  onNew: () => void
  onGoSettings: () => void
}

function TaskArea({
  view,
  tasks,
  progressMap,
  loadState,
  loadError,
  search,
  onToggle,
  onDelete,
  onRestore,
  onPurge,
  onEdit,
  onRetry,
  onNew,
  onGoSettings,
}: TaskAreaProps) {
  // 组织管理视图（项目与分类、标签）走专门界面
  if (ORGANIZE_VIEWS.has(view)) {
    return <OrganizeView />
  }

  // 设置页（外观、数据与备份、提醒、关于）
  if (view === 'settings') {
    return <SettingsView />
  }

  // 日历视图（日 / 周 / 月 + 拖拽改期）
  if (view === 'calendar') {
    return <CalendarView />
  }

  // 看板视图（按状态分列 + 拖拽改状态）
  if (view === 'board') {
    return <BoardView onEdit={onEdit} />
  }

  // 未实现的视图：明确说明，而不是假装能用
  if (!IMPLEMENTED_VIEWS.has(view)) {
    return (
      <div className="state">
        <div className="state__inner">
          <div className="state__icon" aria-hidden="true">
            🚧
          </div>
          <div className="state__title">「{VIEW_META[view]?.title ?? view}」尚未实现</div>
          <div className="state__text">
            该视图属于后续开发阶段，当前版本没有提供可用的界面。
            {'\n'}
            为避免误导，这里如实说明，而不是放一个看起来能点的按钮。
          </div>
          <div className="state__actions">
            <button type="button" className="btn btn--ghost" onClick={onGoSettings}>
              查看设置
            </button>
          </div>
        </div>
      </div>
    )
  }

  if (loadState === 'loading' && tasks.length === 0) {
    return (
      <div style={{ maxWidth: 900, margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 6 }}>
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="skeleton" />
        ))}
        <span className="sr-only">正在加载任务…</span>
      </div>
    )
  }

  if (loadState === 'error') {
    return (
      <div className="state">
        <div className="state__inner">
          <div className="state__icon" aria-hidden="true">
            ⚠
          </div>
          <div className="state__title">任务加载失败</div>
          <div className="state__text selectable">{loadError ?? '未知错误'}</div>
          <div className="state__actions">
            <button type="button" className="btn btn--primary" onClick={onRetry}>
              重试
            </button>
          </div>
        </div>
      </div>
    )
  }

  if (tasks.length === 0) {
    const isSearch = search.trim().length > 0
    return (
      <div className="state">
        <div className="state__inner">
          <div className="state__icon" aria-hidden="true">
            {isSearch ? '⌕' : view === 'trash' ? '🗑' : view === 'completed' ? '✓' : '☰'}
          </div>
          <div className="state__title">
            {isSearch
              ? '没有匹配的任务'
              : view === 'trash'
                ? '回收站是空的'
                : view === 'completed'
                  ? '还没有已完成的任务'
                  : '这里还没有任务'}
          </div>
          <div className="state__text">
            {isSearch
              ? `没有找到包含「${search.trim()}」的任务。可以换个关键词，或清空搜索查看全部。`
              : view === 'trash'
                ? '删除的任务会先进入回收站，可随时恢复。'
                : '按 Ctrl+N 或点击右上角「新建」开始添加。也可以直接写「明天 10:00 开会」。'}
          </div>
          {!isSearch && view !== 'trash' && view !== 'completed' && (
            <div className="state__actions">
              <button type="button" className="btn btn--primary" onClick={onNew}>
                ＋ 新建任务
              </button>
            </div>
          )}
        </div>
      </div>
    )
  }

  return (
    <ul className="tasklist">
      {tasks.map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          mode={view === 'trash' ? 'trash' : 'normal'}
          progress={progressMap[t.id]}
          onToggle={onToggle}
          onDelete={onDelete}
          onRestore={onRestore}
          onEdit={onEdit}
          onPurge={(id) => {
            if (window.confirm('永久删除后无法恢复，确定继续吗？')) onPurge(id)
          }}
        />
      ))}
    </ul>
  )
}
