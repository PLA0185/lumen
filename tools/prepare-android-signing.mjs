import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'

const androidDir = resolve(process.argv[2] ?? 'src-tauri/gen/android')
const keystoreBase64 = process.env.LUMEN_ANDROID_KEYSTORE_BASE64?.trim()
const keyAlias = process.env.LUMEN_ANDROID_KEY_ALIAS?.trim()
const keystorePassword = process.env.LUMEN_ANDROID_KEYSTORE_PASSWORD
const keyPassword = process.env.LUMEN_ANDROID_KEY_PASSWORD

if (!keystoreBase64 || !keyAlias || !keystorePassword || !keyPassword) {
  throw new Error('Android release 签名密钥未完整配置；请设置四个 LUMEN_ANDROID_* 环境变量。')
}
for (const [name, value] of Object.entries({ keyAlias, keystorePassword, keyPassword })) {
  if (!value || /[\r\n\0]/.test(value)) throw new Error(`${name} 为空或含换行字符。`)
}

const keystore = Buffer.from(keystoreBase64, 'base64')
if (keystore.length < 1024) throw new Error('Android keystore 内容过短，拒绝生成发布包。')

const gradlePath = resolve(androidDir, 'app/build.gradle.kts')
let gradle = readFileSync(gradlePath, 'utf8')
if (!gradle.includes('signingConfigs.create("lumenRelease")')) {
  const buildTypes = /^([ \t]*)buildTypes\s*\{/m.exec(gradle)
  if (!buildTypes) throw new Error('找不到 app/build.gradle.kts 中的 buildTypes 块；未改动文件。')
  const indent = buildTypes[1]
  const signingBlock = `${indent}signingConfigs {\n${indent}    create("lumenRelease") {\n${indent}        keyAlias = System.getenv("LUMEN_ANDROID_KEY_ALIAS") ?: error("Missing LUMEN_ANDROID_KEY_ALIAS")\n${indent}        keyPassword = System.getenv("LUMEN_ANDROID_KEY_PASSWORD") ?: error("Missing LUMEN_ANDROID_KEY_PASSWORD")\n${indent}        storeFile = file(System.getenv("LUMEN_ANDROID_KEYSTORE_PATH") ?: error("Missing LUMEN_ANDROID_KEYSTORE_PATH"))\n${indent}        storePassword = System.getenv("LUMEN_ANDROID_KEYSTORE_PASSWORD") ?: error("Missing LUMEN_ANDROID_KEYSTORE_PASSWORD")\n${indent}    }\n${indent}}\n`
  gradle = `${gradle.slice(0, buildTypes.index)}${signingBlock}${gradle.slice(buildTypes.index)}`
}

const releaseBlock = /(getByName\("release"\)\s*\{)/
if (!releaseBlock.test(gradle)) throw new Error('找不到 release buildType 块；未写入签名文件。')
if (!gradle.includes('signingConfig = signingConfigs.getByName("lumenRelease")')) {
  gradle = gradle.replace(releaseBlock, '$1\n            signingConfig = signingConfigs.getByName("lumenRelease")')
}

const keystorePath = resolve(dirname(gradlePath), 'lumen-release.keystore')
writeFileSync(keystorePath, keystore, { mode: 0o600 })
writeFileSync(gradlePath, gradle)
console.log('Android release signing is configured; key material was not printed.')
