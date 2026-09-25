import type { Category, ProjectWithCount, Tag, TagWithCount } from './organize-ipc'

export interface TaskEditorOptionSources {
  projects: () => Promise<ProjectWithCount[]>
  categories: () => Promise<Category[]>
  tags: () => Promise<TagWithCount[]>
  selectedTags: () => Promise<Tag[]>
}

export interface TaskEditorOptionResult {
  projects: PromiseSettledResult<ProjectWithCount[]>
  categories: PromiseSettledResult<Category[]>
  tags: PromiseSettledResult<TagWithCount[]>
  selectedTags: PromiseSettledResult<Tag[]>
}

/** Each auxiliary source settles independently so one failure cannot lock task saving. */
export async function loadTaskEditorOptions(
  sources: TaskEditorOptionSources,
): Promise<TaskEditorOptionResult> {
  const [projects, categories, tags, selectedTags] = await Promise.allSettled([
    sources.projects(), sources.categories(), sources.tags(), sources.selectedTags(),
  ])
  return { projects, categories, tags, selectedTags }
}

/**
 * Keep core field saving available when tag metadata cannot be loaded.
 * The field-only path deliberately leaves the task's existing tags untouched.
 */
export async function saveTaskWithOptionalTags<T>(options: {
  tagsReady: boolean
  saveFields: () => Promise<T>
  saveFieldsAndTags: () => Promise<T>
}): Promise<T> {
  return options.tagsReady ? options.saveFieldsAndTags() : options.saveFields()
}
