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
