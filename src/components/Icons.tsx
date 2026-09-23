/**
 * 统一图标集（自绘 SVG，无外部素材）。
 *
 * ## 为什么不用 emoji
 *
 * 初版侧边栏用的是 `☀ ⛅ 🗓 📅 ▦ 📥 📋 ⑦ ㊳ ◲ ❶ 📁 🏷 📊 ⚙` 这类字符。
 * 问题有三：
 * 1. **它们不是一个体系**——彩色 emoji（📅📁）与纯文字符号（⑦◲⇥✎）
 *    混在一起，粗细、大小、基线全都对不齐，看起来像随手拼的；
 * 2. **颜色不受控**——emoji 自带配色，和应用的强调色/深浅主题打架；
 * 3. **跨平台不一致**——Windows 与其它系统的 emoji 字体不同，同一个
 *    图标会长得完全两样。
 *
 * ## 这套图标的约定
 *
 * - 统一 24×24 视口、`stroke="currentColor"`、线宽 1.7、圆角端点；
 * - 颜色一律继承文字色，因此浅色/深色主题、选中态、禁用态自动适配；
 * - 只有"确实需要实心"的部分（第二个周期图标里的点）才单独填充；
 * - 每个图标都放在同一个视觉重量上，并排看不会有的重有的轻。
 *
 * 任务书 §1 要求"视觉独立设计、不照搬第三方素材"，因此所有路径都是
 * 手写坐标，没有引入任何图标库。
 */

import type { ReactNode } from 'react'

export type IconName =
  // ---------------- 导航 ----------------
  | 'today'
  | 'tomorrow'
  | 'week'
  | 'calendar'
  | 'board'
  | 'inbox'
  | 'list'
  | 'period-week'
  | 'period-month'
  | 'period-quarter'
  | 'period-year'
  | 'projects'
  | 'tags'
  | 'completed'
  | 'trash'
  | 'stats'
  | 'settings'
  // ---------------- 操作 ----------------
  | 'plus'
  | 'repeat'
  | 'download'
  | 'alert'
  | 'search'
  | 'close'
  | 'edit'
  | 'chevron-down'
  | 'chevron-up'
  | 'copy'
  | 'restore'
  | 'pin'
  | 'ban'
  | 'contrast'
  | 'archive'
  | 'merge'
  | 'star'
  | 'clock'
  | 'play'
  | 'pause'
  | 'folder-open'
  | 'info'
  | 'minus'
  // ---------------- 附件类型 ----------------
  | 'file'
  | 'image'
  | 'document'
  | 'audio'
  | 'video'
  | 'package'

/** 周期任务图标共用的外框：四个图标只在框内做区分，保证一眼看出是一族 */
const PERIOD_FRAME = <rect x="3.5" y="3.5" width="17" height="17" rx="4.5" />

