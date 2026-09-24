import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { MUTATION_DOMAINS, invokeData, onDataChanged } from './data-change'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn(), listen: vi.fn() }))

const root = process.cwd()

describe('mutation architecture', () => {
  it('registers every backend mutation domain', () => {
    for (const cmd of [
      'task_create', 'task_update', 'task_toggle_done', 'task_soft_delete',
      'task_restore', 'task_purge', 'task_commit_purge_deleted', 'task_bulk',
      'task_duplicate', 'task_reorder', 'task_reschedule', 'recurring_create',
      'recurring_materialize', 'recurring_edit_instance', 'recurring_skip_occurrence',
      'recurring_delete', 'ai_apply', 'project_create', 'project_update',
      'project_set_archived', 'project_delete', 'project_merge', 'category_create',
      'category_update', 'category_delete', 'category_merge', 'tag_create',
      'tag_update', 'tag_delete', 'tag_merge', 'task_tags_set', 'subtask_create',
      'subtask_update', 'subtask_delete', 'dependency_add', 'dependency_remove',
      'attachment_add', 'attachment_remove', 'reminder_create',
      'reminder_set_enabled', 'reminder_delete', 'reminder_snooze', 'focus_end',
      'backup_restore',
    ]) {
      expect(MUTATION_DOMAINS, cmd).toHaveProperty(cmd)
    }
    expect(MUTATION_DOMAINS.focus_end).toContain('tasks')
    expect(MUTATION_DOMAINS.backup_restore).toContain('all')
    expect(MUTATION_DOMAINS.recurring_edit_instance).toContain('organization')
    expect(MUTATION_DOMAINS.ai_apply).toContain('reminders')
    expect(MUTATION_DOMAINS.ai_apply).toContain('subtasks')
  })

  it('keeps change publication out of components and the store', () => {
    const paths = readdirSync(join(root, 'src/components'))
      .filter((name) => name.endsWith('.tsx'))
      .map((name) => join(root, 'src/components', name))
    paths.push(join(root, 'src/lib/store.ts'))
    for (const path of paths) {
      const source = readFileSync(path, 'utf8')
      expect(source, path).not.toMatch(/\bnotifyTasksChanged\s*\(/)
      expect(source, path).not.toMatch(/\bpublishDataChange\s*\(/)
    }
  })

  it('publishes after a committed mutation, never after a read or rejection', async () => {
    const listener = vi.fn()
    const off = onDataChanged(['tasks'], listener)
    const mocked = vi.mocked(invoke)
    try {
      mocked.mockResolvedValueOnce({ id: 'task-1' })
      await invokeData('task_create', { input: { title: '测试' } })
      expect(listener).toHaveBeenCalledTimes(1)

      mocked.mockResolvedValueOnce([])
      await invokeData('task_list', { query: {} })
      expect(listener).toHaveBeenCalledTimes(1)

      mocked.mockRejectedValueOnce(new Error('database rolled back'))
      await expect(invokeData('task_save', { id: 'task-1' })).rejects.toThrow()
      expect(listener).toHaveBeenCalledTimes(1)

      mocked.mockResolvedValueOnce(0)
      await invokeData('recurring_ensure_range', {})
      expect(listener).toHaveBeenCalledTimes(1)
    } finally {
      off()
      mocked.mockReset()
    }
  })
})
