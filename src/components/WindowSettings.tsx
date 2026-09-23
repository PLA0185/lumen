/**
 * 窗口设置界面（任务书 §8）。
 *
 * ## 设计原则：每项设置都要让用户看到实际效果
 *
 * §3 要求"设置页面要让用户直观看到每项窗口配置的实际效果"。
 * 因此这里的每一项都：
 * - 立即生效（不等"保存"按钮）；
 * - 旁边有一句话说明它到底改变什么；
 * - 危险项（穿透、隐藏任务栏、关闭托盘）额外说明后果与恢复方式。
 *
 * ## 安全提示不能省
 *
 * §8 反复强调"不得让用户必须删除配置文件才能找回窗口"。
 * 因此界面上有一块常驻的"恢复入口"状态区，实时告诉用户当前有几条
 * 可用的找回路径；当某次操作会让它变成 0 条时，后端会直接拒绝，
 * 这里把拒绝原因原样呈现。
 */

import { useCallback, useEffect, useState } from 'react'
import * as win from '../lib/window-ipc'
import { IpcError } from '../lib/ipc'
import { useApp } from '../lib/store'
import type { WindowConfig, WindowConfigState } from '../lib/window-ipc'

function errText(e: unknown): string {
  return e instanceof IpcError ? e.userMessage() : String(e)
}

