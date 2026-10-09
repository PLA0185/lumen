// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import * as api from '../lib/nutrition-ipc'
import { NutritionView } from './NutritionView'

vi.mock('../lib/nutrition-ipc', () => ({
  nutritionDashboard: vi.fn(), nutritionSetDailyTarget: vi.fn(), nutritionLookupFood: vi.fn(),
  nutritionEntryCreateFood: vi.fn(), nutritionEntryCreateTraining: vi.fn(),
  nutritionEntryUpdateAmount: vi.fn(), nutritionEntryReplaceFood: vi.fn(), nutritionEntrySetState: vi.fn(), nutritionEntryDelete: vi.fn(),
  nutritionRecipeCreate: vi.fn(), nutritionRecipeUpdate: vi.fn(), nutritionRecipeDelete: vi.fn(),
  nutritionRecipeAddIngredient: vi.fn(), nutritionRecipeRemoveIngredient: vi.fn(), nutritionRecipePlan: vi.fn(),
  nutritionShoppingCreate: vi.fn(), nutritionShoppingUpdate: vi.fn(), nutritionShoppingSetCompleted: vi.fn(),
  nutritionShoppingDelete: vi.fn(), nutritionShoppingImportCommit: vi.fn(), nutritionShoppingAddRecipe: vi.fn(),
  nutritionSearchKeyStatus: vi.fn(), nutritionSetSearchKey: vi.fn(), nutritionShoppingImportPreview: vi.fn(),
}))
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true })

const emptyDashboard: api.NutritionDashboard = {
  date: '2026-10-08', targetKcal: 2000,
  totals: { baseKcal: 600, snackKcal: 100, trainingKcal: 250, netKcal: 450, remainingKcal: 1550 },
  recipes: [], entries: [
    { id: 'planned', logDate: '2026-10-08', mealType: 'dinner', name: '计划晚餐', amount: 1, unit: '份', kcalPerUnit: 600, calories: 600, state: 'planned', sourceTitle: null, sourceUrl: null, recipeId: null },
  ], shoppingItems: [],
}
let root: Root | undefined
let host: HTMLDivElement | undefined

async function mount(initial = emptyDashboard) {
  host = document.createElement('div'); document.body.append(host); root = createRoot(host)
  vi.mocked(api.nutritionDashboard).mockResolvedValue(initial)
  vi.mocked(api.nutritionSearchKeyStatus).mockResolvedValue(false)
  await act(async () => root!.render(<NutritionView />))
}

afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  host?.remove(); root = undefined; host = undefined; vi.clearAllMocks()
})

it('makes the calorie accounting and manual daily goal readable', async () => {
  await mount()
  expect(document.body.textContent).toContain('基础饮食')
  expect(document.body.textContent).toContain('加餐')
  expect(document.body.textContent).toContain('训练消耗')
  expect(document.body.textContent).toContain('净摄入')
  expect(document.body.textContent).toContain('1,550')
  expect(document.body.textContent).toContain('计划晚餐')
  expect(document.body.textContent).toContain('计划')
})

it('zero training consumption is displayed as zero without a misleading minus sign', async () => {
  await mount({
    ...emptyDashboard,
    totals: { baseKcal: 0, snackKcal: 0, trainingKcal: 0, netKcal: 0, remainingKcal: 2000 },
    entries: [],
  })
  const training = [...document.querySelectorAll('.nutrition-totals > div')]
    .find(item => item.querySelector('span')?.textContent === '训练消耗')
  expect(training?.querySelector('strong')?.textContent).toBe('0 千卡')
})

it('requires an online food candidate before adding a meal', async () => {
  await mount()
  expect(document.body.textContent).toContain('先联网查询食物热量并选择来源')
  expect(document.body.textContent).toContain('候选热量必须带可打开的来源')
})

it('requires a fresh online candidate before replacing a saved food', async () => {
  const dashboard = {
    ...emptyDashboard,
    entries: [{ id: 'food-1', logDate: '2026-10-08', mealType: 'lunch' as const, name: '旧食物', amount: 100, unit: 'g' as const, kcalPerUnit: 100, calories: 100, state: 'eaten' as const, sourceTitle: '旧来源', sourceUrl: 'https://example.test/old', recipeId: null }],
  }
  vi.mocked(api.nutritionDashboard).mockResolvedValue(dashboard)
  vi.mocked(api.nutritionLookupFood).mockResolvedValue([{ id: 'candidate-2', name: '新食物', kcalPer100g: 210, sourceTitle: '新来源', sourceUrl: 'https://example.test/new', checkedAt: '2026-10-08T12:00:00.000Z' }])
  vi.mocked(api.nutritionEntryReplaceFood).mockResolvedValue(true)
  await mount(dashboard)

  const edit = Array.from(document.querySelectorAll('button')).find(button => button.textContent === '改食物')!
  await act(async () => edit.click())
  expect(document.body.textContent).toContain('原记录在确认保存前保持不变')
  expect(document.querySelector<HTMLInputElement>('input[placeholder^="例如：水煮鸡胸肉"]')?.value).toBe('旧食物')

  const lookup = Array.from(document.querySelectorAll('button')).find(button => button.textContent?.includes('查询热量'))!
  await act(async () => lookup.click())
  expect(document.body.textContent).toContain('新来源')
  const save = Array.from(document.querySelectorAll('button')).find(button => button.textContent === '保存修改')!
  await act(async () => save.click())
  expect(api.nutritionEntryReplaceFood).toHaveBeenCalledWith('food-1', 'candidate-2', 100, 'lunch', 'eaten')
})
it('keeps grocery imports in an editable preview until the user confirms', async () => {
  vi.mocked(api.nutritionShoppingImportPreview).mockResolvedValue({
    fileName: '采购.txt',
    drafts: [{ name: '鸡蛋', quantity: 12, unit: '个', category: '食材', notes: '' }],
    warnings: [],
  })
  await mount()

  const input = document.querySelector<HTMLInputElement>('input[type="file"]')
  expect(input).not.toBeNull()
  Object.defineProperty(input, 'files', {
    configurable: true,
    value: [new File(['鸡蛋 12个'], '采购.txt', { type: 'text/plain' })],
  })
  await act(async () => { input!.dispatchEvent(new Event('change', { bubbles: true })) })

  expect(document.body.textContent).toContain('导入预览 · 1 项')
  expect(document.querySelector<HTMLInputElement>('input[aria-label="采购名称"]')?.value).toBe('鸡蛋')
  expect(api.nutritionShoppingImportCommit).not.toHaveBeenCalled()

  const confirm = Array.from(document.querySelectorAll('button')).find(button => button.textContent?.includes('确认导入已选项目'))
  expect(confirm).toBeDefined()
  await act(async () => { confirm!.click() })

  expect(api.nutritionShoppingImportCommit).toHaveBeenCalledWith([
    { name: '鸡蛋', quantity: 12, unit: '个', category: '食材', notes: '' },
  ])
  expect(document.body.textContent).toContain('已确认导入 1 项采购内容。')
})
