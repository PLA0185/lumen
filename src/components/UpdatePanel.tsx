/**
 * 检查更新面板（§9）。
 *
 * 三态必须分清，界面不能含糊：
 * - **已是最新**：明确说"当前 x.y.z 已是最新版本"，并显示检查时间；
 * - **有新版本**：显示版本号、更新说明、下载进度；
 * - **检查失败**：显示具体原因 + 手动下载入口，绝不说成"已是最新"。
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import * as up from '../lib/update-ipc'

type Phase = 'idle' | 'checking' | 'latest' | 'available' | 'installing' | 'failed'

export function UpdatePanel({ currentVersion }: { currentVersion: string | undefined }) {
  const [phase, setPhase] = useState<Phase>('idle')
  const [info, setInfo] = useState<up.UpdateInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<up.UpdateProgress | null>(null)
  const [checkedAt, setCheckedAt] = useState<string | null>(null)
  const handle = useRef<up.UpdateHandle>(null)

  const doCheck = useCallback(async () => {
    setPhase('checking')
    setError(null)
    setInfo(null)
    setProgress(null)
    try {
      const { info: found, handle: h } = await up.checkUpdate()
      handle.current = h
      setInfo(found)
      setPhase(found ? 'available' : 'latest')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setPhase('failed')
    } finally {
      setCheckedAt(
        new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }),
      )
    }
  }, [])

  const doInstall = useCallback(async () => {
    if (!handle.current) return
    setPhase('installing')
    setError(null)
    try {
      await up.downloadAndInstall(handle.current, setProgress)
      // 正常情况下走不到这里：Windows 上安装器会接管并结束本进程
      setError('更新已下载完成，安装程序应当已经启动。若界面没有关闭，请手动重启 Lumen。')
      setPhase('failed')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setPhase('failed')
    }
  }, [])

  // 打开面板时自动查一次：用户点进「关于」多半就是想知道有没有新版
  useEffect(() => {
    void doCheck()
  }, [doCheck])

  return (
    <div className="setgroup">
      <h3 className="setgroup__title">软件更新</h3>
      <p className="setgroup__desc">
        更新来自 GitHub Releases。下载后会用 minisign 公钥校验签名，校验不通过会被拒绝安装。
        {checkedAt && `　最近检查：${checkedAt}`}
      </p>

      <div className="setactions">
        <button
          type="button"
          className="btn btn--ghost"
          disabled={phase === 'checking' || phase === 'installing'}
          onClick={() => void doCheck()}
        >
          {phase === 'checking' ? '检查中…' : '检查更新'}
        </button>

        {phase === 'available' && (
          <button type="button" className="btn btn--primary" onClick={() => void doInstall()}>
            下载并安装 v{info?.version}
          </button>
        )}

        {phase === 'installing' && (
          <button type="button" className="btn btn--primary" disabled>
            {progress?.percent != null
              ? `下载中 ${Math.round(progress.percent * 100)}%`
              : progress
                ? `下载中 ${up.humanSize(progress.downloaded)}`
                : '准备下载…'}
          </button>
        )}
      </div>

      {phase === 'installing' && (
        <div className="progress" style={{ marginTop: 8 }}>
          <div
            className="progress__bar"
            style={{ width: `${Math.round((progress?.percent ?? 0) * 100)}%` }}
          />
        </div>
      )}

      {phase === 'latest' && (
        <p className="setgroup__hint">
          当前版本 {info?.currentVersion ?? currentVersion ?? '未知'} 已是最新版本。
        </p>
      )}

      {phase === 'available' && info && (
        <div className="updatecard">
          <div className="updatecard__head">
            发现新版本 <strong>{info.version}</strong>
            <span className="updatecard__from">当前 {info.currentVersion}</span>
          </div>
          {info.date && <div className="updatecard__date">发布日期：{info.date.slice(0, 10)}</div>}
          {info.notes ? (
            <pre className="updatecard__notes selectable">{info.notes}</pre>
          ) : (
            <div className="updatecard__date">该版本未提供更新说明。</div>
          )}
          <p className="setgroup__hint">
            安装过程中任务数据不会被动到：数据在 %APPDATA%\com.pla0185.lumen 下，
            更新只替换程序文件。
          </p>
        </div>
      )}

      {phase === 'failed' && error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
        </div>
      )}

      {/* 手动退路：无论上面成功与否都能用。
          网络到不了 GitHub、或自动安装没生效时，这是唯一确定能升级的路径。 */}
      <div className="setactions">
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          title={`用浏览器打开 ${up.RELEASES_URL}`}
          onClick={() =>
            void (async () => {
              try {
                await up.openReleasesPage()
              } catch (e) {
                setError(
                  `打不开浏览器，请手动访问：${up.RELEASES_URL}（${e instanceof Error ? e.message : String(e)}）`,
                )
                setPhase('failed')
              }
            })()
          }
        >
          手动下载安装包
        </button>
        <span className="setgroup__hint" style={{ margin: 0 }}>
          自动更新失败时走这里，数据不受影响
        </span>
      </div>
    </div>
  )
}
