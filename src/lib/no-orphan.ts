const han = /\p{Script=Han}/u
type Node = { type: string; value?: string; tagName?: string; properties?: Record<string, unknown>; children?: Node[] }
type Glyph = { text: string; ancestors: Node[] }
/** Two/three Han glyphs share a wrapping unit; punctuation never becomes a residual row. */
function units(text: string): string[] {
  const words = text.match(/\p{Script=Han}+|[\p{Letter}\p{Number}_]+|\s+|[^\p{Script=Han}\p{Letter}\p{Number}_\s]/gu) ?? []
  const result: string[] = []
  for (const word of words) {
    if (han.test([...word][0]!)) {
      const characters = [...word]
      for (let from = 0; from < characters.length;) {
        const count = characters.length - from === 3 ? 3 : Math.min(2, characters.length - from)
        result.push(characters.slice(from, from + count).join('')); from += count
      }
    } else if (/^[，。；：！？、…）》】”’%.,;:!?)]$/u.test(word) && result.length) result[result.length - 1] += word
    else result.push(word)
  }
  for (let i = 0; i < result.length; i++) {
    const word = result[i]!, count = [...word].filter(c => han.test(c)).length
    if (count !== 1 || /[\p{Letter}\p{Number}]/u.test(word.replace(/\p{Script=Han}/gu, ''))) continue
    let next = i + 1
    while (next < result.length && !/[\p{Letter}\p{Number}]/u.test(result[next]!)) next++
    if (next < result.length) { result.splice(i, next - i + 1, result.slice(i, next + 1).join('')); i-- }
    else if (i > 0) {
      let previous = i - 1
      while (previous > 0 && !/[\p{Letter}\p{Number}]/u.test(result[previous]!)) previous--
      result.splice(previous, i - previous + 1, result.slice(previous, i + 1).join('')); i = previous - 1
    }
  }
  // Opening punctuation stays with what follows it.
  for (let i = result.length - 2; i >= 0; i--) if (/^[（(《【“‘]$/u.test(result[i]!)) result.splice(i, 2, result[i]! + result[i + 1]!)
  return result
}
export function protectedText(text: string): Array<{ text: string; protect: boolean }> {
  return units(text).flatMap(value => {
    const long = han.test(value) ? /[A-Za-z0-9_]{9,}/.exec(value) : null
    const pieces = long ? [value.slice(0, long.index) + long[0].slice(0, 2), long[0].slice(2, -2), long[0].slice(-2) + value.slice(long.index + long[0].length)] : [value]
    return pieces.map(part => ({ text: part, protect: han.test(part) && [...part].filter(c => /[\p{Letter}\p{Number}]/u.test(c)).length > 1 }))
  })
}
const inlineTags = new Set(['strong', 'em', 'del', 'a', 'span'])
function isInline(node: Node): boolean {
  // A local link renders an interactive resource, not its label's text fragments.
  if (node.tagName === 'a' && String(node.properties?.href ?? '').startsWith('lumen-asset:')) return false
  return node.type === 'text' || (node.type === 'element' && inlineTags.has(node.tagName ?? '') && (node.children ?? []).every(isInline))
}
function protectInline(nodes: Node[]): Node[] {
  const glyphs: Glyph[] = []
  function flatten(node: Node, ancestors: Node[]) {
    if (node.type === 'text') for (const text of [...(node.value ?? '')]) glyphs.push({ text, ancestors })
    else for (const child of node.children ?? []) flatten(child, [...ancestors, node])
  }
  nodes.forEach(node => flatten(node, []))
  let cursor = 0
  return protectedText(glyphs.map(g => g.text).join('')).flatMap(unit => {
    const group: Node[] = [], chain: Array<{ original: Node; copy: Node }> = []
    for (const glyph of glyphs.slice(cursor, cursor + [...unit.text].length)) {
      let same = 0
      while (same < chain.length && chain[same]!.original === glyph.ancestors[same]) same++
      chain.length = same
      let children = same ? chain[same - 1]!.copy.children! : group
      for (const ancestor of glyph.ancestors.slice(same)) {
        const copy: Node = { type: 'element', tagName: ancestor.tagName, properties: ancestor.properties, children: [] }
        children.push(copy); chain.push({ original: ancestor, copy }); children = copy.children!
      }
      const last = children[children.length - 1]
      if (last?.type === 'text') last.value += glyph.text
      else children.push({ type: 'text', value: glyph.text })
    }
    cursor += [...unit.text].length
    return unit.protect ? [{ type: 'element', tagName: 'span', properties: { className: ['no-orphan'] }, children: group }] : group
  })
}
export function rehypeNoOrphans() {
  return (tree: Node) => {
    function visit(node: Node) {
      if (!node.children || node.tagName === 'pre' || node.tagName === 'code') return
      if (/^(p|h[1-6]|li|td|th|caption)$/.test(node.tagName ?? '')) {
        const children: Node[] = [], run: Node[] = []
        const flush = () => { children.push(...protectInline(run)); run.length = 0 }
        for (const child of node.children) {
          if (isInline(child)) run.push(child)
          else { flush(); visit(child); children.push(child) }
        }
        flush(); node.children = children
      } else node.children.forEach(visit)
    }
    visit(tree)
  }
}
