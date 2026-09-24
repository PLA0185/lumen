/**
 * 设置页（任务书 §3 / §9）。
 *
 * 本页承担三件事：
 * 1. **外观**：浅色 / 深色 / 跟随系统、字体大小、界面缩放、动画开关（§3）。
 *    每项改动**立即生效**，让用户直接看到实际效果，而不是保存后才见效。
 * 2. **数据位置**：展示数据目录、数据库、日志目录，并可一键在资源管理器中打开（§9）。
 * 3. **备份与恢复**：导出、导入预览、恢复、备份管理、自动备份保留份数（§9）。
 *
 * 关于恢复：必须先预览再确认，且界面明确提示"将整体替换现有数据"。
 * 任务书把"数据丢失或不可恢复"列为阻断交付的问题，因此这里的措辞
 * 不能含糊——不使用"导入成功"这种掩盖后果的说法。
 */

import { useCallback, useEffect, useState } from 'react'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import { save, open } from '@tauri-apps/plugin-dialog'
import { relaunch } from '@tauri-apps/plugin-process'
import { UpdatePanel } from './UpdatePanel'
import * as bk from '../lib/backup-ipc'
import * as att from '../lib/attachment-ipc'
import { IpcError } from '../lib/ipc'
import * as rem from '../lib/reminder-ipc'
import { useApp } from '../lib/store'
import { WindowSettings } from './WindowSettings'
import { AiPanel } from './AiPanel'
import type { BackupEntry, ImportPreview } from '../lib/backup-ipc'
import type { DataPaths } from '../lib/types'
import { Icon } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

type Tab = 'appearance' | 'window' | 'ai' | 'data' | 'reminders' | 'about'

