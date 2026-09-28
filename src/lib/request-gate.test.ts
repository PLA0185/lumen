import { describe, expect, it } from 'vitest'
import { createRequestGate } from './request-gate'

describe('latest request wins', () => {
  it('discards an older response even when it finishes last', () => {
    const gate = createRequestGate()
    const old = gate.begin()
    const newest = gate.begin()
    expect(gate.isCurrent(newest)).toBe(true)
    expect(gate.isCurrent(old)).toBe(false)
    gate.invalidate()
    expect(gate.isCurrent(newest)).toBe(false)
    gate.dispose()
    expect(gate.isCurrent(gate.begin())).toBe(false)
  })

  it('accepts responses after effect remount without accepting the old mount', () => {
    const gate = createRequestGate()
    const old = gate.begin()
    gate.dispose()
    gate.activate()
    const current = gate.begin()
    expect(gate.isCurrent(old)).toBe(false)
    expect(gate.isCurrent(current)).toBe(true)
    gate.dispose()
    expect(gate.isCurrent(current)).toBe(false)
  })
})
