/**
 * IPC 暴露面的静态护栏（最终收口任务书 §7）。
 *
 * ## 为什么需要"读源码文本"的测试
 *
 * 上一轮把旧的批量永久删除 `task_purge_all_deleted` 标注成"已弃用"，
 * 但它**仍然注册在 `generate_handler` 里**、前端也还留着命令常量。
 * 也就是说任何代码仍然可以 `invoke("task_purge_all_deleted")` 走到那条旧路径
 * —— 而那条路径用的是"事务外取附件路径 + 只校验数量 + 按动态 query 删除"的旧模型，
 * 不满足现在的破坏性删除标准。
 *
 * 这类"接口是否仍然暴露"的问题，用行为测试很难覆盖（没人调用就不会失败），
 * 所以这里直接对**源码文本**做断言：只要有人把它加回来，测试立刻红。
 * 这是刻意的、也是任务书 §7 明确要求的做法。
 */

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '../..')

function readSrc(relative: string): string {
  return readFileSync(resolve(repoRoot, relative), 'utf8')
}

describe('危险的旧 IPC 不得重新暴露', () => {
  it('前端 ipc.ts 里不再有 task_purge_all_deleted 命令常量', () => {
    const src = readSrc('src/lib/ipc.ts')
    expect(src).not.toContain('task_purge_all_deleted')
  })

  it('前端不再导出 purgeAllDeleted 这个只校验数量的包装', () => {
    const src = readSrc('src/lib/ipc.ts')
    expect(src).not.toContain('purgeAllDeleted')
  })

  it('Tauri 侧不再把它注册成命令（Rust 侧另有同源断言）', () => {
    // 前端这边只能看到自己的文件；Rust 侧由 remediation2_e2e.rs 的
    // dangerous_bulk_purge_command_is_not_exposed_anymore 检查 generate_handler。
    // 这里额外确认前端**没有**任何地方重新拼出这个命令名。
    for (const f of ['src/lib/ipc.ts', 'src/lib/store.ts', 'src/App.tsx']) {
      expect(readSrc(f), `${f} 不应出现旧命令名`).not.toContain('task_purge_all_deleted')
    }
  })

  it('两阶段命令仍然在位（防止相反方向的回归：删过头把安全路径也删了）', () => {
    const src = readSrc('src/lib/ipc.ts')
    expect(src).toContain('task_prepare_purge_deleted')
    expect(src).toContain('task_commit_purge_deleted')
  })
})
