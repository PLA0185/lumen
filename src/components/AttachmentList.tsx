/**
 * 附件列表（任务书 §4.1：受控存储，删除任务不得误删用户原文件）。
 *
 * ## 两种存储模式的差别必须在界面上说清
 *
 * - **仅记录**（reference）：只在数据库里记一条指向你原文件的信息。
 *   移动或改名原文件后，这里就找不到它了。不占额外磁盘空间。
 * - **复制到附件目录**（copied）：把文件复制一份到 Lumen 的数据目录里统一管理。
 *   删除任务时删的是这份副本，<strong>你的原文件始终不受影响</strong>。
 *
 * 界面默认选「仅记录」，因为这是最不会让用户意外丢失或重复占用空间的选项；
 * 需要"备份后仍能找回文件"的用户可以显式选择复制。
 */

import { useCallback, useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import * as att from '../lib/attachment-ipc'
import { IpcError } from '../lib/ipc'
import type { Attachment } from '../lib/attachment-ipc'
import { Icon, iconForMime, type IconName } from './Icons'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

/** 字节数 → 可读文本 */
function humanSize(bytes: number | null): string {
  if (bytes == null) return '未知大小'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
}

/**
 * 按 MIME 选图标。
 *
 * 统一走 `Icons.tsx` 的 `iconForMime`：原来的实现返回 emoji
 * （🖼 📕 🎵 🎬 🗜 📄 📃 📎），与其它地方的自绘图标完全不是一个体系，
 * 而且是彩色的，在深色主题里格外突兀。
 */
function iconNameFor(a: Attachment): IconName {
  return iconForMime(a.mimeType ?? '')
}

export function AttachmentList({ taskId }: { taskId: string }) {
  const [items, setItems] = useState<Attachment[]>([])
  /** 附件 ID → 文件是否仍存在（引用模式的附件可能已被移动） */
  const [exists, setExists] = useState<Record<string, boolean>>({})
  const [mode, setMode] = useState<'reference' | 'copied'>('reference')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [list, checks] = await Promise.all([
        att.attachmentList(taskId),
        att.attachmentCheck(taskId),
      ])
      setItems(list)
      setExists(Object.fromEntries(checks.map((c) => [c.id, c.exists])))
      setError(null)
    } catch (e) {
      setError(errText(e))
    } finally {
      setLoading(false)
    }
  }, [taskId])

  useEffect(() => {
    void reload()
  }, [reload])

  const addFiles = async () => {
    setError(null)
    try {
      const picked = await open({
        title: '选择要添加的附件',
        multiple: true,
        directory: false,
      })
      if (!picked) return
      const list = Array.isArray(picked) ? picked : [picked]

      setBusy(true)
      for (const p of list) {
        await att.attachmentAdd(taskId, p, mode)
      }
      await reload()
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (a: Attachment) => {
    const msg =
      a.storageMode === 'copied'
        ? `移除附件「${a.fileName}」？\n\n会删除 Lumen 附件目录中的副本，你的原文件不受影响。`
        : `移除附件「${a.fileName}」？\n\n只会删除这条记录，你的原文件不会被删除。`
    if (!window.confirm(msg)) return
    try {
      await att.attachmentRemove(a.id)
      await reload()
    } catch (e) {
      setError(errText(e))
    }
  }

  const reveal = async (a: Attachment) => {
    setError(null)
    try {
      const p = await att.attachmentReveal(a.id)
      await revealItemInDir(p)
    } catch (e) {
      setError(errText(e))
    }
  }

  return (
    <div className="attach">
      <div className="subtasks__head">
        <span>附件</span>
        {items.length > 0 && <span className="chip chip--muted">{items.length} 个</span>}
      </div>

      {loading ? (
        <div className="skeleton" style={{ height: 26 }} />
      ) : items.length === 0 ? (
        <p className="reminders__empty">还没有附件。</p>
      ) : (
        <ul className="attachlist">
          {items.map((a) => {
            const gone = exists[a.id] === false
            return (
              <li key={a.id} className={`attachrow${gone ? ' attachrow--missing' : ''}`}>
                <span className="attachrow__icon" aria-hidden="true">
                  <Icon name={iconNameFor(a)} size={16} />
                </span>
                <span className="attachrow__name" title={a.fileName}>
                  {a.fileName}
                </span>
                <span className="attachrow__meta">
                  {humanSize(a.byteSize)}
                  {a.storageMode === 'copied' ? '　已复制' : '　仅记录'}
                </span>
                {gone && (
                  <span className="chip chip--warn" title="文件已不在原位置">
                    文件已丢失
                  </span>
                )}
                <span className="orgrow__actions">
                  <button
                    type="button"
                    className="icon-btn"
                    title="在资源管理器中显示"
                    aria-label={`定位附件 ${a.fileName}`}
                    onClick={() => void reveal(a)}
                  >
                    <Icon name="folder-open" size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    title="移除附件（不删除原文件）"
                    aria-label={`移除附件 ${a.fileName}`}
                    onClick={() => void remove(a)}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </span>
              </li>
            )
          })}
        </ul>
      )}

      <div className="attachnew">
        <select
          className="input input--compact"
          value={mode}
          aria-label="附件存储方式"
          onChange={(e) => setMode(e.target.value as 'reference' | 'copied')}
        >
          <option value="reference">仅记录（不动原文件）</option>
          <option value="copied">复制到附件目录（备份后可找回）</option>
        </select>
        <button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void addFiles()}>
          {busy ? '处理中…' : '添加文件…'}
        </button>
      </div>

      <p className="remnew__hint" style={{ color: 'var(--c-text-3)' }}>
        删除任务或移除附件都<strong>不会删除你的原文件</strong>；「复制到附件目录」模式下的
        副本位于 Lumen 数据目录内，清理它同样不影响原文件。
      </p>

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
