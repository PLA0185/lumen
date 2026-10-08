export type NutritionMealType = 'breakfast' | 'lunch' | 'dinner' | 'snack' | 'training'
export type NutritionEntryState = 'planned' | 'eaten'

export interface NutritionSummaryEntry {
  mealType: NutritionMealType
  calories: number
  state: NutritionEntryState
}

export interface NutritionSummary {
  baseKcal: number
  snackKcal: number
  trainingKcal: number
  netKcal: number
  remainingKcal: number | null
  targetKcal: number | null
}

export function summarizeNutrition(
  entries: readonly NutritionSummaryEntry[],
  targetKcal: number | null,
): NutritionSummary {
  let baseKcal = 0
  let snackKcal = 0
  let trainingKcal = 0

  for (const entry of entries) {
    if (entry.state !== 'eaten') continue
    if (entry.mealType === 'training') trainingKcal += entry.calories
    else if (entry.mealType === 'snack') snackKcal += entry.calories
    else baseKcal += entry.calories
  }

  const netKcal = baseKcal + snackKcal - trainingKcal
  return {
    baseKcal,
    snackKcal,
    trainingKcal,
    netKcal,
    remainingKcal: targetKcal === null ? null : targetKcal - netKcal,
    targetKcal,
  }
}
