import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { FloatingToday, QuickAddWindow } from './components/FloatingToday'
import './styles.css'

// 主题：读取用户上次选择（默认"跟随系统"，由 CSS 的 prefers-color-scheme 处理）
const savedTheme = localStorage.getItem('lumen.theme')
if (savedTheme === 'light' || savedTheme === 'dark') {
  document.documentElement.dataset.theme = savedTheme
}

// 字体与缩放（§3）：在设置页可调，这里应用已保存的值
const savedScale = localStorage.getItem('lumen.uiScale')
if (savedScale) {
  document.documentElement.style.setProperty('--ui-scale', savedScale)
}
const savedFontSize = localStorage.getItem('lumen.fontSize')
if (savedFontSize) {
  document.documentElement.style.setProperty('--font-size-base', savedFontSize)
}
// 动画开关（§3「动画应轻且可关闭」）
if (localStorage.getItem('lumen.motion') === 'off') {
  document.documentElement.dataset.motion = 'off'
}

/**
 * 按窗口类型选择要渲染的界面。
 *
 * 三个窗口（主窗口 / 悬浮今日 / 快速添加）共用同一份前端产物，
 * 由窗口标签区分要渲染什么——这样只需构建一次，也避免了为每个窗口
 * 维护独立的 HTML 入口。
 *
 * 标签来源：`__TAURI_INTERNALS__.metadata.currentWebview.label`。
 * 注意是 `currentWebview` 而不是 `currentWindow`——后者在 Tauri 2
 * 的类型定义里存在但结构不同，写错会静默回落到主界面，
 * 表现为"悬浮窗里显示的是整个主界面"。
 */
function currentWindowLabel(): string {
  try {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__?: {
          metadata?: { currentWebview?: { label?: string }; currentWindow?: { label?: string } }
        }
      }
    ).__TAURI_INTERNALS__
    return (
      internals?.metadata?.currentWebview?.label ??
      internals?.metadata?.currentWindow?.label ??
      'main'
    )
  } catch {
    // 非 Tauri 环境或内部结构变化时回落到主界面（开发时在浏览器预览的常见情形）
    return 'main'
  }
}

function pickRoot(): React.ComponentType {
  switch (currentWindowLabel()) {
    case 'floating':
      return FloatingToday
    case 'quick-add':
      return QuickAddWindow
    default:
      return App
  }
}

const Root = pickRoot()

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('找不到 #root 挂载点，index.html 可能损坏')
}

// 给 body 打上窗口类型，便于 CSS 针对不同窗口调整（例如去掉外边距）
document.body.dataset.window = Root === FloatingToday ? 'floating' : Root === QuickAddWindow ? 'quick-add' : 'main'

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
)
