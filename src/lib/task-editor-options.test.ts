import { describe, expect, it, vi } from 'vitest'
import { loadTaskEditorOptions, saveTaskWithOptionalTags } from './task-editor-options'

describe('TaskEditor 辅助数据独立降级', () => {
  it('标签失败时仍返回项目和分类，不把基本编辑整体判成失败', async () => {
    const result = await loadTaskEditorOptions({
      projects: vi.fn().mockResolvedValue([{ id: 'p1', name: '工作' }]),
      categories: vi.fn().mockResolvedValue([{ id: 'c1', name: '计划' }]),
      tags: vi.fn().mockRejectedValue(new Error('tag database unavailable')),
      selectedTags: vi.fn().mockResolvedValue([{ id: 't1', name: '已有标签' }]),
    })
    expect(result.projects.status).toBe('fulfilled')
    expect(result.categories.status).toBe('fulfilled')
    expect(result.tags.status).toBe('rejected')
    expect(result.selectedTags.status).toBe('fulfilled')
  })

  it('项目失败不影响标签编辑所需的两份数据', async () => {
    const result = await loadTaskEditorOptions({
      projects: vi.fn().mockRejectedValue(new Error('project unavailable')),
      categories: vi.fn().mockResolvedValue([]),
      tags: vi.fn().mockResolvedValue([{ id: 't1', name: '标签', taskCount: 1 }]),
      selectedTags: vi.fn().mockResolvedValue([{ id: 't1', name: '标签' }]),
    })
    expect(result.projects.status).toBe('rejected')
    expect(result.tags.status).toBe('fulfilled')
    expect(result.selectedTags.status).toBe('fulfilled')
  })

  it('标签辅助数据失败时仍保存基本字段，并保留已有标签', async () => {
    const saveFields = vi.fn().mockResolvedValue({ title: '已保存' })
    const saveFieldsAndTags = vi.fn().mockResolvedValue({ title: '不应调用' })

    const saved = await saveTaskWithOptionalTags<{ title: string }>({
      tagsReady: false,
      saveFields,
      saveFieldsAndTags,
    })

    expect(saved.title).toBe('已保存')
    expect(saveFields).toHaveBeenCalledOnce()
    expect(saveFieldsAndTags).not.toHaveBeenCalled()
  })
})
