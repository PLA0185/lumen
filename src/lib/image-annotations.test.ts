import { expect, it } from 'vitest'
import { arrowHead } from './image-annotations'
it('短箭头头部不越过起点，大小受箭头长度约束', () => {
  const a = { x: 10, y: 20 }, b = { x: 15, y: 20 }, head = arrowHead(a, b, 4)
  expect(head[0]).toEqual(b)
  expect(head.every(p => p.x >= a.x && p.x <= b.x)).toBe(true)
  expect(Math.max(...head.map(p => p.y)) - Math.min(...head.map(p => p.y))).toBeLessThan(5)
})
