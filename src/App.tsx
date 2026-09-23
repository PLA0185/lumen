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
import { IpcError } from './lib/ipc'
import * as ipc from './lib/ipc'
import { Sidebar, VIEW_META } from './components/Sidebar'
import { TaskCard } from './components/TaskCard'
import { QuickAdd } from './components/QuickAdd'
import { OrganizeView } from './components/OrganizeView'
import { SettingsView } from './components/SettingsView'
import { CalendarView } from './components/CalendarView'
import { StatsView } from './components/StatsView'
import { BoardView } from './components/BoardView'
import { TaskEditor } from './components/TaskEditor'
import { PrintReport } from './components/PrintReport'
import { RecurringTaskDialog } from './components/RecurringTaskDialog'
import { bucketOf } from './lib/datetime'
import * as bus from './lib/bus'
import type { Task, ViewId } from './lib/types'
import { Icon } from './components/Icons'

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
  'stats',
  // 周期任务视图：与"本周安排"是不同维度（按任务自身的周期跨度筛选）
  'period-week',
  'period-month',
  'period-quarter',
  'period-year',
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
    duplicate,
    reorder,
    remove,
    restore,
    purge,
    purgeAll,
    reportRows,
    dismissToast,
    pushToast,
    pushReminder,
  } = useApp()

  const [showQuickAdd, setShowQuickAdd] = useState(false)
  /** 正在编辑的任务（null 表示编辑对话框关闭） */
  const [editing, setEditing] = useState<Task | null>(null)
  /** 新建重复任务对话框 */
  const [showRecurring, setShowRecurring] = useState(false)
  /** PDF 导出期间的打印报告数据（非 null 时界面切到打印视图） */
  const [printData, setPrintData] = useState<{
    rows: ipc.TaskReportRow[]
    scopeTitle: string
    filterNote?: string
  } | null>(null)
  const [exporting, setExporting] = useState(false)

  // ------------------------------ 启动 ------------------------------
  useEffect(() => {
    void init()
  }, [init])

  /** 打印视图开关：交给 CSS 决定"只显示报告" */
  useEffect(() => {
    if (printData) {
      document.body.dataset.print = 'on'
    } else {
      delete document.body.dataset.print
    }
    return () => {
      delete document.body.dataset.print
    }
  }, [printData])

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

  // --------------------- 启动后静默检查更新（§9） ---------------------
  /**
   * 延迟 20 秒再查：避免和启动时的数据库初始化、首屏加载抢带宽与主线程。
   * 只提示、不自动安装——安装会让程序退出，必须由用户决定时机。
   */
  useEffect(() => {
    let cancelled = false
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const up = await import('./lib/update-ipc')
          const { info } = await up.checkUpdate()
          if (!cancelled && info) {
            pushToast(
              'info',
              `发现新版本 ${info.version}（当前 ${info.currentVersion}）。到「设置 → 关于」可一键更新。`,
            )
          }
        } catch {
          // 静默检查失败不打扰用户：网络原因很常见，
          // 用户真要更新时会去「关于」里手动检查并看到具体原因
        }
      })()
    }, 20_000)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [pushToast])

  // --------------------- 跨窗口同步（悬浮窗改了要立刻反映到这里） ---------------------
  useEffect(() => {
    return bus.onTasksChanged(() => {
      void reload()
      void useApp.getState().refreshOverview()
    })
  }, [reload])

  // --------------------- 提醒触发 → 可点开任务 ---------------------
  /**
   * 桌面端系统通知**没有点击回调**（Windows 的 toast 激活需要打包身份，
   * Tauri 的通知插件在桌面不暴露该事件）。因此"点击通知跳转任务"
   * 改用应用内提醒条实现：提醒一到就出现在界面上，点「打开任务」
   * 直接定位到该任务，不需要用户自己去找。
   */
  const openTaskById = useCallback(
    async (taskId: string) => {
      try {
        const t = await ipc.getTask(taskId)
        setEditing(t)
      } catch (e) {
        pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
      }
    },
    [pushToast],
  )

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen<{ id: string; taskId: string; title: string }>(
          'reminder-fired',
          (e) => {
            pushReminder(`提醒：${e.payload.title}`, e.payload.taskId)
            void reload()
          },
        )
      } catch {
        // 非 Tauri 环境下忽略
      }
    })()
    return () => unlisten?.()
  }, [pushReminder, reload])

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

  // ---------------------- 拖拽排序（§4.1 手动排序） ----------------------
  /** 正在被拖动的任务 id */
  const [dragId, setDragId] = useState<string | null>(null)
  /** 落点：插入到这个任务之前 */
  const [dropBeforeId, setDropBeforeId] = useState<string | null>(null)

  // 只有"手动排序 + 无搜索"时拖拽才有意义：其它排序方式下顺序由字段决定，
  // 拖了也会被服务端排序覆盖，与其给一个假交互不如直接禁用。
  const canSort = sortBy === 'manual' && search.trim().length === 0 && view !== 'trash'

  const handleDragOverCard = useCallback(
    (overId: string) => {
      if (!dragId || overId === dragId) return
      setDropBeforeId(overId)
    },
    [dragId],
  )

  const handleDragEndCard = useCallback(() => {
    const movedId = dragId
    const beforeId = dropBeforeId
    setDragId(null)
    setDropBeforeId(null)
    if (!movedId || !beforeId || movedId === beforeId) return
    void reorder(movedId, beforeId)
  }, [dragId, dropBeforeId, reorder])

  const handleDragStartCard = useCallback((id: string) => setDragId(id), [])

  // ---------------------------- PDF 导出 ----------------------------
  /**
   * 导出流程（顺序很重要）：
   * 1. 先让用户选保存位置；
   * 2. 取报告数据并切到打印视图（WebView2 打印的是**当前页面**）；
   * 3. 等两帧 + 一点余量，确保表格布局与字体都已就绪，
   *    否则可能出现"导出的 PDF 是上一个界面"或半张空白；
   * 4. 调后端 PrintToPdf，最后无论成败都恢复界面。
   */
  const doExportPdf = useCallback(async () => {
    setExporting(true)
    try {
      const { save } = await import('@tauri-apps/plugin-dialog')
      const stamp = new Date().toISOString().slice(0, 10)
      const target = await save({
        title: '导出 PDF',
        defaultPath: `lumen-tasks-${stamp}.pdf`,
        filters: [{ name: 'PDF 文件', extensions: ['pdf'] }],
      })
      if (typeof target !== 'string') return

      const rows = await reportRows()
      if (rows.length === 0) {
        pushToast('info', '当前范围内没有任务，未生成 PDF')
        return
      }
      setPrintData({
        rows,
        scopeTitle: meta.title,
        filterNote: search.trim() ? `搜索「${search.trim()}」` : undefined,
      })

      await new Promise<void>((r) =>
        requestAnimationFrame(() => requestAnimationFrame(() => r())),
      )
      await new Promise<void>((r) => window.setTimeout(r, 180))

      await ipc.exportPdf(target)
      pushToast('success', `已导出 ${rows.length} 条任务到 ${target}`)
    } catch (e) {
      pushToast('error', e instanceof IpcError ? e.userMessage() : String(e))
    } finally {
      setPrintData(null)
      setExporting(false)
    }
  }, [meta.title, pushToast, reportRows, search])

  return (
    <>
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
                <Icon name="search" size={15} />
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
                  <Icon name="close" size={14} />
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
              className="btn btn--ghost btn--sm"
              disabled={exporting || loadState !== 'ready'}
              onClick={() => void doExportPdf()}
              title="把当前列表导出为 PDF（含项目、标签、时间等字段）"
            >
              {exporting ? '导出中…' : <><Icon name="download" size={15} /> PDF</>}
            </button>

            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={() => setShowRecurring(true)}
              title="新建可以按规则重复的任务"
            >
              <Icon name="repeat" size={15} /> 重复任务
            </button>

            <button
              type="button"
              className="btn btn--primary btn--sm"
              onClick={() => setShowQuickAdd((v) => !v)}
              title="新建任务（Ctrl+N）"
            >
              <Icon name="plus" size={15} /> 新建
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
            onDuplicate={(t) => void duplicate(t.id)}
            sortable={canSort}
            dragId={dragId}
            dropBeforeId={dropBeforeId}
            onDragStartCard={handleDragStartCard}
            onDragOverCard={handleDragOverCard}
            onDragEndCard={handleDragEndCard}
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

      {/* 新建重复任务（§5） */}
      {showRecurring && (
        <RecurringTaskDialog
          onClose={() => setShowRecurring(false)}
          onCreated={async () => {
            pushToast('success', '重复任务已创建，后续发生已按规则生成')
            await reload()
            await useApp.getState().refreshOverview()
          }}
        />
      )}

      {/* 提示条（§3 保存失败等要有明确反馈） */}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast--${t.kind}`}>
            <span className={`toast__icon toast__icon--${t.kind}`} aria-hidden="true">
              <Icon
                name={
                  t.kind === 'success'
                    ? 'completed'
                    : t.kind === 'error'
                      ? 'alert'
                      : t.kind === 'reminder'
                        ? 'clock'
                        : 'info'
                }
                size={16}
              />
            </span>
            <span style={{ flex: 1 }}>{t.text}</span>
            {/* 提醒条提供跳转：这是"点击通知打开任务"在桌面端的可用替代 */}
            {t.taskId && (
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => {
                  void openTaskById(t.taskId as string)
                  dismissToast(t.id)
                }}
              >
                打开任务
              </button>
            )}
            <button
              type="button"
              className="icon-btn"
              aria-label="关闭提示"
              onClick={() => dismissToast(t.id)}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        ))}
      </div>
    </div>

    {/*
      打印报告**必须是 `.app` 的兄弟节点**，不能放在里面：
      导出 PDF 时用 `body[data-print='on'] .app { display: none }` 隐藏主界面，
      报告若在 `.app` 内会跟着被隐藏——WebView2 打印的是"当前页面"，
      结果就是导出一张空白 PDF（实测踩过：导出的文件只有 1 KB，没有任何内容）。
    */}
    {printData && (
      <PrintReport
        rows={printData.rows}
        scopeTitle={printData.scopeTitle}
        filterNote={printData.filterNote}
      />
    )}
    </>
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
  /** 复制为副本 */
  onDuplicate: (task: Task) => void
  /** 是否允许拖拽排序 */
  sortable: boolean
  dragId: string | null
  dropBeforeId: string | null
  onDragStartCard: (id: string) => void
  onDragOverCard: (id: string) => void
  onDragEndCard: () => void
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
  onDuplicate,
  sortable,
  dragId,
  dropBeforeId,
  onDragStartCard,
  onDragOverCard,
  onDragEndCard,
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

  // 统计与成长（§7）
  if (view === 'stats') {
    return <StatsView />
  }

  // 未实现的视图：明确说明，而不是假装能用
  if (!IMPLEMENTED_VIEWS.has(view)) {
    return (
      <div className="state">
        <div className="state__inner">
          <div className="state__icon" aria-hidden="true">
            <Icon name="info" size={30} strokeWidth={1.5} />
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
            <Icon name="alert" size={30} strokeWidth={1.5} />
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
            <Icon
              name={isSearch ? 'search' : view === 'trash' ? 'trash' : view === 'completed' ? 'completed' : 'list'}
              size={30}
              strokeWidth={1.5}
            />
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
                <Icon name="plus" size={15} /> 新建任务
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
          onDuplicate={onDuplicate}
          sortable={sortable}
          isDragging={dragId === t.id}
          dropHint={dropBeforeId === t.id && dragId !== t.id ? 'before' : null}
          onDragStartCard={onDragStartCard}
          onDragOverCard={onDragOverCard}
          onDragEndCard={onDragEndCard}
          onPurge={(id) => {
            if (window.confirm('永久删除后无法恢复，确定继续吗？')) onPurge(id)
          }}
        />
      ))}
    </ul>
  )
}
