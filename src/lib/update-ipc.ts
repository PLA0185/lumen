/**
 * 自动更新（任务书 §9「后续版本升级不需要用户手动下载安装包」）。
 *
 * ## 机制
 *
 * 用官方 `tauri-plugin-updater`：
 * 1. 从 `tauri.conf.json` 的 `plugins.updater.endpoints` 拉 `latest.json`；
 * 2. 比较版本号，有新版则下载更新包（Windows 上是 NSIS 安装器的 zip）；
 * 3. **校验 minisign 签名**——签名不匹配直接拒绝安装。
 *    这一步无法关闭，也是"不让人随便塞一个包就能更新"的唯一保障；
 * 4. 调 NSIS 静默安装（`installMode: passive`，只显示进度条），随后重启。
 *
 * ## 关于失败
 *
 * 更新端点放在 GitHub Releases 上，国内网络可能连不上。
 * 因此所有错误都转成可读中文，并明确区分"已是最新"与"检查失败"——
 * 不能把网络失败说成"已是最新"，那是骗用户。
 */

/** 检查结果：null 表示已是最新 */
export interface UpdateInfo {
  /** 可用版本号 */
  version: string
  /** 发布日期（可能为空） */
  date: string | null
  /** 更新说明（可能为空） */
  notes: string | null
  /** 当前版本号 */
  currentVersion: string
}

/** 下载/安装进度 */
export interface UpdateProgress {
  /** 0–1；总长度未知时为 null */
  percent: number | null
  /** 已下载字节数（仅在能拿到进度时有意义） */
  downloaded: number
}

/** 底层 Update 对象的句柄类型（保持插件类型不泄漏到组件里） */
export type UpdateHandle = Awaited<ReturnType<typeof import('@tauri-apps/plugin-updater')['check']>>

/** 更新来源页面（打不开更新接口时给用户的手动退路） */
export const RELEASES_URL = 'https://github.com/PLA0185/lumen/releases/latest'

/** 用系统默认浏览器打开 Releases 页面 */
export async function openReleasesPage(): Promise<void> {
  const { openUrl } = await import('@tauri-apps/plugin-opener')
  await openUrl(RELEASES_URL)
}

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/**
 * 把插件抛出的异常翻译成人能看懂的说明。
 *
 * 之前这里直接把插件的英文原文抛给用户，于是界面上出现
 * `Could not fetch a valid release JSON from the remote` —— 既没说清是网络问题
 * 还是文件坏了，也没告诉用户下一步该做什么。用户实测就撞上了这条。
 */
function readable(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e)
  if (/InsecureTransportProtocol/i.test(raw)) {
    return '更新地址必须使用 HTTPS，当前配置被拒绝'
  }
  // 插件在"请求失败"和"返回内容不是合法清单"两种情况用的是同一句话，
  // 实际绝大多数是网络到不了 GitHub，因此按最常见原因给建议。
  if (/valid release JSON|release JSON from the remote/i.test(raw)) {
    return (
      `没能从 GitHub 取到更新信息（${raw}）。` +
      '常见原因是网络到不了 GitHub，或对方临时限流；稍后重试通常就好。' +
      '如果一直不行，可以用下面的「手动下载安装包」按钮。'
    )
  }
  if (/dns|connect|timed? ?out|network|error sending request/i.test(raw)) {
    return `无法连接更新服务器（${raw}）。若是网络原因，可稍后重试，或到 GitHub Releases 手动下载安装包。`
  }
  if (/signature|verify|minisign/i.test(raw)) {
    return `更新包签名校验未通过，已拒绝安装（${raw}）。这通常意味着文件被篡改或与当前版本不匹配。`
  }
  return raw
}

/**
 * 检查更新。
 *
 * 返回 `null` 表示确实已是最新；抛错表示"没查成功"。
 * 这两种情况必须分开，否则用户会以为自己是新版。
 */
export async function checkUpdate(): Promise<{ info: UpdateInfo | null; handle: UpdateHandle }> {
  if (!inTauri()) {
    throw new Error('当前不在 Lumen 桌面程序内运行，无法检查更新')
  }
  const { check } = await import('@tauri-apps/plugin-updater')
  const { getVersion } = await import('@tauri-apps/api/app')

  const currentVersion = await getVersion()
  try {
    const update = await check()
    if (!update) return { info: null, handle: null }
    return {
      info: {
        version: update.version,
        date: update.date ?? null,
        notes: update.body ?? null,
        currentVersion,
      },
      handle: update,
    }
  } catch (e) {
    throw new Error(readable(e))
  }
}

/**
 * 下载并安装更新。
 *
 * 安装完成后**应用会自动退出**（Windows 上 NSIS 安装器接管进程），
 * 因此这个函数通常不会正常返回——不要在执行后紧接着刷新界面，
 * 而要让用户看到"正在安装"的提示。
 */
export async function downloadAndInstall(
  handle: NonNullable<UpdateHandle>,
  onProgress?: (p: UpdateProgress) => void,
): Promise<void> {
  let downloaded = 0
  let total: number | null = null
  try {
    await handle.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        total = event.data.contentLength ?? null
        onProgress?.({ percent: total ? 0 : null, downloaded: 0 })
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength
        onProgress?.({
          percent: total && total > 0 ? Math.min(1, downloaded / total) : null,
          downloaded,
        })
      } else {
        onProgress?.({ percent: 1, downloaded })
      }
    })
  } catch (e) {
    throw new Error(readable(e))
  }
}

/** 安装完成后重启应用 */
export async function relaunchApp(): Promise<void> {
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}

/** 字节数 → 可读大小 */
export function humanSize(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
