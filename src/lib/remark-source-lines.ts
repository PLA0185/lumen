type Node = { type: string; value?: string; children?: Node[] }
/** Render source soft line endings as breaks without changing stored text, code or tables. */
export function remarkSourceLines() {
  return (tree: Node) => {
    function visit(node: Node) {
      if (!node.children || ['code', 'inlineCode', 'table'].includes(node.type)) return
      node.children = node.children.flatMap(child => {
        if (child.type === 'text' && child.value?.includes('\n')) return child.value.split(/\r?\n/).flatMap((value, i) => i ? [{ type: 'break' }, { type: 'text', value }] : [{ type: 'text', value }])
        visit(child)
        return [child]
      })
    }
    visit(tree)
  }
}