export function SettingsView() {
  const { appInfo, dataPaths, theme, setTheme, pushToast } = useApp()
  const [restoreNeedsRestart, setRestoreNeedsRestart] = useState(false)
  const [tab, setTab] = useState<Tab>('appearance')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // 外观（本地持久化，立即可见）
  const [scale, setScale] = useState(() => localStorage.getItem('lumen.uiScale') ?? '1')
  const [fontSize, setFontSize] = useState(() => localStorage.getItem('lumen.fontSize') ?? '14px')
  const [motion, setMotion] = useState(() => localStorage.getItem('lumen.motion') ?? 'on')

  // 备份
  const [backups, setBackups] = useState<BackupEntry[]>([])
  const [keep, setKeep] = useState(10)
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [pendingRestore, setPendingRestore] = useState<string | null>(null)

  // 提醒调度状态
  const [sched, setSched] = useState<rem.SchedulerStatus | null>(null)
  const [grace, setGrace] = useState(360)

  // 附件副本清理（第二轮整改任务书 §6.5）
  const [orphanResult, setOrphanResult] = useState<att.OrphanCleanupResult | null>(null)

  const doCleanupOrphans = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await att.attachmentCleanupOrphans()
      setOrphanResult(r)
      pushToast(
        'success',
        r.removed > 0 ? `已清理 ${r.removed} 个无引用的附件副本` : '没有发现需要清理的孤儿副本',
      )
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const loadBackups = useCallback(async () => {
    try {
      setBackups(await bk.backupList())
    } catch (e) {
      setError(errText(e))
    }
  }, [])

  const loadSched = useCallback(async () => {
    try {
      const s = await rem.reminderSchedulerStatus()
      setSched(s)
      setGrace(s.missedGraceMinutes)
    } catch {
      // 调度状态是可选信息，失败不打断设置页
    }
  }, [])

  useEffect(() => {
    void loadBackups()
    void loadSched()
  }, [loadBackups, loadSched])

  // ------------------------------ 外观 ------------------------------
  const applyScale = (v: string) => {
    setScale(v)
    localStorage.setItem('lumen.uiScale', v)
    document.documentElement.style.setProperty('--ui-scale', v)
  }

  const applyFontSize = (v: string) => {
    setFontSize(v)
    localStorage.setItem('lumen.fontSize', v)
    document.documentElement.style.setProperty('--font-size-base', v)
  }

  const applyMotion = (v: string) => {
    setMotion(v)
    localStorage.setItem('lumen.motion', v)
    if (v === 'off') document.documentElement.dataset.motion = 'off'
    else delete document.documentElement.dataset.motion
  }

  const applyTheme = (v: 'light' | 'dark' | 'system') => {
    setTheme(v)
    if (v === 'system') {
      delete document.documentElement.dataset.theme
      localStorage.removeItem('lumen.theme')
    } else {
      document.documentElement.dataset.theme = v
      localStorage.setItem('lumen.theme', v)
    }
  }

  // ------------------------------ 备份 ------------------------------
  const doExport = async () => {
    setBusy(true)
    setError(null)
    try {
      // 让用户选择保存位置；取消则回退到默认的备份目录
      const target = await save({
        title: '导出备份',
        defaultPath: `lumen-backup-${new Date().toISOString().slice(0, 10)}.lumen-backup.json`,
        filters: [{ name: 'Lumen 备份', extensions: ['json'] }],
      })
      const r = await bk.backupExport(typeof target === 'string' ? target : undefined)
      pushToast('success', `已导出 ${r.stats.tasks} 个任务到 ${r.path}`)
      if (r.attachmentWarning) {
        pushToast('info', r.attachmentWarning)
      }
      await loadBackups()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doExportCsv = async () => {
    setBusy(true)
    setError(null)
    try {
      const target = await save({
        title: '导出 CSV',
        defaultPath: `lumen-tasks-${new Date().toISOString().slice(0, 10)}.csv`,
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      })
      if (typeof target !== 'string') return
      const p = await bk.exportCsv(target)
      pushToast('success', `已导出 CSV：${p}`)
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doExportMarkdown = async () => {
    setBusy(true)
    setError(null)
    try {
      const target = await save({
        title: '导出 Markdown',
        defaultPath: `lumen-tasks-${new Date().toISOString().slice(0, 10)}.md`,
        filters: [{ name: 'Markdown', extensions: ['md'] }],
      })
      if (typeof target !== 'string') return
      const p = await bk.exportMarkdown(target)
      pushToast('success', `已导出 Markdown：${p}`)
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const pickBackupFile = async () => {
    setError(null)
    try {
      const picked = await open({
        title: '选择要导入的备份文件',
        multiple: false,
        filters: [{ name: 'Lumen 备份', extensions: ['json'] }],
      })
      if (typeof picked !== 'string') return
      setBusy(true)
      setPreview(await bk.backupPreview(picked))
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const previewFromList = async (path: string) => {
    setError(null)
    setBusy(true)
    try {
      setPreview(await bk.backupPreview(path))
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doRestore = async () => {
    if (!preview) return
    setBusy(true)
    setError(null)
    try {
      const r = await bk.backupRestore(preview.path)
      pushToast(
        'success',
        `已恢复 ${r.imported.tasks} 个任务、${r.imported.projects} 个项目` +
          (r.safetyBackup ? `\n恢复前的数据已快照保存` : '') +
          '\n请重启应用以完整应用备份中的设置',
      )
      setRestoreNeedsRestart(true)
      setPreview(null)
      setPendingRestore(null)
      await loadBackups()
      await useApp.getState().reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const doAutoBackup = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await bk.backupAuto(keep)
      pushToast('success', `已生成自动备份（${bk.formatBytes(r.bytes)}）`)
      await loadBackups()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const deleteBackup = async (b: BackupEntry) => {
    if (!window.confirm(`删除备份文件「${b.fileName}」？\n\n此操作只删除备份文件，不影响当前数据。`)) {
      return
    }
    try {
      await bk.backupDelete(b.path)
      pushToast('success', '已删除备份文件')
      await loadBackups()
    } catch (e) {
      setError(errText(e))
    }
  }

  const openDir = async (p: string | undefined, label: string) => {
    if (!p) return
    try {
      await openPath(p)
    } catch (e) {
      setError(`无法打开${label}：${errText(e)}`)
    }
  }

  const revealFile = async (p: string) => {
    try {
      await revealItemInDir(p)
    } catch (e) {
      setError(errText(e))
    }
  }

  // ------------------------------ 提醒设置 ------------------------------
  const applyGrace = async (v: number) => {
    setGrace(v)
    try {
      await rem.reminderSetGrace(v)
      await loadSched()
    } catch (e) {
      setError(errText(e))
    }
  }

  // ------------------------------ 渲染 ------------------------------
  const tabs: { id: Tab; label: string }[] = [
    { id: 'appearance', label: '外观' },
    { id: 'window', label: '窗口' },
    { id: 'ai', label: 'AI' },
    { id: 'data', label: '数据与备份' },
    { id: 'reminders', label: '提醒' },
    { id: 'about', label: '关于' },
  ]

  const paths: DataPaths | null = dataPaths

  return (
    <div className="settings">
      <div className="tabs" role="tablist" aria-label="设置分类">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={`tab${tab === t.id ? ' tab--active' : ''}`}
            onClick={() => {
              setTab(t.id)
              setError(null)
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {error && (
        <div className="alert alert--error" role="alert" style={{ marginTop: 12 }}>
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      )}

      {/* ---------------------------- 外观 ---------------------------- */}
      {tab === 'appearance' && (
        <div className="setgroup">
          <div className="setrow">
            <div className="setrow__label">
              <div className="setrow__title">主题</div>
              <div className="setrow__desc">深色模式在夜间更护眼；跟随系统会自动切换。</div>
            </div>
            <div className="segmented" role="radiogroup" aria-label="主题">
              {(
                [
                  ['light', '浅色'],
                  ['dark', '深色'],
                  ['system', '跟随系统'],
                ] as const
              ).map(([v, label]) => (
                <button
                  key={v}
                  type="button"
                  role="radio"
                  aria-checked={theme === v}
                  className={`segmented__item${theme === v ? ' segmented__item--on' : ''}`}
                  onClick={() => applyTheme(v)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <div className="setrow">
            <div className="setrow__label">
              <div className="setrow__title">字体大小</div>
              <div className="setrow__desc">调整后立即生效，便于在高分屏或视力不佳时阅读。</div>
            </div>
            <div className="setrow__control">
              <input
                type="range"
                min="12"
                max="20"
                step="1"
                value={parseInt(fontSize, 10) || 14}
                aria-label="字体大小（像素）"
                onChange={(e) => applyFontSize(`${e.target.value}px`)}
              />
              <span className="setrow__value">{parseInt(fontSize, 10) || 14} px</span>
            </div>
          </div>

          <div className="setrow">
            <div className="setrow__label">
              <div className="setrow__title">界面缩放</div>
              <div className="setrow__desc">整体放大或缩小界面，适合不同尺寸的显示器。</div>
            </div>
            <div className="setrow__control">
              <input
                type="range"
                min="0.8"
                max="1.5"
                step="0.05"
                value={Number(scale) || 1}
                aria-label="界面缩放比例"
                onChange={(e) => applyScale(e.target.value)}
              />
              <span className="setrow__value">{Math.round((Number(scale) || 1) * 100)}%</span>
            </div>
          </div>

          <div className="setrow">
            <div className="setrow__label">
              <div className="setrow__title">界面动画</div>
              <div className="setrow__desc">
                关闭后界面切换不再有过渡效果，可减少视觉干扰或提升低配机器流畅度。
              </div>
            </div>
            <div className="segmented" role="radiogroup" aria-label="界面动画">
              {(
                [
                  ['on', '开启'],
                  ['off', '关闭'],
                ] as const
              ).map(([v, label]) => (
                <button
                  key={v}
                  type="button"
                  role="radio"
                  aria-checked={motion === v}
                  className={`segmented__item${motion === v ? ' segmented__item--on' : ''}`}
                  onClick={() => applyMotion(v)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ---------------------------- 窗口 ---------------------------- */}
      {tab === 'window' && <WindowSettings />}

      {/* ---------------------------- AI ---------------------------- */}
      {tab === 'ai' && <AiPanel />}

      {/* ---------------------------- 数据与备份 ---------------------------- */}
      {tab === 'data' && (
        <>
          <div className="setgroup">
            <h3 className="setgroup__title">数据位置</h3>
            <p className="setgroup__desc">
              所有数据都保存在本机，不需要注册账号。卸载时安装程序会询问是否一并删除这些数据。
            </p>

            <div className="pathrow">
              <span className="pathrow__label">数据目录</span>
              <code className="pathrow__value selectable">{paths?.dataDir ?? '读取中…'}</code>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => void openDir(paths?.dataDir, '数据目录')}
              >
                打开
              </button>
            </div>

            <div className="pathrow">
              <span className="pathrow__label">数据库</span>
              <code className="pathrow__value selectable">{paths?.dbPath ?? '读取中…'}</code>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => paths && void revealFile(paths.dbPath)}
              >
                定位
              </button>
            </div>

            <div className="pathrow">
              <span className="pathrow__label">备份目录</span>
              <code className="pathrow__value selectable">{paths?.backupDir ?? '读取中…'}</code>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                onClick={() => void openDir(paths?.backupDir, '备份目录')}
              >
                打开
              </button>
            </div>

            <p className="setgroup__hint">
              数据库结构版本：<strong>{paths?.schemaVersion ?? '未知'}</strong>
              　当前共 <strong>{appInfo?.taskCount ?? 0}</strong> 个任务
              {(appInfo?.trashCount ?? 0) > 0 && (
                <>
                  ，回收站 <strong>{appInfo?.trashCount}</strong> 项
                </>
              )}
            </p>
          </div>

          <div className="setgroup">
            <h3 className="setgroup__title">导出</h3>
            <p className="setgroup__desc">
              完整备份包含任务、重复规则、标签、子任务、依赖、提醒与设置，并附带版本号与
              SHA-256 校验和。恢复时会先校验，校验不通过将拒绝导入。
            </p>
            <div className="setactions">
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy}
                onClick={() => void doExport()}
              >
                导出完整备份（JSON）
              </button>
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void doExportCsv()}
              >
                导出 CSV
              </button>
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void doExportMarkdown()}
              >
                导出 Markdown
              </button>
            </div>
            <p className="setgroup__hint">
              注意：备份文件包含任务数据但不包含附件文件本体。如需连附件一起备份，
              请另行复制数据目录中的 attachments 文件夹。
            </p>
          </div>

          {/*
            附件副本清理（第二轮整改任务书 §6.5）。
            永久删除任务时程序会顺手删掉自己复制出来的附件副本；但如果当时删除失败
            （文件被占用等），就会留下"数据库里没记录、磁盘上还占着"的孤儿文件。
            这里提供一个显式的清理入口，并说清楚它**只动什么**。
          */}
          <div className="setgroup">
            <h3 className="setgroup__title">附件副本清理</h3>
            <p className="setgroup__desc">
              永久删除任务时，Lumen 会同时删掉自己复制到数据目录里的附件副本。
              如果那次删除因为文件被占用等原因失败，副本文件会留在磁盘上成为孤儿。
              这里可以扫描并清理它们。
            </p>
            <p className="setgroup__hint">
              只会删除<strong>位于受控附件目录内、且命名为 Lumen 生成的 UUID 形式、
              数据库已无任何引用</strong>的文件。你自己放进该目录的文件、
              以及所有引用模式的原文件都不会被触碰。
            </p>
            <div className="setactions">
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void doCleanupOrphans()}
              >
                扫描并清理孤儿副本
              </button>
            </div>
            {orphanResult && (
              <p className="setgroup__hint" role="status">
                扫描 {orphanResult.scanned} 个文件：仍被引用 {orphanResult.kept} 个，
                已清理 {orphanResult.removed} 个，跳过 {orphanResult.skipped} 个
                {orphanResult.removedFiles.length > 0 &&
                  `（${orphanResult.removedFiles.slice(0, 5).join('、')}${
                    orphanResult.removedFiles.length > 5 ? ' …' : ''
                  }）`}
              </p>
            )}
          </div>

          <div className="setgroup">
            <h3 className="setgroup__title">自动备份</h3>
            <p className="setgroup__desc">
              超出保留份数的旧自动备份会被自动清理。手动备份不会被清理。
            </p>
            <div className="setactions">
              <label className="field">
                保留份数
                <input
                  type="number"
                  className="input input--compact"
                  min={1}
                  max={100}
                  value={keep}
                  aria-label="自动备份保留份数"
                  onChange={(e) => setKeep(Math.max(1, Math.min(100, Number(e.target.value) || 10)))}
                />
              </label>
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void doAutoBackup()}
              >
                立即备份一次
              </button>
            </div>
          </div>

          <div className="setgroup">
            <h3 className="setgroup__title">恢复</h3>
            <p className="setgroup__desc">
              恢复会<strong>整体替换</strong>当前所有数据。程序会在替换前自动为当前数据生成一份快照，
              数据记录可用这份快照退回；附件文件本体不在 JSON 备份中。请务必先查看下面的预览。
            </p>
            <div className="setactions">
              <button
                type="button"
                className="btn btn--ghost"
                disabled={busy}
                onClick={() => void pickBackupFile()}
              >
                选择备份文件…
              </button>
            </div>
          </div>

          {restoreNeedsRestart && (
            <div className="alert alert--warn" role="status">
              数据已恢复。备份中的窗口、AI 等运行时设置需要重启后才会完整生效。
              <button type="button" className="btn btn--primary"
                onClick={() => void relaunch()}>立即重启</button>
            </div>
          )}

          {preview && (
            <div className="preview">
              <h3 className="setgroup__title">导入预览</h3>
              <table className="kvtable">
                <tbody>
                  <tr>
                    <th>备份文件</th>
                    <td className="selectable">{preview.path}</td>
                  </tr>
                  <tr>
                    <th>生成时间</th>
                    <td>{preview.createdAt}</td>
                  </tr>
                  <tr>
                    <th>格式版本</th>
                    <td>
                      {preview.formatVersion}
                      {preview.appVersion && `（由 Lumen ${preview.appVersion} 生成）`}
                    </td>
                  </tr>
                  <tr>
                    <th>校验和</th>
                    <td>
                      {preview.checksumOk ? (
                        <span className="chip chip--ok">校验通过</span>
                      ) : (
                        <span className="chip chip--warn">校验失败</span>
                      )}
                      {preview.checksumError && (
                        <div className="selectable" style={{ marginTop: 4 }}>
                          {preview.checksumError}
                        </div>
                      )}
                    </td>
                  </tr>
                  <tr>
                    <th>备份内容</th>
                    <td>
                      任务 {preview.stats.tasks}　项目 {preview.stats.projects}　分类{' '}
                      {preview.stats.categories}　标签 {preview.stats.tags}
                      <br />
                      子任务 {preview.stats.subtasks}　依赖 {preview.stats.dependencies}　提醒{' '}
                      {preview.stats.reminders}　重复系列 {preview.stats.series}
                    </td>
                  </tr>
                  <tr>
                    <th>当前数据</th>
                    <td className="setgroup__hint">
                      任务 {preview.current.tasks}　项目 {preview.current.projects}　标签{' '}
                      {preview.current.tags}　提醒 {preview.current.reminders}
                    </td>
                  </tr>
                  <tr>
                    <th>附件</th>
                    <td>{preview.attachmentsNote}</td>
                  </tr>
                </tbody>
              </table>

              {preview.blockingIssues.length > 0 && (
                <div className="alert alert--error" role="alert">
                  <div>
                    <strong>无法导入：</strong>
                    <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
                      {preview.blockingIssues.map((s) => (
                        <li key={s}>{s}</li>
                      ))}
                    </ul>
                  </div>
                </div>
              )}

              {preview.willReplaceTasks > 0 && preview.blockingIssues.length === 0 && (
                <div className="alert alert--warn" role="alert">
                  恢复后，当前库中的 <strong>{preview.willReplaceTasks}</strong> 个任务将被备份内容替换。
                  程序会先为当前数据生成快照，但仍建议你现在先自行导出一份。
                  JSON 备份不包含附件文件本体，安全快照也不保证能恢复附件文件。
                </div>
              )}

              <div className="setactions">
                <button
                  type="button"
                  className="btn btn--ghost"
                  onClick={() => {
                    setPreview(null)
                    setPendingRestore(null)
                  }}
                >
                  取消
                </button>
                {pendingRestore === preview.path ? (
                  <button
                    type="button"
                    className="btn btn--danger-solid"
                    disabled={busy || preview.blockingIssues.length > 0}
                    onClick={() => void doRestore()}
                  >
                    {busy ? '恢复中…' : '确认恢复（将替换现有数据）'}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn btn--danger"
                    disabled={preview.blockingIssues.length > 0}
                    onClick={() => setPendingRestore(preview.path)}
                  >
                    恢复这份备份…
                  </button>
                )}
              </div>
            </div>
          )}

          <div className="setgroup">
            <h3 className="setgroup__title">备份文件</h3>
            {backups.length === 0 ? (
              <p className="setgroup__hint">备份目录中还没有备份文件。</p>
            ) : (
              <ul className="backuplist">
                {backups.map((b) => (
                  <li key={b.path} className="backuprow">
                    <span className="chip chip--muted">{b.kind}</span>
                    <span className="backuprow__name selectable">{b.fileName}</span>
                    <span className="backuprow__meta">
                      {bk.formatBytes(b.bytes)}　{b.modifiedAt}
                    </span>
                    <span className="orgrow__actions">
                      <button
                        type="button"
                        className="icon-btn"
                        title="查看这份备份的内容"
                        aria-label={`预览 ${b.fileName}`}
                        onClick={() => void previewFromList(b.path)}
                      >
                        <Icon name="search" size={15} />
                      </button>
                      <button
                        type="button"
                        className="icon-btn"
                        title="在资源管理器中定位"
                        aria-label={`定位 ${b.fileName}`}
                        onClick={() => void revealFile(b.path)}
                      >
                        <Icon name="folder-open" size={15} />
                      </button>
                      <button
                        type="button"
                        className="icon-btn icon-btn--danger"
                        title="删除这个备份文件"
                        aria-label={`删除 ${b.fileName}`}
                        onClick={() => void deleteBackup(b)}
                      >
                        <Icon name="close" size={14} />
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </>
      )}

      {/* ---------------------------- 提醒 ---------------------------- */}
      {tab === 'reminders' && (
        <div className="setgroup">
          <h3 className="setgroup__title">提醒调度</h3>
          <p className="setgroup__desc">
            Lumen 自己负责调度提醒（不使用系统计划任务），因此程序关闭期间到期的提醒会在下次
            启动时处理。下面的窗口决定"补发多久以内错过的提醒"。
          </p>

          <div className="setrow">
            <div className="setrow__label">
              <div className="setrow__title">错过的提醒补发窗口</div>
              <div className="setrow__desc">
                关机一晚后开机，6 小时窗口内的提醒会被补发；更早的会被标记为"已过期"，
                以免一次性弹出大量历史提醒。设为 0 表示不补发。
              </div>
            </div>
            <div className="setrow__control">
              <input
                type="range"
                min={0}
                max={1440}
                step={30}
                value={grace}
                aria-label="补发窗口（分钟）"
                onChange={(e) => void applyGrace(Number(e.target.value))}
              />
              <span className="setrow__value">
                {grace === 0 ? '不补发' : grace < 60 ? `${grace} 分钟` : `${(grace / 60).toFixed(1)} 小时`}
              </span>
            </div>
          </div>

          {sched && (
            <table className="kvtable">
              <tbody>
                <tr>
                  <th>调度器</th>
                  <td>{sched.running ? '运行中' : '已停止'}</td>
                </tr>
                <tr>
                  <th>状态</th>
                  <td>{sched.paused ? '已暂停（可从托盘恢复）' : '正常'}</td>
                </tr>
                <tr>
                  <th>待触发的提醒</th>
                  <td>{sched.pendingCount} 条</td>
                </tr>
                <tr>
                  <th>已过期作废</th>
                  <td>
                    {sched.expiredCount} 条
                    {sched.expiredCount > 0 && (
                      <span className="setgroup__hint">
                        　（因超出补发窗口被标记，不是静默丢弃）
                      </span>
                    )}
                  </td>
                </tr>
              </tbody>
            </table>
          )}

          <div className="setactions">
            <button
              type="button"
              className="btn btn--ghost"
              onClick={async () => {
                try {
                  const n = await rem.reminderCheckMissed()
                  pushToast('success', n > 0 ? `已处理 ${n} 条过期提醒` : '没有需要处理的过期提醒')
                  await loadSched()
                } catch (e) {
                  setError(errText(e))
                }
              }}
            >
              立即检查错过的提醒
            </button>
            <button type="button" className="btn btn--ghost" onClick={() => void loadSched()}>
              刷新状态
            </button>
          </div>

          <p className="setgroup__hint">
            已知限制：Windows 的系统通知在桌面端不支持"点击通知跳转到该任务"，
            因此通知会显示任务标题，需要你手动在应用中查看。
          </p>
        </div>
      )}

      {/* ---------------------------- 关于 ---------------------------- */}
      {tab === 'about' && (
        <div className="setgroup">
          <h3 className="setgroup__title">关于 Lumen</h3>
          <table className="kvtable">
            <tbody>
              <tr>
                <th>版本</th>
                <td>{appInfo?.version ?? '读取中…'}</td>
              </tr>
              <tr>
                <th>状态</th>
                <td>{appInfo?.status === 'ok' ? '正常' : (appInfo?.status ?? '读取中…')}</td>
              </tr>
              <tr>
                <th>任务总数</th>
                <td>{appInfo?.taskCount ?? 0}</td>
              </tr>
              <tr>
                <th>回收站</th>
                <td>{appInfo?.trashCount ?? 0}</td>
              </tr>
              <tr>
                <th>数据存储</th>
                <td>本地 SQLite，无需联网即可使用全部基础功能</td>
              </tr>
            </tbody>
          </table>
          <p className="setgroup__hint">
            Lumen 是本地优先的应用：不配置 AI 服务时，任务管理、重复规则与提醒全部可用。
          </p>
        </div>
      )}

      {/* ---------------------------- 软件更新（§9） ---------------------------- */}
      {tab === 'about' && <UpdatePanel currentVersion={appInfo?.version} />}
    </div>
  )
}
