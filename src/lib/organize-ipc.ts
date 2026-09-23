/**
 * 组织类（项目 / 分类 / 标签）与子任务、依赖的 IPC 封装。
 *
 * 与 `ipc.ts` 同样的约定：命令名集中定义、错误规范化为 IpcError。
 * 单独成文件是因为这部分命令数量较多，混在一起会让任务命令难以查找。
 */

import { invoke } from '@tauri-apps/api/core'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

// =============================================================================
// 类型
// =============================================================================

/** 项目 */
export interface Project {
  id: string
  name: string
  description: string
  color: string | null
  icon: string | null
  sortOrder: number
  isFavorite: number
  isArchived: number
  archivedAt: string | null
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** 带计数的项目 */
export interface ProjectWithCount extends Project {
  /** 未完成任务数 */
  openCount: number
  /** 全部未删除任务数 */
  totalCount: number
}

/** 分类 */
export interface Category {
  id: string
  name: string
  description: string
  color: string | null
  icon: string | null
  sortOrder: number
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** 标签 */
export interface Tag {
  id: string
  name: string
  color: string | null
  sortOrder: number
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

/** 带计数的标签 */
export interface TagWithCount extends Tag {
  taskCount: number
}

/** 子任务 */
export interface Subtask {
  id: string
  taskId: string
  title: string
  isDone: number
  sortOrder: number
  completedAt: string | null
  createdAt: string
  updatedAt: string
}

/** 子任务进度 */
export interface SubtaskProgress {
  taskId: string
  total: number
  done: number
  /** 无子任务时为 null —— 与"完成 0%"是不同含义 */
  percent: number | null
}

/** 依赖项 */
export interface DependencyItem {
  dependsOnId: string
  title: string
  status: string
  dueAt: string | null
  isDone: boolean
}

/** 删除时对关联任务的处理策略（§4.2 要求明确说明） */
export type OrphanStrategy = 'detach' | 'cascade_soft_delete'

/** 删除影响预览 */
export interface DeleteImpact {
  affectedTasks: number
  completedTasks: number
  relatedRecords: number
}

// =============================================================================
// 命令名
// =============================================================================

export const ORG_CMD = {
  projectList: 'project_list',
  projectCreate: 'project_create',
  projectUpdate: 'project_update',
  projectSetArchived: 'project_set_archived',
  projectDeleteImpact: 'project_delete_impact',
  projectDelete: 'project_delete',
  projectMerge: 'project_merge',

  categoryList: 'category_list',
  categoryCreate: 'category_create',
  categoryUpdate: 'category_update',
  categoryDeleteImpact: 'category_delete_impact',
  categoryDelete: 'category_delete',

  tagList: 'tag_list',
  tagCreate: 'tag_create',
  tagUpdate: 'tag_update',
  tagDelete: 'tag_delete',
  tagMerge: 'tag_merge',
  taskTagsGet: 'task_tags_get',
  taskTagsSet: 'task_tags_set',

  subtaskCreate: 'subtask_create',
  subtaskList: 'subtask_list',
  subtaskUpdate: 'subtask_update',
  subtaskDelete: 'subtask_delete',
  subtaskProgress: 'subtask_progress',
  subtaskProgressBatch: 'subtask_progress_batch',

  dependencyAdd: 'dependency_add',
  dependencyList: 'dependency_list',
  dependencyDependents: 'dependency_dependents',
  dependencyRemove: 'dependency_remove',
  dependencyIsBlocked: 'dependency_is_blocked',
} as const

/** 统一调用封装（与 ipc.ts 的 call 行为一致） */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    throw new IpcError(
      'internal',
      '当前不在 Lumen 桌面程序内运行',
      '请通过桌面应用打开，而不是在浏览器中访问页面',
      null,
    )
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'message' in e && 'code' in e) {
      const err = e as { code: string; message: string; hint: string | null }
      throw new IpcError(err.code as ErrorCode, err.message, err.hint ?? null, e)
    }
    if (typeof e === 'string') throw new IpcError('internal', e, null, e)
    throw new IpcError('internal', '发生了未预期的错误', '请查看日志目录获取详细信息', e)
  }
}

// =============================================================================
// 项目
// =============================================================================

export const projectList = (includeArchived = false): Promise<ProjectWithCount[]> =>
  call(ORG_CMD.projectList, { includeArchived })

export const projectCreate = (input: {
  name: string
  description?: string
  color?: string | null
  icon?: string | null
}): Promise<Project> => call(ORG_CMD.projectCreate, { input })

