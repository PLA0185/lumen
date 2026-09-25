/** Reject responses from obsolete requests or unmounted consumers. */
export function createRequestGate() {
  let generation = 0
  let disposed = false
  return {
    begin(): number {
      return ++generation
    },
    isCurrent(token: number): boolean {
      return !disposed && token === generation
    },
    invalidate(): void {
      generation += 1
    },
    dispose(): void {
      disposed = true
      generation += 1
    },
  }
}

export type RequestGate = ReturnType<typeof createRequestGate>

/** Shared consumer runner: only the newest request may mutate UI state. */
export async function runLatestRequest<T>(
  gate: RequestGate,
  request: () => Promise<T>,
  callbacks: {
    apply: (value: T) => void
    reject?: (error: unknown) => void
    finish?: () => void
  },
): Promise<boolean> {
  const token = gate.begin()
  try {
    const value = await request()
    if (!gate.isCurrent(token)) return false
    callbacks.apply(value)
    return true
  } catch (error) {
    if (gate.isCurrent(token)) callbacks.reject?.(error)
    return false
  } finally {
    if (gate.isCurrent(token)) callbacks.finish?.()
  }
}