export function WindowSettings() {
  const pushToast = useApp((s) => s.pushToast)
  const [state, setState] = useState<WindowConfigState | null>(null)
  const [cfg, setCfg] = useState<WindowConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [shortcutError, setShortcutError] = useState<string | null>(null)

  const reload = useCallback(async () => {
    try {
      const s = await win.windowGetConfig()
      setState(s)
      setCfg(s.config)
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  /** 监听后端广播的配置变化（托盘操作也会改配置） */
  useEffect(() => {
    let unlisten: (() => void) | undefined
    let unlistenErr: (() => void) | undefined
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen<WindowConfig>('window-config-changed', (e) => {
          setCfg(e.payload)
        })
        unlistenErr = await listen<string>('shortcut-error', (e) => {
          setShortcutError(e.payload)
        })
      } catch {
        // 非 Tauri 环境忽略
      }
    })()
    return () => {
      unlisten?.()
      unlistenErr?.()
    }
  }, [])

  /** 提交配置；失败时把后端给出的原因展示出来 */
  const apply = async (patch: Partial<WindowConfig>) => {
    if (!cfg) return
    const next: WindowConfig = { ...cfg, ...patch }
    // 乐观更新让开关跟手；失败时回滚
    setCfg(next)
    setBusy(true)
    setError(null)
    try {
      const saved = await win.windowSetConfig(next)
      setCfg(saved)
      setState((s) => (s ? { ...s, config: saved, hasRecoveryPath: saved.trayEnabled || saved.shortcutEnabled } : s))
    } catch (e) {
      setCfg(cfg) // 回滚到改动前
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const resetSafe = async () => {
    if (
      !window.confirm(
        '重置窗口设置？\n\n' +
          '会把窗口恢复为安全默认值：主窗口显示在任务栏、悬浮窗关闭且不穿透、托盘启用。\n' +
          '这是"窗口找不到了"时的应急手段。',
      )
    ) {
      return
    }
    setBusy(true)
    try {
      const saved = await win.windowResetSafe()
      setCfg(saved)
      await reload()
      pushToast('success', '窗口设置已重置为安全默认值')
    } catch (e) {
      setError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  if (!cfg || !state) {
    return (
      <div className="setgroup">
        <h3 className="setgroup__title">窗口</h3>
        <p className="setgroup__desc">{error ?? '读取窗口配置中…'}</p>
      </div>
    )
  }

  /** 恢复入口状态：这是本页最重要的实时信息 */
  const recoveryCount = (cfg.trayEnabled ? 1 : 0) + (cfg.shortcutEnabled ? 1 : 0)

  return (
    <>
      {/* ---------------- 恢复入口状态（常驻） ---------------- */}
      <div className={`setgroup recovery${recoveryCount === 0 ? ' recovery--danger' : ''}`}>
        <h3 className="setgroup__title">窗口恢复入口</h3>
        <p className="setgroup__desc">
          如果把窗口设置得很隐蔽（穿透、隐藏任务栏、关掉托盘），你需要一条能重新找回界面的路径。
          当前可用：
        </p>
        <ul className="recovery__list">
          <li className={cfg.trayEnabled ? 'ok' : 'off'}>
            {cfg.trayEnabled ? '✓' : '✕'} 托盘图标（右键可打开主窗口；另有「窗口找不到了？重置窗口设置」）
          </li>
          <li className={cfg.shortcutEnabled ? 'ok' : 'off'}>
            {cfg.shortcutEnabled ? '✓' : '✕'} 全局快捷键 {win.humanizeAccel(cfg.shortcutToggle)} 打开/隐藏主窗口
          </li>
          <li className="ok">✓ 每次启动都会自动关闭悬浮窗穿透并显示主窗口</li>
        </ul>
        {recoveryCount === 0 ? (
          <div className="alert alert--error" role="alert">
            <span>
              <strong>当前没有任何手动恢复入口。</strong>
              你仍可依靠"重启后自动显示主窗口"这一条，但不建议这样使用。
              请至少启用托盘或全局快捷键。
            </span>
          </div>
        ) : (
          <p className="setgroup__hint">
            共 {recoveryCount} 条手动入口 + 1 条自动恢复。开启鼠标穿透前必须保留至少一条手动入口。
          </p>
        )}
        <div className="setactions">
          <button type="button" className="btn btn--danger" disabled={busy} onClick={() => void resetSafe()}>
            窗口找不到了？重置窗口设置
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => void win.windowFloatingResetPosition()}
            title="把悬浮窗移回屏幕右下角"
          >
            把悬浮窗移回右下角
          </button>
        </div>
      </div>

      {error && (
        <div className="alert alert--error" role="alert">
          <span className="selectable">{error}</span>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      )}

      {shortcutError && (
        <div className="alert alert--warn" role="alert">
          <span>
            <strong>全局快捷键注册失败：</strong>
            {shortcutError}
          </span>
          <button
            type="button"
            className="icon-btn"
            aria-label="关闭"
            onClick={() => setShortcutError(null)}
          >
            ✕
          </button>
        </div>
      )}

      {/* ---------------- 主窗口 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">主窗口</h3>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">始终置顶</div>
            <div className="setrow__desc">
              让主窗口保持在其它窗口之上。
              <strong>这与任务列表里的「置顶任务」是两回事</strong>——后者只影响任务排序，
              不会改变任何窗口层级。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '开启'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.mainAlwaysOnTop === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.mainAlwaysOnTop === v}
                disabled={busy}
                onClick={() => void apply({ mainAlwaysOnTop: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">在任务栏显示</div>
            <div className="setrow__desc">
              关闭后主窗口不再出现在任务栏与 Alt+Tab 中，只能通过托盘或快捷键唤出。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '显示'],
                [false, '隐藏'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.mainShowInTaskbar === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.mainShowInTaskbar === v}
                disabled={busy}
                onClick={() => void apply({ mainShowInTaskbar: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">关闭主窗口时</div>
            <div className="setrow__desc">
              选择「缩到托盘」时，你需要从托盘菜单或快捷键重新打开窗口；
              选择「退出程序」则真正结束运行，提醒也不会再触发。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                ['tray', '缩到托盘'],
                ['quit', '退出程序'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={v}
                type="button"
                className={`segmented__item${cfg.closeAction === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.closeAction === v}
                disabled={busy}
                onClick={() => void apply({ closeAction: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* ---------------- 悬浮窗 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">今日悬浮小窗</h3>
        <p className="setgroup__desc">
          一个无边框的桌面小组件，只显示今天的任务。可以置顶、半透明、甚至让鼠标点穿它。
        </p>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">显示悬浮窗</div>
            <div className="setrow__desc">开启后立即出现在屏幕右下角（无边框窗口，拖动可移动）。</div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '显示'],
                [false, '隐藏'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.floatingEnabled === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.floatingEnabled === v}
                disabled={busy}
                onClick={() => void apply({ floatingEnabled: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">悬浮窗不透明度</div>
            <div className="setrow__desc">
              实时生效。下限为 {Math.round(state.opacityMin * 100)}%，再低文字就无法阅读，
              也失去了"找回窗口"的可能。
            </div>
          </div>
          <div className="setrow__control">
            <input
              type="range"
              min={state.opacityMin}
              max={1}
              step={0.05}
              value={cfg.floatingOpacity}
              aria-label="悬浮窗不透明度"
              disabled={busy || !cfg.floatingEnabled}
              onChange={(e) => void apply({ floatingOpacity: Number(e.target.value) })}
            />
            <span className="setrow__value">{Math.round(cfg.floatingOpacity * 100)}%</span>
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">悬浮窗大小</div>
            <div className="setrow__desc">
              当前 {Math.round(cfg.floatingWidth)} × {Math.round(cfg.floatingHeight)}。
              也可以直接拖动悬浮窗右下角的把手调整，松手后自动记住。
            </div>
          </div>
          <div className="setrow__control">
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              disabled={busy}
              title="恢复为默认尺寸 320 × 460"
              onClick={() =>
                void (async () => {
                  try {
                    const applied = await win.windowSetFloatingSize(320, 460)
                    await apply({
                      floatingWidth: applied.width,
                      floatingHeight: applied.height,
                    })
                  } catch (e) {
                    setError(errText(e))
                  }
                })()
              }
            >
              恢复默认大小
            </button>
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">悬浮窗始终置顶</div>
            <div className="setrow__desc">让今日清单浮在其它窗口之上。</div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '开启'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.floatingAlwaysOnTop === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.floatingAlwaysOnTop === v}
                disabled={busy || !cfg.floatingEnabled}
                onClick={() => void apply({ floatingAlwaysOnTop: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">鼠标穿透</div>
            <div className="setrow__desc">
              开启后鼠标点击会"穿过"悬浮窗作用到下层窗口，因此
              <strong>你将无法点击悬浮窗上的任何内容</strong>，包括它自己的关闭按钮。
              关闭方式只有两条：托盘菜单的「悬浮窗鼠标穿透」，
              或按 {win.humanizeAccel(cfg.shortcutToggle)} 打开主窗口后来这里关闭。
              每次重启也会自动关闭它。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '开启'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.floatingClickThrough === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.floatingClickThrough === v}
                disabled={busy || !cfg.floatingEnabled}
                onClick={() => void apply({ floatingClickThrough: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">悬浮窗在任务栏显示</div>
            <div className="setrow__desc">
              默认关闭——它更像桌面组件而不是一个独立程序窗口。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '显示'],
                [false, '隐藏'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.floatingShowInTaskbar === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.floatingShowInTaskbar === v}
                disabled={busy}
                onClick={() => void apply({ floatingShowInTaskbar: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* ---------------- 托盘与快捷键 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">托盘与快捷键</h3>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">启用托盘图标</div>
            <div className="setrow__desc">
              托盘菜单包含打开/隐藏主窗口、快速添加、今日概览、置顶与穿透开关、
              暂停提醒、设置、窗口重置与完全退出。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '启用'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.trayEnabled === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.trayEnabled === v}
                disabled={busy}
                onClick={() => void apply({ trayEnabled: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="setrow">
          <div className="setrow__label">
            <div className="setrow__title">启用全局快捷键</div>
            <div className="setrow__desc">
              在任意程序前台时都能唤出 Lumen。这是穿透与隐藏设置下最可靠的恢复入口。
            </div>
          </div>
          <div className="segmented">
            {(
              [
                [true, '启用'],
                [false, '关闭'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                className={`segmented__item${cfg.shortcutEnabled === v ? ' segmented__item--on' : ''}`}
                aria-pressed={cfg.shortcutEnabled === v}
                disabled={busy}
                onClick={() => void apply({ shortcutEnabled: v })}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {cfg.shortcutEnabled && (
          <>
            {(
              [
                ['shortcutToggle', '打开/隐藏主窗口'],
                ['shortcutQuickAdd', '快速添加'],
                ['shortcutToday', '今日概览'],
              ] as const
            ).map(([key, label]) => (
              <div className="setrow" key={key}>
                <div className="setrow__label">
                  <div className="setrow__title">{label}</div>
                  <div className="setrow__desc">
                    当前：<code>{win.humanizeAccel(cfg[key])}</code>
                  </div>
                </div>
                <select
                  className="input input--compact"
                  value={cfg[key]}
                  aria-label={`${label}快捷键`}
                  disabled={busy}
                  onChange={(e) => void apply({ [key]: e.target.value } as Partial<WindowConfig>)}
                >
                  {[...new Set([cfg[key], ...win.SHORTCUT_PRESETS])].map((a) => (
                    <option key={a} value={a}>
                      {win.humanizeAccel(a)}
                    </option>
                  ))}
                </select>
              </div>
            ))}
            <p className="setgroup__hint">
              若某个组合已被其它程序占用，注册会失败并在页面顶部给出提示——
              此时换一个组合即可。修改后立即生效，无需重启。
            </p>
          </>
        )}
      </div>

      {/* ---------------- 数据与退出 ---------------- */}
      <div className="setgroup">
        <h3 className="setgroup__title">退出</h3>
        <p className="setgroup__desc">
          「完全退出」会结束程序，之后提醒不会再触发（直到你再次启动 Lumen）。
          如果只是想让它不在前台，用主窗口右上角的关闭按钮即可（按当前设置为准）。
        </p>
        <div className="setactions">
          <button
            type="button"
            className="btn btn--danger"
            onClick={() => {
              if (window.confirm('确定要完全退出 Lumen 吗？\n\n退出后提醒不会再触发。')) {
                void win.appQuit()
              }
            }}
          >
            完全退出 Lumen
          </button>
        </div>
      </div>
    </>
  )
}