export const projectUpdate = (
  id: string,
  input: Partial<{
    name: string
    description: string
    color: string | null
    icon: string | null
    sortOrder: number
    isFavorite: boolean
    isArchived: boolean
  }>,
): Promise<Project> => call(ORG_CMD.projectUpdate, { id, input })

export const projectSetArchived = (id: string, archived: boolean): Promise<Project> =>
  call(ORG_CMD.projectSetArchived, { id, archived })

export const projectDeleteImpact = (id: string): Promise<DeleteImpact> =>
  call(ORG_CMD.projectDeleteImpact, { id })

export const projectDelete = (id: string, strategy: OrphanStrategy): Promise<number> =>
  call(ORG_CMD.projectDelete, { id, strategy })

export const projectMerge = (sourceIds: string[], targetId: string): Promise<number> =>
  call(ORG_CMD.projectMerge, { input: { sourceIds, targetId } })

// =============================================================================
// 分类
// =============================================================================

export const categoryList = (): Promise<Category[]> => call(ORG_CMD.categoryList)

export const categoryCreate = (input: {
  name: string
  description?: string
  color?: string | null
  icon?: string | null
}): Promise<Category> => call(ORG_CMD.categoryCreate, { input })

export const categoryUpdate = (
  id: string,
  input: Partial<{
    name: string
    description: string
    color: string | null
    icon: string | null
    sortOrder: number
  }>,
): Promise<Category> => call(ORG_CMD.categoryUpdate, { id, input })

export const categoryDeleteImpact = (id: string): Promise<DeleteImpact> =>
  call(ORG_CMD.categoryDeleteImpact, { id })

export const categoryDelete = (id: string, strategy: OrphanStrategy): Promise<number> =>
  call(ORG_CMD.categoryDelete, { id, strategy })

// =============================================================================
// 标签
// =============================================================================

export const tagList = (): Promise<TagWithCount[]> => call(ORG_CMD.tagList)

export const tagCreate = (input: { name: string; color?: string | null }): Promise<Tag> =>
  call(ORG_CMD.tagCreate, { input })

export const tagUpdate = (
  id: string,
  input: Partial<{ name: string; color: string | null; sortOrder: number }>,
): Promise<Tag> => call(ORG_CMD.tagUpdate, { id, input })

export const tagDelete = (id: string): Promise<number> => call(ORG_CMD.tagDelete, { id })

export const tagMerge = (sourceIds: string[], targetId: string): Promise<number> =>
  call(ORG_CMD.tagMerge, { input: { sourceIds, targetId } })

export const taskTagsGet = (taskId: string): Promise<Tag[]> =>
  call(ORG_CMD.taskTagsGet, { taskId })

export const taskTagsSet = (taskId: string, tagIds: string[]): Promise<number> =>
  call(ORG_CMD.taskTagsSet, { taskId, tagIds })

// =============================================================================
// 子任务
// =============================================================================

export const subtaskCreate = (taskId: string, title: string): Promise<Subtask> =>
  call(ORG_CMD.subtaskCreate, { taskId, title })

export const subtaskList = (taskId: string): Promise<Subtask[]> =>
  call(ORG_CMD.subtaskList, { taskId })

export const subtaskUpdate = (
  id: string,
  patch: { title?: string; isDone?: boolean; sortOrder?: number },
): Promise<Subtask> => call(ORG_CMD.subtaskUpdate, { id, ...patch })

export const subtaskDelete = (id: string): Promise<number> =>
  call(ORG_CMD.subtaskDelete, { id })

export const subtaskProgress = (taskId: string): Promise<SubtaskProgress> =>
  call(ORG_CMD.subtaskProgress, { taskId })

/** 批量取进度，避免列表页 N+1 查询（§10 上千条任务仍要流畅） */
export const subtaskProgressBatch = (taskIds: string[]): Promise<SubtaskProgress[]> =>
  call(ORG_CMD.subtaskProgressBatch, { taskIds })

// =============================================================================
// 依赖
// =============================================================================

export const dependencyAdd = (taskId: string, dependsOnId: string): Promise<number> =>
  call(ORG_CMD.dependencyAdd, { taskId, dependsOnId })

export const dependencyList = (taskId: string): Promise<DependencyItem[]> =>
  call(ORG_CMD.dependencyList, { taskId })

export const dependencyDependents = (taskId: string): Promise<DependencyItem[]> =>
  call(ORG_CMD.dependencyDependents, { taskId })

export const dependencyRemove = (taskId: string, dependsOnId: string): Promise<number> =>
  call(ORG_CMD.dependencyRemove, { taskId, dependsOnId })

export const dependencyIsBlocked = (taskId: string): Promise<boolean> =>
  call(ORG_CMD.dependencyIsBlocked, { taskId })
