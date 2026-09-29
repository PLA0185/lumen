import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('SQL 迁移跨机器校验和稳定', () => {
  it('旧迁移固定 CRLF 保留已安装校验和，新迁移固定 LF', () => {
    for (const name of readdirSync('src-tauri/migrations').filter((p) => p.endsWith('.sql'))) {
      const path = `src-tauri/migrations/${name}`
      const old = Number(name.split('_')[0]) <= 7
      const ending = old ? 'crlf' : 'lf'
      expect(execFileSync('git', ['check-attr', 'eol', '--', path], { encoding: 'utf8' }).trim()).toBe(`${path}: eol: ${ending}`)
      const text = readFileSync(path, 'utf8')
      expect(text).toContain(old ? '\r\n' : '\n')
      expect(old ? text.replaceAll('\r\n', '').includes('\n') : text.includes('\r')).toBe(false)
    }
  })
})
