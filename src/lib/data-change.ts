import { invoke } from '@tauri-apps/api/core'
import { emit, listen } from '@tauri-apps/api/event'

export type DataDomain =
  | 'tasks'
  | 'organization'
  | 'subtasks'
  | 'dependencies'
  | 'attachments'
  | 'reminders'
  | 'recurrence'
  | 'focus'
  | 'stats'
  | 'all'

/** A command has one authoritative invalidation policy, regardless of its caller. */
export const MUTATION_DOMAINS = {
  task_create: ['tasks', 'organization', 'stats'],
  task_update: ['tasks', 'organization', 'reminders', 'stats'],
  task_save: ['tasks', 'organization', 'reminders', 'stats'],
  task_toggle_done: ['tasks', 'organization', 'stats'],
  task_soft_delete: ['tasks', 'organization', 'stats'],
  task_restore: ['tasks', 'organization', 'stats'],
  task_purge: ['tasks', 'organization', 'stats'],
  task_commit_purge_deleted: ['tasks', 'organization', 'stats'],
  task_bulk: ['tasks', 'organization', 'stats'],
  task_duplicate: ['tasks', 'organization', 'subtasks', 'reminders', 'stats'],
  task_reorder: ['tasks'],
  task_reschedule: ['tasks', 'reminders', 'stats'],
  recurring_create: ['tasks', 'recurrence', 'organization', 'stats'],
  recurring_materialize: ['tasks', 'recurrence', 'organization', 'stats'],
  recurring_ensure_range: ['tasks', 'recurrence', 'organization', 'stats'],
  recurring_edit_instance: ['tasks', 'recurrence', 'organization', 'reminders', 'stats'],
  recurring_skip_occurrence: ['tasks', 'recurrence', 'organization', 'stats'],
  recurring_delete: ['tasks', 'recurrence', 'organization', 'stats'],
  ai_apply: ['tasks', 'organization', 'subtasks', 'recurrence', 'reminders', 'stats'],
  project_create: ['organization'],
  project_update: ['tasks', 'organization'],
  project_set_archived: ['organization'],
  project_delete: ['tasks', 'organization', 'stats'],
  project_merge: ['tasks', 'organization', 'stats'],
  category_create: ['organization'],
  category_update: ['organization'],
  category_delete: ['tasks', 'organization', 'stats'],
  category_merge: ['tasks', 'organization', 'stats'],
  tag_create: ['organization'],
  tag_update: ['organization'],
  tag_delete: ['tasks', 'organization'],
  tag_merge: ['tasks', 'organization'],
  task_tags_set: ['tasks', 'organization'],
  subtask_create: ['tasks', 'subtasks', 'stats'],
  subtask_update: ['tasks', 'subtasks', 'stats'],
  subtask_delete: ['tasks', 'subtasks', 'stats'],
  dependency_add: ['dependencies', 'tasks'],
  dependency_remove: ['dependencies', 'tasks'],
  attachment_add: ['attachments'],
  attachment_remove: ['attachments'],
  attachment_cleanup_orphans: ['attachments'],
  reminder_create: ['reminders'],
  reminder_set_enabled: ['reminders'],
  reminder_delete: ['reminders'],
  reminder_snooze: ['reminders'],
  reminder_set_grace: ['reminders'],
  focus_start: ['focus'],
  focus_pause: ['focus'],
  focus_resume: ['focus'],
  focus_end: ['focus', 'tasks', 'stats'],
  focus_cancel: ['focus'],
  growth_set_config: ['stats'],
  goal_create: ['stats'],
  goal_delete: ['stats'],
  backup_restore: ['all'],
} as const satisfies Record<string, readonly DataDomain[]>

export type MutatingCommand = keyof typeof MUTATION_DOMAINS
type Change = { from: string; domains: readonly DataDomain[] }
type Listener = { domains: readonly DataDomain[]; callback: () => void }

const EVENT = 'lumen-data-changed'
const source = crypto.randomUUID()
const listeners = new Set<Listener>()
let remoteUnlisten: (() => void) | undefined
let remoteStarting = false

function matches(watched: readonly DataDomain[], changed: readonly DataDomain[]): boolean {
  return watched.includes('all') || changed.includes('all') || watched.some((d) => changed.includes(d))
}

function dispatch(domains: readonly DataDomain[]): void {
  for (const listener of listeners) {
    if (matches(listener.domains, domains)) listener.callback()
  }
}

function startRemoteListener(): void {
  if (remoteUnlisten || remoteStarting || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  remoteStarting = true
  void listen<Change>(EVENT, ({ payload }) => {
    if (payload?.from !== source && Array.isArray(payload?.domains)) dispatch(payload.domains)
  }).then((off) => {
    remoteUnlisten = off
    if (listeners.size === 0) {
      remoteUnlisten()
      remoteUnlisten = undefined
    }
  }).catch(() => {
    // Local invalidation remains available if the event bridge is unavailable.
  }).finally(() => { remoteStarting = false })
}

export function onDataChanged(domains: readonly DataDomain[], callback: () => void): () => void {
  const listener = { domains, callback }
  listeners.add(listener)
  startRemoteListener()
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) {
      remoteUnlisten?.()
      remoteUnlisten = undefined
    }
  }
}

/** Publish only after the backend has committed a successful mutation. */
export function publishDataChange(domains: readonly DataDomain[]): void {
  dispatch(domains)
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  void emit(EVENT, { from: source, domains } satisfies Change).catch(() => {
    // Cross-window refresh is best effort; the committed mutation still succeeds.
  })
}

export async function mutate<T>(cmd: MutatingCommand, args?: Record<string, unknown>): Promise<T> {
  const result = await invoke<T>(cmd, args)
  if (!((cmd === 'recurring_materialize' || cmd === 'recurring_ensure_range') && result === 0)) {
    publishDataChange(MUTATION_DOMAINS[cmd])
  }
  return result
}

/** Shared entry used by all IPC wrappers. Reads do not publish changes. */
export function invokeData<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (Object.hasOwn(MUTATION_DOMAINS, cmd)) return mutate<T>(cmd as MutatingCommand, args)
  return invoke<T>(cmd, args)
}
