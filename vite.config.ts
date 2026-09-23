// 从 `vitest/config` 导入 defineConfig：它扩展了 Vite 的配置类型并支持 `test` 字段。
// 从 'vite' 导入会导致 `test` 报类型错误（vite 自身不认识该字段）。
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

// Tauri 期望前端开发服务器运行在固定端口上，失败即报错而不是静默换端口，
// 否则 tauri.conf.json 里的 devUrl 会指向失效地址。
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react()],

  // Tauri CLI 通过该前缀暴露自身环境变量，前端可用 import.meta.env.TAURI_ENV_*
  envPrefix: ['VITE_', 'TAURI_ENV_'],

  // 保持终端输出干净，让 Tauri / Rust 编译错误不被 Vite 清屏冲掉
  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // src-tauri 由 Rust 侧自己监听，Vite 不必跟着重启
      ignored: ['**/src-tauri/**'],
    },
  },

  build: {
    // Windows 上 Tauri 使用 WebView2(Chromium)，可安全目标到较新的 ES 版本
    target: 'chrome120',
    // 注意：Vite 8 已转向 Rolldown/Oxc，minify 用布尔开关而非 'esbuild' 字符串，
    // 避免绑定到具体压缩器名称。
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        // Vite 8 的 manualChunks 类型契约已改为函数形式；
        // 这里用函数返回 chunk 名，把体积较大的图表库单独拆出。
        manualChunks(id: string) {
          if (id.includes('node_modules/recharts') || id.includes('node_modules/d3-')) {
            return 'charts'
          }
          if (id.includes('node_modules/react-dom') || id.includes('node_modules/react/')) {
            return 'react'
          }
          return undefined
        },
      },
    },
  },

  test: {
    // 领域逻辑（日期、重复规则、校验）用 node 环境即可，无需 DOM
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
})
