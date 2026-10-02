export type Point = { x: number; y: number }
export type Annotation = { id: string; kind: 'pen' | 'arrow' | 'text'; points: Point[]; color: string; width: number; text: string; fontSize: number }
export function arrowHead(a: Point, b: Point, width: number): Point[] {
  const angle = Math.atan2(b.y - a.y, b.x - a.x), length = Math.min(24, width * 2.5 + 5, Math.hypot(b.x - a.x, b.y - a.y) * .28)
  return [b, { x: b.x - length * Math.cos(angle - Math.PI / 6), y: b.y - length * Math.sin(angle - Math.PI / 6) }, { x: b.x - length * Math.cos(angle + Math.PI / 6), y: b.y - length * Math.sin(angle + Math.PI / 6) }]
}
export function arrowShaft(a: Point, b: Point, width: number): { end: Point; width: number } {
  const head = arrowHead(a, b, width)
  return { end: { x: (head[1]!.x + head[2]!.x) / 2, y: (head[1]!.y + head[2]!.y) / 2 }, width: Math.min(width, Math.hypot(b.x - a.x, b.y - a.y) * .08) }
}
export function textBounds(item: Annotation): { width: number; height: number } {
  const lines = item.text.split('\n')
  return { width: Math.max(item.fontSize, ...lines.map(line => [...line].reduce((sum, ch) => sum + (/\p{Script=Han}/u.test(ch) ? 1 : .65), 0) * item.fontSize)), height: lines.length * item.fontSize * 1.3 }
}
export function annotationHit(item: Annotation, point: Point, tolerance: number): boolean {
  if (item.kind === 'text') {
    const first = item.points[0]!
    const bounds = textBounds(item)
    return point.x >= first.x - tolerance && point.x <= first.x + bounds.width + tolerance && point.y >= first.y - tolerance && point.y <= first.y + bounds.height + tolerance
  }
  const points = item.points
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1]!, b = points[i]!, dx = b.x - a.x, dy = b.y - a.y
    const t = Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / (dx * dx + dy * dy || 1)))
    if (Math.hypot(point.x - a.x - t * dx, point.y - a.y - t * dy) <= tolerance + item.width / 2) return true
  }
  return false
}
export async function annotatedImage(url: string, items: Annotation[], format: 'png' | 'jpeg' | 'webp' = 'png'): Promise<File> {
  const image = new Image()
  image.src = url
  await image.decode()
  if (!image.naturalWidth || image.naturalWidth * image.naturalHeight > 25_000_000) throw new Error('图片超过 2500 万像素，请缩小后再批注')
  const canvas = document.createElement('canvas'); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight
  const context = canvas.getContext('2d')
  if (!context) throw new Error('无法创建图片画布')
  if (format === 'jpeg') { context.fillStyle = '#fff'; context.fillRect(0, 0, canvas.width, canvas.height) }
  context.drawImage(image, 0, 0)
  for (const item of items) {
    context.strokeStyle = item.color; context.fillStyle = item.color; context.lineWidth = item.width; context.lineCap = 'round'; context.lineJoin = 'round'
    const first = item.points[0]
    if (!first) continue
    if (item.kind === 'text') { context.font = `${item.fontSize}px "Microsoft YaHei", sans-serif`; context.textBaseline = 'top'; item.text.split('\n').forEach((line, index) => context.fillText(line, first.x, first.y + index * item.fontSize * 1.3)); continue }
    context.beginPath(); context.moveTo(first.x, first.y)
    if (item.kind === 'arrow' && item.points.length > 1) {
      const shaft = arrowShaft(first, item.points[item.points.length - 1]!, item.width)
      context.lineWidth = shaft.width; context.lineTo(shaft.end.x, shaft.end.y)
    } else for (const p of item.points.slice(1)) context.lineTo(p.x, p.y)
    context.stroke()
    if (item.kind === 'arrow' && item.points.length >= 2) {
      const head = arrowHead(first, item.points[item.points.length - 1]!, item.width)
      context.beginPath(); context.moveTo(head[0]!.x, head[0]!.y); context.lineTo(head[1]!.x, head[1]!.y); context.lineTo(head[2]!.x, head[2]!.y); context.closePath(); context.fill()
    }
  }
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(b => b ? resolve(b) : reject(new Error('图片编码失败')), `image/${format}`, .95))
  if (blob.type !== `image/${format}`) throw new Error('此系统无法编码所选图片格式')
  if (blob.size > 20 * 1024 * 1024) throw new Error('批注图片超过 20 MiB，请缩小后重试')
  return new File([blob], `批注图片.${format === 'jpeg' ? 'jpg' : format}`, { type: blob.type })
}
