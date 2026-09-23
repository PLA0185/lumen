import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles.css'

// 主题：读取用户上次选择（阶段 2 会改为从设置表读取）；
// 当前默认"跟随系统"，由 CSS 的 prefers-color-scheme 处理。
const savedTheme = localStorage.getItem('aitodo.theme')
if (savedTheme === 'light' || savedTheme === 'dark') {
  document.documentElement.dataset.theme = savedTheme
}

// 字体与缩放（§3）：在设置页可调，这里应用已保存的值
const savedScale = localStorage.getItem('aitodo.uiScale')
if (savedScale) {
  document.documentElement.style.setProperty('--ui-scale', savedScale)
}
const savedFontSize = localStorage.getItem('aitodo.fontSize')
if (savedFontSize) {
  document.documentElement.style.setProperty('--font-size-base', savedFontSize)
}
// 动画开关（§3「动画应轻且可关闭」）
if (localStorage.getItem('aitodo.motion') === 'off') {
  document.documentElement.dataset.motion = 'off'
}

const root = document.getElementById('root')
if (!root) {
  throw new Error('找不到 #root 挂载点，index.html 可能损坏')
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
