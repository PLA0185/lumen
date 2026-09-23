/**
 * 窗口能力 IPC 封装（任务书 §8）。
 */

import { invoke } from '@tauri-apps/api/core'
import { IpcError } from './ipc'
import type { ErrorCode } from './types'

/** 窗口配置（与 Rust `WindowConfig` 一一对应） */
export interface WindowConfig {
  /** 主窗口是否始终置顶（与"任务置顶"无关） */
  mainAlwaysOnTop: boolean
  /** 主窗口是否显示在任务栏 */
  mainShowInTaskbar: boolean
  /** 关闭主窗口的行为：缩到托盘 / 退出程序 */
  closeAction: 'tray' | 'quit'

  /** 是否显示悬浮今日小窗 */
  floatingEnabled: boolean
  floatingAlwaysOnTop: boolean
  /** 鼠标穿透：开启后悬浮窗无法被点击 */
  floatingClickThrough: boolean
  /** 不透明度（下限见 opacityMin） */
  floatingOpacity: number
  floatingShowInTaskbar: boolean
  floatingX: number | null
  floatingY: number | null
  /** 悬浮窗尺寸（逻辑像素），由拖动把手调节后持久化 */
  floatingWidth: number
  floatingHeight: number

  /** 是否启用托盘图标 */
  trayEnabled: boolean
  /** 是否启用全局快捷键 */
  shortcutEnabled: boolean
  shortcutToggle: string
  shortcutQuickAdd: string
  shortcutToday: string
}

/** 窗口配置查询结果 */
export interface WindowConfigState {
  config: WindowConfig
  /** 不透明度下限（由后端定义，前端不重复硬编码） */
  opacityMin: number
  /** 当前是否至少存在一条恢复路径 */
  hasRecoveryPath: boolean
}

/** 悬浮窗运行态 */
export interface FloatingState {
  enabled: boolean
  visible: boolean
  clickThrough: boolean
  alwaysOnTop: boolean
  opacity: number
  /** 当前持久化的尺寸（逻辑像素） */
  width: number
  height: number
}

export const WINDOW_CMD = {
  getConfig: 'window_get_config',
  setConfig: 'window_set_config',
  applyAction: 'window_apply_action',
  resetSafe: 'window_reset_safe',
  floatingState: 'window_floating_state',
  floatingResetPosition: 'window_floating_reset_position',
  setFloatingOpacity: 'window_set_floating_opacity',
  setFloatingSize: 'window_set_floating_size',
  quit: 'app_quit',
} as const

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    throw new IpcError('internal', '当前不在 Lumen 桌面程序内运行', null, null)
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    if (e && typeof e === 'object' && 'message' in e && 'code' in e) {
      const err = e as { code: string; message: string; hint: string | null }
      throw new IpcError(err.code as ErrorCode, err.message, err.hint ?? null, e)
    }
    if (typeof e === 'string') throw new IpcError('internal', e, null, e)
    throw new IpcError('internal', '发生了未预期的错误', null, e)
  }
}

export const windowGetConfig = (): Promise<WindowConfigState> => call(WINDOW_CMD.getConfig)

/**
 * 更新窗口配置。
 *
 * 后端会做两项安全校验并可能拒绝：
 * - 开启穿透但没有任何恢复路径（托盘与快捷键都关）；
 * - 同时隐藏任务栏图标并关闭托盘。
 * 因此调用方必须处理 IpcError 并把原因展示给用户。
 */
export const windowSetConfig = (config: WindowConfig): Promise<WindowConfig> =>
  call(WINDOW_CMD.setConfig, { config })

/** 即时操作（不进设置页也能切换） */
export type WindowAction =
  | 'toggle_main_top'
  | 'toggle_floating_top'
  | 'toggle_floating_click_through'
  | 'toggle_tray'
  | 'show_floating'
  | 'hide_floating'
  | 'show_main'

export const windowApplyAction = (action: WindowAction): Promise<WindowConfig> =>
  call(WINDOW_CMD.applyAction, { action })

/** 窗口安全重置：把配置拉回安全默认值（§8 的兜底手段） */
export const windowResetSafe = (): Promise<WindowConfig> => call(WINDOW_CMD.resetSafe)

export const windowFloatingState = (): Promise<FloatingState> => call(WINDOW_CMD.floatingState)

/** 把悬浮窗移回屏幕右下角（拖丢后的一键找回） */
export const windowFloatingResetPosition = (): Promise<boolean> =>
  call(WINDOW_CMD.floatingResetPosition)

/**
 * 设置悬浮窗不透明度。
 *
 * 悬浮窗里的滑块拖动时高频调用，因此后端只写配置并广播事件，
 * 不重建窗口——重建会让用户看到明显闪烁。
 */
export const windowSetFloatingOpacity = (opacity: number): Promise<number> =>
  call(WINDOW_CMD.setFloatingOpacity, { opacity })

/** 设置悬浮窗尺寸；后端会做边界收敛并把收敛结果返回 */
export const windowSetFloatingSize = (
  width: number,
  height: number,
): Promise<{ width: number; height: number }> =>
  call(WINDOW_CMD.setFloatingSize, { width, height })

/** 完全退出应用 */
export const appQuit = (): Promise<void> => call(WINDOW_CMD.quit)

/** 快捷键的可读描述（把 CmdOrCtrl 转成用户熟悉的 Ctrl） */
export function humanizeAccel(accel: string): string {
  return accel
    .replace(/CmdOrCtrl|CommandOrControl/gi, 'Ctrl')
    .replace(/Super|Meta/gi, 'Win')
    .replace(/\+/g, ' + ')
}

/**
 * 常用的快捷键候选。
 *
 * 刻意避开 Ctrl+Alt+T 这类被系统或常见软件占用的组合——
 * 实机验证时 Ctrl+Alt+T 就因被占用而注册失败。
 */
export const SHORTCUT_PRESETS: string[] = [
  'CmdOrCtrl+Alt+A',
  'CmdOrCtrl+Alt+D',
  'CmdOrCtrl+Alt+N',
  'CmdOrCtrl+Alt+Q',
  'CmdOrCtrl+Shift+A',
  'CmdOrCtrl+Shift+T',
  'Alt+Shift+A',
  'Alt+Shift+N',
]
