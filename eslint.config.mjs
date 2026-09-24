// ESLint 扁平配置（ESLint 9+ 的唯一格式）。
//
// 项目里原本有 `pnpm lint` 脚本，但从 ESLint v9 起默认只认 `eslint.config.*`，
// 而仓库里一直没有这个文件——也就是说 `pnpm lint` **一直是坏的**。
// 整改任务书 §8.4 要求"如果 lint 有历史问题应先修复，而不是直接关闭规则"，
// 所以这里补上配置并把它接进 CI。
//
// 规则集的选择原则：只用各插件官方推荐的集合，不额外堆规则。
// 目的是"能长期保持绿灯"——一个常年红灯的检查等于没有检查。

import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'

export default tseslint.config(
  {
    // 构建产物、依赖、Tauri 侧与 Python 工具脚本都不属于前端 lint 范围。
    // `_research*` 是开发期下载的第三方文档与资料，不是本项目的代码，
    // 对它们做 lint 没有意义（里面还有大量第三方的 eslint 注释）。
    ignores: [
      'dist/**',
      'node_modules/**',
      'src-tauri/**',
      'tools/**',
      'coverage/**',
      '_research/**',
      '_research-docs/**',
      // 注意这两个目录名**以点开头**，必须按实际名字写；
      // 上面那两个不带点的写法是无效的（曾经因此被 lint 出一堆第三方报错）
      '.research/**',
      '.research-docs/**',
      '*.config.js',
      '*.config.ts',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      globals: {
        // 浏览器环境（前端产物跑在 WebView2 里）
        window: 'readonly',
        document: 'readonly',
        localStorage: 'readonly',
        console: 'readonly',
        fetch: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        requestAnimationFrame: 'readonly',
        HTMLElement: 'readonly',
        HTMLInputElement: 'readonly',
        HTMLTextAreaElement: 'readonly',
        KeyboardEvent: 'readonly',
        MouseEvent: 'readonly',
        Event: 'readonly',
        DataTransfer: 'readonly',
        DragEvent: 'readonly',
        navigator: 'readonly',
        performance: 'readonly',
        __TAURI_INTERNALS__: 'readonly',
      },
    },
    rules: {
      // react-hooks 的两条核心规则：依赖数组与 Hooks 调用位置。
      // 这两条是真正会引发线上 bug 的，其它风格类规则不开。
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',

      // 允许以 _ 开头的"明确不使用"参数/变量（惯用约定）
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      // any 在少数与 WebView2/第三方打交道的边界上难以避免，降为警告
      '@typescript-eslint/no-explicit-any': 'warn',

      // 界面文案是中文，模板字符串里用全角空格（U+3000）做分隔是**有意的排版**，
      // 例如 `最近检查：…` 前留一个全角空档。默认配置只放行普通字符串，
      // 模板字符串里的会被误报，因此这里显式放行模板与字符串。
      // 注释里的异常空白仍然会报，避免零宽字符悄悄混进代码。
      'no-irregular-whitespace': [
        'error',
        { skipStrings: true, skipTemplates: true, skipComments: false, skipJSXText: true },
      ],
    },
  },
)