const PATHS: Record<IconName, ReactNode> = {
  // ============================ 导航 ============================
  today: (
    <>
      <circle cx="12" cy="12" r="4.2" />
      <path d="M12 2.4v2.1M12 19.5v2.1M4.2 4.2l1.5 1.5M18.3 18.3l1.5 1.5M2.4 12h2.1M19.5 12h2.1M4.2 19.8l1.5-1.5M18.3 5.7l1.5-1.5" />
    </>
  ),
  tomorrow: (
    <>
      <circle cx="16.4" cy="7.2" r="2.6" />
      <path d="M16.4 2.6v1.5M21 7.2h-1.5M19.6 4l-1.05 1.05M13.2 4l1.05 1.05" />
      <path d="M6.6 19.4h9a3.4 3.4 0 0 0 .3-6.8 4.9 4.9 0 0 0-9.4 1 3 3 0 0 0 .1 5.8Z" />
    </>
  ),
  week: (
    <>
      <rect x="3" y="5" width="18" height="16" rx="2.6" />
      <path d="M3 9.8h18M8 3.2v3.4M16 3.2v3.4" />
      <path d="M7 13.6h10M7 16.8h6" />
    </>
  ),
  calendar: (
    <>
      <rect x="3" y="5" width="18" height="16" rx="2.6" />
      <path d="M3 9.8h18M8 3.2v3.4M16 3.2v3.4" />
      <path d="M8 13.6h.01M12 13.6h.01M16 13.6h.01M8 17h.01M12 17h.01" />
    </>
  ),
  board: (
    <>
      <rect x="3" y="4" width="5" height="16" rx="1.8" />
      <rect x="9.5" y="4" width="5" height="11" rx="1.8" />
      <rect x="16" y="4" width="5" height="7" rx="1.8" />
    </>
  ),
  inbox: (
    <>
      <path d="M3 13.6 5.5 5.8A2 2 0 0 1 7.4 4.4h9.2a2 2 0 0 1 1.9 1.4L21 13.6v4.4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4.4Z" />
      <path d="M3 13.6h4.8l1.3 2.3h5.8l1.3-2.3H21" />
    </>
  ),
  list: (
    <>
      <path d="M8.5 6.2h12M8.5 12h12M8.5 17.8h12" />
      <path d="M3.6 6.2h.01M3.6 12h.01M3.6 17.8h.01" />
    </>
  ),
  // 周期任务：同一个外框 + 由"粗到细"的内部刻度，
  // 让周/月/季/年看起来是一族，而不是四个无关的符号。
  'period-week': (
    <>
      {PERIOD_FRAME}
      <path d="M6.8 10.6h10.4M8.9 10.6v4M12 10.6v4M15.1 10.6v4" />
    </>
  ),
  'period-month': (
    <>
      {PERIOD_FRAME}
      <circle cx="8.6" cy="8.8" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="12" cy="8.8" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="15.4" cy="8.8" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="8.6" cy="12.2" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12.2" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="15.4" cy="12.2" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="8.6" cy="15.6" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="12" cy="15.6" r="0.95" fill="currentColor" stroke="none" />
      <circle cx="15.4" cy="15.6" r="0.95" fill="currentColor" stroke="none" />
    </>
  ),
  'period-quarter': (
    <>
      {PERIOD_FRAME}
      <path d="M12 6.5v11M6.5 12h11" />
    </>
  ),
  'period-year': (
    <>
      {PERIOD_FRAME}
      <circle cx="12" cy="12" r="4.4" />
    </>
  ),
  projects: (
    <path d="M3 6.8A2.2 2.2 0 0 1 5.2 4.6h3.1a2 2 0 0 1 1.5.7l1.1 1.3h7.9a2.2 2.2 0 0 1 2.2 2.2v8.4a2.2 2.2 0 0 1-2.2 2.2H5.2A2.2 2.2 0 0 1 3 17.2V6.8Z" />
  ),
  tags: (
    <>
      <path d="M11.7 3.6H19a1.6 1.6 0 0 1 1.6 1.6v7.3a2 2 0 0 1-.6 1.4l-6.5 6.5a2 2 0 0 1-2.8 0l-6.2-6.2a2 2 0 0 1 0-2.8l6.5-6.5a2 2 0 0 1 1.3-.3Z" />
      <path d="M16.4 7.6h.01" />
    </>
  ),
  completed: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M8.3 12.3l2.5 2.5 4.9-5.3" />
    </>
  ),
  trash: (
    <>
      <path d="M4 7h16M9.6 7V5.6A1.6 1.6 0 0 1 11.2 4h1.6a1.6 1.6 0 0 1 1.6 1.6V7" />
      <path d="M6.4 7l.8 11.2A2 2 0 0 0 9.2 20h5.6a2 2 0 0 0 2-1.8L17.6 7" />
      <path d="M10.4 11v5.2M13.6 11v5.2" />
    </>
  ),
  stats: (
    <>
      <path d="M3.6 20.2h16.8" />
      <path d="M6.6 20.2V13M12 20.2V5.6M17.4 20.2v-9.6" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7.6h8.4M17.6 7.6h2.4M4 16.4h3.4M11.6 16.4h8.4" />
      <circle cx="15" cy="7.6" r="2.4" />
      <circle cx="9" cy="16.4" r="2.4" />
    </>
  ),

  // ============================ 操作 ============================
  plus: <path d="M12 5.4v13.2M5.4 12h13.2" />,
  minus: <path d="M5.4 12h13.2" />,
  repeat: (
    <>
      <path d="M4 12a8 8 0 0 1 13.4-5.8l2.1 1.9" />
      <path d="M19.8 4.6v4h-4" />
      <path d="M20 12a8 8 0 0 1-13.4 5.8L4.5 15.9" />
      <path d="M4.2 19.4v-4h4" />
    </>
  ),
  download: (
    <>
      <path d="M12 3.8v11.4" />
      <path d="M7.4 10.8 12 15.4l4.6-4.6" />
      <path d="M4.4 19.6h15.2" />
    </>
  ),
  alert: (
    <>
      <path d="M12 4.2 21 19.8H3L12 4.2Z" />
      <path d="M12 10v4.2M12 17h.01" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.6" />
      <path d="M15.8 15.8 20.2 20.2" />
    </>
  ),
  close: <path d="M6.2 6.2 17.8 17.8M17.8 6.2 6.2 17.8" />,
  edit: (
    <>
      <path d="M4.2 19.8h3.9l10-10a1.9 1.9 0 0 0 0-2.7l-1.2-1.2a1.9 1.9 0 0 0-2.7 0l-10 10v3.9Z" />
      <path d="M13.4 7.2l3.4 3.4" />
    </>
  ),
  'chevron-down': <path d="M6.4 9.6 12 15.2l5.6-5.6" />,
  'chevron-up': <path d="M6.4 14.4 12 8.8l5.6 5.6" />,
  copy: (
    <>
      <rect x="8.6" y="8.6" width="11.8" height="11.8" rx="2.6" />
      <path d="M15.4 6.2V5.8a2.2 2.2 0 0 0-2.2-2.2H5.8a2.2 2.2 0 0 0-2.2 2.2v7.4a2.2 2.2 0 0 0 2.2 2.2h.4" />
    </>
  ),
  restore: (
    <>
      <path d="M4.2 12a7.8 7.8 0 1 0 2.4-5.6" />
      <path d="M4 4.4v4.4h4.4" />
    </>
  ),
  pin: (
    <>
      <path d="M9.2 3.8h5.6l-.7 5.3 2.6 2.5H7.3l2.6-2.5-.7-5.3Z" />
      <path d="M12 11.6V20.2" />
    </>
  ),
  ban: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M5.9 5.9 18.1 18.1" />
    </>
  ),
  contrast: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M12 3.4a8.6 8.6 0 0 1 0 17.2V3.4Z" fill="currentColor" stroke="none" />
    </>
  ),
  archive: (
    <>
      <rect x="3.4" y="4.4" width="17.2" height="4.2" rx="1.6" />
      <path d="M5.2 8.6V19a1.6 1.6 0 0 0 1.6 1.6h10.4A1.6 1.6 0 0 0 18.8 19V8.6" />
      <path d="M10.2 12.6h3.6" />
    </>
  ),
  merge: (
    <>
      <path d="M4.4 6.4h5.2a3 3 0 0 1 3 3v5.2a3 3 0 0 0 3 3h2.6" />
      <path d="M15.6 14.6l3 3-3 3" />
      <path d="M4.4 17.6h3.4" />
    </>
  ),
  star: (
    <path d="m12 4.2 2.4 5 5.4.8-3.9 3.8.9 5.4-4.8-2.6-4.8 2.6.9-5.4-3.9-3.8 5.4-.8 2.4-5Z" />
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M12 7.2V12l3.2 2" />
    </>
  ),
  play: <path d="M8.4 5.2v13.6L19 12 8.4 5.2Z" />,
  pause: <path d="M9.6 5.4v13.2M14.4 5.4v13.2" />,
  'folder-open': (
    <>
      <path d="M3 7.4A2 2 0 0 1 5 5.4h3.3l1.7 2h6.6a2 2 0 0 1 2 2" />
      <path d="M3 9.4h17.6l-1.8 9.2a1.6 1.6 0 0 1-1.6 1.3H5.6A1.6 1.6 0 0 1 4 18.6L3 9.4Z" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.6" />
      <path d="M12 11v5.6M12 7.6h.01" />
    </>
  ),

  // ========================= 附件类型 =========================
  // 六种类型共用同一个"纸张"外形（折角 + 内页），只在内部做区分，
  // 这样一列附件看起来是一族图标而不是六种贴纸。
  file: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
    </>
  ),
  image: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
      <circle cx="9.8" cy="12.2" r="1.3" />
      <path d="M6.6 17.6l3.4-3.2 2.4 2.2 2-1.8 2.8 2.6" />
    </>
  ),
  document: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
      <path d="M8.6 12.4h6.8M8.6 15.2h6.8M8.6 17.6h4" />
    </>
  ),
  audio: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
      <path d="M14.6 11.6v5.2" />
      <circle cx="12.9" cy="17" r="1.7" />
      <path d="M14.6 11.6l-2.4.7" />
    </>
  ),
  video: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
      <path d="M10.4 12.2v4.8l4.2-2.4-4.2-2.4Z" />
    </>
  ),
  package: (
    <>
      <path d="M13.6 3.6H7.2A1.8 1.8 0 0 0 5.4 5.4v13.2a1.8 1.8 0 0 0 1.8 1.8h9.6a1.8 1.8 0 0 0 1.8-1.8V8.4l-4.8-4.8Z" />
      <path d="M13.4 3.8v4.6h4.8" />
      <path d="M8.4 12.2h7.2M8.4 14.6h7.2M8.4 17h7.2" />
      <path d="M11.4 11.4v6.4" />
    </>
  ),
}

