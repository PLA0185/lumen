import { listen } from '@tauri-apps/api/event'
import { invokeData } from './data-change'

export type MealType = 'breakfast' | 'lunch' | 'dinner' | 'snack' | 'training'
export type EntryState = 'planned' | 'eaten'

export interface NutritionEntry {
  id: string; logDate: string; mealType: MealType; name: string; amount: number; unit: 'g' | '份' | 'kcal'
  kcalPerUnit: number; calories: number; state: EntryState; sourceTitle: string | null; sourceUrl: string | null; recipeId: string | null
}
export interface FoodCandidate {
  id: string; name: string; kcalPer100g: number; sourceTitle: string; sourceUrl: string; checkedAt: string
}
export interface RecipeIngredient {
  id: string; foodName: string; amountGrams: number; kcalPer100g: number; calories: number
  sourceTitle: string; sourceUrl: string; checkedAt: string
}
export interface Recipe {
  id: string; title: string; servings: number; instructions: string; calories: number; caloriesPerServing: number
  ingredients: RecipeIngredient[]; createdAt: string; updatedAt: string
}
export interface ShoppingItem {
  id: string; name: string; quantity: number; unit: string; category: string; notes: string
  completed: boolean; source: 'manual' | 'import' | 'recipe'; sortOrder: number
}
export interface NutritionTotals {
  baseKcal: number; snackKcal: number; trainingKcal: number; netKcal: number; remainingKcal: number | null
}
export interface NutritionDashboard {
  date: string; targetKcal: number | null; totals: NutritionTotals
  recipes: Recipe[]; entries: NutritionEntry[]; shoppingItems: ShoppingItem[]
}
export interface ShoppingDraft { name: string; quantity: number; unit: string; category: string; notes: string }
export interface ShoppingImportPreview { fileName: string; drafts: ShoppingDraft[]; warnings: string[] }
export interface NutritionImportProgress {
  requestId: string; fileName: string; phase: string; message: string; current: number | null; total: number | null
}

export const nutritionDashboard = (date: string) => invokeData<NutritionDashboard>('nutrition_dashboard', { date })
export const nutritionSetDailyTarget = (targetKcal: number | null) =>
  invokeData<NutritionDashboard>('nutrition_set_daily_target', { targetKcal })
export const nutritionLookupFood = (query: string) => invokeData<FoodCandidate[]>('nutrition_lookup_food', { query })
export const nutritionEntryCreateFood = (date: string, mealType: MealType, candidateId: string, amountGrams: number, status: EntryState) =>
  invokeData<NutritionDashboard>('nutrition_entry_create_food', { date, mealType, candidateId, amountGrams, status })
export const nutritionEntryCreateTraining = (date: string, name: string, calories: number) =>
  invokeData<NutritionDashboard>('nutrition_entry_create_training', { date, name, calories })
export const nutritionEntryReplaceFood = (id: string, candidateId: string, amountGrams: number, mealType: Exclude<MealType, 'training'>, status: EntryState) =>
  invokeData<boolean>('nutrition_entry_replace_food', { id, candidateId, amountGrams, mealType, status })
export const nutritionEntryUpdateAmount = (id: string, amount: number) =>
  invokeData<boolean>('nutrition_entry_update_amount', { id, amount })
export const nutritionEntrySetState = (id: string, status: EntryState) =>
  invokeData<boolean>('nutrition_entry_set_state', { id, status })
export const nutritionEntryDelete = (id: string) => invokeData<boolean>('nutrition_entry_delete', { id })
export const nutritionRecipeCreate = (draft: { title: string; servings: number; instructions: string; ingredients: { candidateId: string; amountGrams: number }[] }) =>
  invokeData<Recipe>('nutrition_recipe_create', { draft })
export const nutritionRecipeUpdate = (id: string, title: string, servings: number, instructions: string) =>
  invokeData<boolean>('nutrition_recipe_update', { id, title, servings, instructions })
export const nutritionRecipeDelete = (id: string) => invokeData<boolean>('nutrition_recipe_delete', { id })
export const nutritionRecipeAddIngredient = (recipeId: string, candidateId: string, amountGrams: number) =>
  invokeData<Recipe>('nutrition_recipe_add_ingredient', { recipeId, candidateId, amountGrams })
export const nutritionRecipeRemoveIngredient = (recipeId: string, ingredientId: string) =>
  invokeData<boolean>('nutrition_recipe_remove_ingredient', { recipeId, ingredientId })
export const nutritionRecipePlan = (date: string, recipeId: string, mealType: MealType, servings: number) =>
  invokeData<NutritionDashboard>('nutrition_recipe_plan', { date, recipeId, mealType, servings })
export const nutritionShoppingCreate = (draft: ShoppingDraft) => invokeData<ShoppingItem>('nutrition_shopping_create', { draft })
export const nutritionShoppingUpdate = (id: string, draft: ShoppingDraft) => invokeData<boolean>('nutrition_shopping_update', { id, draft })
export const nutritionShoppingSetCompleted = (id: string, completed: boolean) =>
  invokeData<boolean>('nutrition_shopping_set_completed', { id, completed })
export const nutritionShoppingDelete = (id: string) => invokeData<boolean>('nutrition_shopping_delete', { id })
export const nutritionShoppingImportCommit = (drafts: ShoppingDraft[]) =>
  invokeData<ShoppingItem[]>('nutrition_shopping_import_commit', { drafts })
export const nutritionShoppingAddRecipe = (recipeId: string) =>
  invokeData<ShoppingItem[]>('nutrition_shopping_add_recipe', { recipeId })
export const nutritionSearchKeyStatus = () => invokeData<boolean>('nutrition_search_key_status')
export const nutritionSetSearchKey = (key: string) => invokeData<boolean>('nutrition_set_search_key', { key })

export async function nutritionShoppingImportPreview(
  file: File,
  mode: 'local' | 'ai',
  onProgress?: (progress: Omit<NutritionImportProgress, 'requestId'>) => void,
): Promise<ShoppingImportPreview> {
  if (file.size > 20 * 1024 * 1024) throw new Error('单个文件最多 20 MiB，请分拆后导入')
  if (!file.size) throw new Error(`「${file.name}」是空文件`)
  const requestId = crypto.randomUUID()
  let unlisten: (() => void) | undefined
  if (onProgress) {
    try {
      unlisten = await listen<NutritionImportProgress>('nutrition-import-progress', ({ payload }) => {
        if (payload.requestId === requestId && payload.fileName === file.name) {
          const { requestId: _id, ...progress } = payload
          onProgress(progress)
        }
      })
    } catch {
      onProgress({ fileName: file.name, phase: 'extracting', message: '实时进度连接不可用，文件仍会继续解析。', current: null, total: null })
    }
  }
  try {
    const dataBase64 = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader()
      reader.onerror = () => reject(new Error(`读取「${file.name}」失败，请重新选择`))
      reader.onload = () => resolve(String(reader.result).split(',')[1] ?? '')
      reader.readAsDataURL(file)
    })
    return await invokeData<ShoppingImportPreview>('nutrition_shopping_import_preview', {
      name: file.name, dataBase64, mode, requestId,
    })
  } finally {
    unlisten?.()
  }
}
