import { readFileSync } from 'node:fs'

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
const tauri = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8')
const packageSection = cargo.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0]
const cargoVersion = packageSection?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
const versions = [pkg.version, cargoVersion, tauri.version]
if (!versions.every((version) => version && version === versions[0])) {
  throw new Error(`版本不一致：package=${pkg.version} Cargo=${cargoVersion} tauri=${tauri.version}`)
}

const tag = process.argv.find((value) => value.startsWith('--tag='))?.slice(6)
if (tag !== undefined) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error(`非法发布 tag：${tag}`)
  if (tag.slice(1) !== versions[0]) throw new Error(`tag ${tag} 与应用版本 ${versions[0]} 不一致`)
}
console.log(`版本一致：${versions[0]}${tag ? ` / ${tag}` : ''}`)