export interface IconProps {
  name: IconName
  /** 渲染尺寸（像素）。默认 18，与正文一行的视觉重量相当。 */
  size?: number
  className?: string
  /** 线宽。极少数需要更粗/更细的场合才传。 */
  strokeWidth?: number
  /** 传了就作为无障碍名称；不传则纯装饰（aria-hidden） */
  title?: string
}

export function Icon({ name, size = 18, className, strokeWidth = 1.7, title }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      role={title ? 'img' : undefined}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      {title ? <title>{title}</title> : null}
      {PATHS[name]}
    </svg>
  )
}

/**
 * 按 MIME 选附件图标。
 *
 * 与 `☁ 📕 🎵` 那套 emoji 相比，这里只按"用户真正会区分的类别"分：
 * 图片 / 文档 / 音频 / 视频 / 压缩包 / 其它。分得太细反而没人看得懂。
 */
export function iconForMime(mime: string): IconName {
  if (mime.startsWith('image/')) return 'image'
  if (mime.startsWith('audio/')) return 'audio'
  if (mime.startsWith('video/')) return 'video'
  if (mime.includes('zip') || mime.includes('rar') || mime.includes('7z') || mime.includes('tar')) {
    return 'package'
  }
  if (
    mime === 'application/pdf' ||
    mime.includes('word') ||
    mime.includes('excel') ||
    mime.includes('presentation') ||
    mime.startsWith('text/') ||
    mime === 'application/json'
  ) {
    return 'document'
  }
  return 'file'
}
