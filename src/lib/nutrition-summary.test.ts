import { describe, expect, it } from 'vitest'
import { summarizeNutrition } from './nutrition-summary'

describe('summarizeNutrition', () => {
  it('excludes planned meals and separates snacks and training', () => {
    expect(summarizeNutrition([
      { mealType: 'breakfast', calories: 320, state: 'eaten' },
      { mealType: 'lunch', calories: 600, state: 'planned' },
      { mealType: 'snack', calories: 180, state: 'eaten' },
      { mealType: 'training', calories: 250, state: 'eaten' },
    ], 1800)).toEqual({ baseKcal: 320, snackKcal: 180, trainingKcal: 250, netKcal: 250, remainingKcal: 1550, targetKcal: 1800 })
  })
})
