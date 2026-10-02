import type { FlowStep } from './memos-ipc'
export const NODE_WIDTH = 420, NODE_HEIGHT = 148, NODE_GAP = 96
export const MIN_WIDTH = 280, MAX_WIDTH = 2400, MAX_HEIGHT = 20000, MAX_POSITION = 1_000_000
export type NodeRect = { id: string; x: number; y: number; width: number; height: number }
export const groupKey = (step?: FlowStep) => step?.group ? JSON.stringify([step.group.id, ...step.group.path]) : ''
export function flowLayout(steps: FlowStep[], heights: Record<string, number>, direction: string) {
  let offset = 0
  const nodes: NodeRect[] = steps.map((s, i) => {
    const width = s.layout?.width ?? NODE_WIDTH, height = Math.max(NODE_HEIGHT, s.layout?.minHeight ?? 0, heights[s.id] ?? 0)
    const starts = !!s.group && groupKey(s) !== groupKey(steps[i - 1])
    if (direction === 'horizontal' && starts && i > 0) offset += 64
    const y = direction === 'horizontal' ? (s.group ? 64 : 0) : offset + (starts ? 64 : 0)
    const node = { id: s.id, width, height, x: s.layout?.x ?? (direction === 'horizontal' ? offset : 0), y: s.layout?.y ?? y }
    offset = (direction === 'horizontal' ? offset + width : y + height) + NODE_GAP
    return node
  })
  const headers = steps.flatMap((s, i) => s.group && groupKey(s) !== groupKey(steps[i - 1]) ? [{ ...nodes[i]!, y: nodes[i]!.y - 64, height: 56, group: s.group, index: i }] : [])
  const left = nodes.length ? Math.min(...nodes.map(n => n.x)) : 0, top = nodes.length ? Math.min(...nodes.map(n => n.y), ...headers.map(n => n.y)) : 0
  const right = nodes.length ? Math.max(...nodes.map(n => n.x + n.width)) : NODE_WIDTH, bottom = nodes.length ? Math.max(...nodes.map(n => n.y + n.height)) : NODE_HEIGHT
  return { nodes, headers, bounds: { left, top, width: right - left, height: bottom - top } }
}
export function autoArrange(steps: FlowStep[]): FlowStep[] {
  return steps.map(s => s.layout ? { ...s, layout: { width: s.layout.width, minHeight: s.layout.minHeight } } : s)
}
export function edgePath(a: NodeRect, b: NodeRect, direction: string): string {
  if (direction === 'horizontal' && b.x >= a.x + a.width) {
    const from = { x: a.x + a.width, y: a.y + 40 }, to = { x: b.x, y: b.y + 40 }, middle = (from.x + to.x) / 2
    return `M ${from.x} ${from.y} C ${middle} ${from.y} ${middle} ${to.y} ${to.x} ${to.y}`
  }
  const down = b.y >= a.y
  const from = { x: a.x + a.width / 2, y: a.y + (down ? a.height : 0) }, to = { x: b.x + b.width / 2, y: b.y + (down ? 0 : b.height) }, middle = (from.y + to.y) / 2
  return `M ${from.x} ${from.y} C ${from.x} ${middle} ${to.x} ${middle} ${to.x} ${to.y}`
}
