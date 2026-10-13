import type { SyncOptions } from '../lib/cloud-sync-ipc'

export function isMissingAttachmentSyncError(error: string): boolean {
  return error.includes('原文件无法读取')
    || error.includes('云端任务附件缺失')
    || error.includes('附件缺少历史副本')
}

export function MissingAttachmentSyncFallback({
  scope,
  busy,
  onSkip,
}: {
  scope: SyncOptions['scope']
  busy: boolean
  onSkip: () => void
}) {
  if (scope === 'memos') return null
  return <div>
    <p className="setgroup__hint">你可以本次跳过全部任务及其附件，只同步备忘与流程。此操作不会删除本机任务、附件或原文件，也不会修改默认同步范围；之后的自动同步仍按原范围运行。若希望长期不传任务，可将默认范围改为“仅备忘与流程”；恢复附件后也可再次完整同步。</p>
    <button type="button" className="btn btn--sm" disabled={busy} onClick={onSkip}>本次跳过任务及附件</button>
  </div>
}
