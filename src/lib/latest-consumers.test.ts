import { describe, expect, it } from 'vitest'
import { createRequestGate, runLatestRequest } from './request-gate'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

describe.each([
  'Calendar', 'Stats', 'Organize', 'Dependency search', 'Floating', 'Focus',
])('%s latest-wins consumer', () => {
  it('A 开始、B 开始、B 返回、A 返回后最终保持 B', async () => {
    const gate = createRequestGate()
    const a = deferred<string>()
    const b = deferred<string>()
    let state = 'initial'
    const old = runLatestRequest(gate, () => a.promise, { apply: (value) => { state = value } })
    const newest = runLatestRequest(gate, () => b.promise, { apply: (value) => { state = value } })
    b.resolve('B')
    await newest
    expect(state).toBe('B')
    a.resolve('A')
    await old
    expect(state).toBe('B')
  })
})
