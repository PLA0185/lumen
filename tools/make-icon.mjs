#!/usr/bin/env node
/**
 * 生成 AiTodo 应用图标源图（1024x1024 PNG），无第三方依赖。
 *
 * 任务书 §1：「独立设计，不直接照搬其他产品的界面、图标或素材」。
 * 本脚本以纯代码合成图形，不使用任何外部素材，因此不存在素材授权问题。
 *
 * 设计：深靛蓝渐变圆角方底 + 白色对勾（完成任务）+ 右上角 AI 星芒点缀。
 * 用法：node tools/make-icon.mjs
 */
import { deflateSync } from 'node:zlib'
import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const S = 1024
const __dirname = dirname(fileURLToPath(import.meta.url))
const outDir = join(__dirname, '..', 'src-tauri', 'icons')
mkdirSync(outDir, { recursive: true })

// ---------- 画布 ----------
const px = new Uint8Array(S * S * 4) // RGBA
const set = (x, y, r, g, b, a) => {
  if (x < 0 || y < 0 || x >= S || y >= S) return
  const i = (y * S + x) * 4
  // 简单 source-over 混合
  const sa = a / 255
  const da = px[i + 3] / 255
  const oa = sa + da * (1 - sa)
  if (oa === 0) return
  px[i] = Math.round((r * sa + px[i] * da * (1 - sa)) / oa)
  px[i + 1] = Math.round((g * sa + px[i + 1] * da * (1 - sa)) / oa)
  px[i + 2] = Math.round((b * sa + px[i + 2] * da * (1 - sa)) / oa)
  px[i + 3] = Math.round(oa * 255)
}

// ---------- 圆角矩形渐变底 ----------
const radius = S * 0.22
const inRounded = (x, y) => {
  const cx = Math.min(Math.max(x, radius), S - radius)
  const cy = Math.min(Math.max(y, radius), S - radius)
  if (x >= radius && x <= S - radius) return y >= 0 && y <= S
  if (y >= radius && y <= S - radius) return x >= 0 && x <= S
  return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2
}
for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    if (!inRounded(x + 0.5, y + 0.5)) continue
    // 对角线渐变：#4F46E5 -> #7C3AED -> #06B6D4 过渡
    const t = (x / S) * 0.6 + (y / S) * 0.4
    let r, g, b
    if (t < 0.5) {
      const k = t / 0.5
      r = 79 + (124 - 79) * k
      g = 70 + (58 - 70) * k
      b = 229 + (237 - 229) * k
    } else {
      const k = (t - 0.5) / 0.5
      r = 124 + (6 - 124) * k
      g = 58 + (182 - 58) * k
      b = 237 + (212 - 237) * k
    }
    set(x, y, Math.round(r), Math.round(g), Math.round(b), 255)
  }
}

// ---------- 对勾（抗锯齿：以有符号距离场估算覆盖率） ----------
const distToSegment = (px_, py, x1, y1, x2, y2) => {
  const dx = x2 - x1
  const dy = y2 - y1
  const len2 = dx * dx + dy * dy
  let t = len2 === 0 ? 0 : ((px_ - x1) * dx + (py - y1) * dy) / len2
  t = Math.max(0, Math.min(1, t))
  return Math.hypot(px_ - (x1 + t * dx), py - (y1 + t * dy))
}

const stroke = S * 0.075
const check = [
  [S * 0.3, S * 0.53, S * 0.45, S * 0.68],
  [S * 0.45, S * 0.68, S * 0.72, S * 0.34],
]
for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    if (!inRounded(x + 0.5, y + 0.5)) continue
    let d = Infinity
    for (const [x1, y1, x2, y2] of check) {
      d = Math.min(d, distToSegment(x + 0.5, y + 0.5, x1, y1, x2, y2))
    }
    const cov = Math.max(0, Math.min(1, stroke / 2 - d + 0.5))
    if (cov > 0) set(x, y, 255, 255, 255, Math.round(cov * 255))
  }
}

// ---------- AI 星芒（四角星，右上角） ----------
const cx = S * 0.755
const cy = S * 0.245
const R = S * 0.115
const w = S * 0.028
for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const dx = Math.abs(x + 0.5 - cx)
    const dy = Math.abs(y + 0.5 - cy)
    if (dx > R || dy > R) continue
    // 四角星：|dx|^0.5 + |dy|^0.5 <= R^0.5 的变体，带宽度
    const dstar = Math.pow(dx / R, 0.5) + Math.pow(dy / R, 0.5)
    const dring = Math.abs(dstar - 0.62) * R
    const cov = Math.max(0, Math.min(1, w / 2 - dring + 0.5))
    if (cov > 0) set(x, y, 255, 255, 255, Math.round(cov * 255))
  }
}

// ---------- PNG 编码 ----------
const crcTable = (() => {
  const t = new Int32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c
  }
  return t
})()
const crc32 = (buf) => {
  let c = -1
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8)
  return (c ^ -1) >>> 0
}
const chunk = (type, data) => {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

const ihdr = Buffer.alloc(13)
ihdr.writeUInt32BE(S, 0)
ihdr.writeUInt32BE(S, 4)
ihdr[8] = 8 // bit depth
ihdr[9] = 6 // RGBA
ihdr[10] = 0
ihdr[11] = 0
ihdr[12] = 0

// 每行前置 filter byte 0
const raw = Buffer.alloc(S * (S * 4 + 1))
for (let y = 0; y < S; y++) {
  raw[y * (S * 4 + 1)] = 0
  Buffer.from(px.buffer, y * S * 4, S * 4).copy(raw, y * (S * 4 + 1) + 1)
}

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
])

const out = join(outDir, 'icon-source.png')
writeFileSync(out, png)
console.log(`已生成 ${out} (${S}x${S}, ${(png.length / 1024).toFixed(1)} KB)`)
