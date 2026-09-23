#!/usr/bin/env node
/**
 * 生成自动更新所需的 `latest.json`。
 *
 * ## 为什么需要这个脚本
 *
 * Tauri **本地构建不会生成 `latest.json`**（官方文档明确说这一步由
 * `tauri-action` 在 CI 里完成）。本项目手动发布 Release，所以必须自己生成，
 * 否则客户端拉不到更新清单，自动更新永远"没有新版本"。
 *
 * ## 格式要点（踩过的坑）
 *
 * - 必需键只有三个：`version`、`platforms.<target>.url`、`platforms.<target>.signature`；
 * - Windows x64 的平台键**必须是 `windows-x86_64`**；
 * - 插件会**先校验整份 JSON 再比版本号**，所以有哪个平台就得写全，
 *   不能留半空占位（写不全就干脆不写那个平台，而不是写空字符串）；
 * - `signature` 是 `.sig` 文件的**内容**，不是路径。
 *
 * 用法：
 *   node tools/make-latest-json.mjs --version 0.2.0 \
 *     --repo PLA0185/lumen [--notes-file RELEASE_NOTES.md] [--out latest.json]
 */

import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

const args = process.argv.slice(2)
function arg(name, fallback = undefined) {
  const i = args.indexOf(`--${name}`)
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback
}

const version = arg('version')
const repo = arg('repo', 'PLA0185/lumen')
const out = arg('out', 'latest.json')
const notesFile = arg('notes-file')

if (!version) {
  console.error('缺少 --version，例如 --version 0.2.0')
  process.exit(1)
}

// NSIS 产物：target/release/bundle/nsis/Lumen_<version>_x64-setup.exe
//
// 注意：Tauri v2 在 Windows 上的**更新包就是这个 .exe 本身**，
// 不是另外的 zip。插件源码里 `extract()` 会先看是不是 zip，不是就按
// `extract_exe` 走（把 exe 落在临时目录后静默运行安装器）。
// 因此 latest.json 的 url 必须指向 `-setup.exe`，signature 是它的
// `.sig` 文件内容。曾经想当然写成 `.nsis.zip` 会直接 404。
const bundleDir = resolve('src-tauri/target/release/bundle/nsis')
const artifactName = `Lumen_${version}_x64-setup.exe`
const artifactPath = join(bundleDir, artifactName)
const sigPath = `${artifactPath}.sig`

for (const [p, what] of [
  [artifactPath, '更新包（NSIS 安装器）'],
  [sigPath, '签名文件 .sig'],
]) {
  if (!existsSync(p)) {
    console.error(
      `找不到${what}：${p}\n` +
        '请先设置 TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD 并执行 `pnpm tauri build`' +
        '（需要在 tauri.conf.json 中开启 createUpdaterArtifacts）。',
    )
    process.exit(1)
  }
}

const signature = readFileSync(sigPath, 'utf8').trim()
const notes = notesFile && existsSync(notesFile) ? readFileSync(notesFile, 'utf8').trim() : ''

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      // Releases 里的资产名会被 URL 编码，这里按原样拼即可
      url: `https://github.com/${repo}/releases/download/v${version}/${artifactName}`,
      signature,
    },
  },
}

writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8')
console.log(`已生成 ${out}（版本 ${version}，更新包 ${artifactName}）`)
