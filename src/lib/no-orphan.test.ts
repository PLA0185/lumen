import { expect, it } from 'vitest'
import { protectedText } from './no-orphan'
it('中文旁长英文编号允许换行，不把整段锁成超宽单元且复制原文不变', () => {
  const text = `第${'Reference'.repeat(12)}章`
  const pieces = protectedText(text)
  expect(pieces.map(p => p.text).join('')).toBe(text)
  expect(pieces.filter(p => p.protect).every(p => [...p.text].length <= 4)).toBe(true)
  expect(pieces.filter(p => /\p{Script=Han}/u.test(p.text)).every(p => [...p.text].filter(c => /[\p{Letter}\p{Number}]/u.test(c)).length >= 2)).toBe(true)
})
