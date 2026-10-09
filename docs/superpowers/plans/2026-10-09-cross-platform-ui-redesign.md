# Lumen 跨平台 UI 重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一 Lumen 全项目的视觉与交互规则，并为 Android 触控和 Windows 键鼠分别打磨主壳层和主要页面。

**Architecture:** 继续使用现有 React 组件、自绘 SVG 图标和 CSS 令牌，不新增运行时依赖。以共用语义令牌和共享控件状态做全局收敛，再修复 Android/Windows 壳层及用户反复指出的页面具体问题；业务数据流不改。

**Tech Stack:** React 19、TypeScript、CSS、Vitest、Tauri。

**Spec:** `docs/superpowers/specs/2026-10-09-cross-platform-ui-redesign.md`

## Global Constraints

- 保留现有功能、数据和同步协议；本轮只改 UI、交互状态和对应可访问性。
- 不复制 Apple 专有材料、SF Symbols 或 Liquid Glass；使用指南中的通用体验原则。
- Android 紧凑窗口保留 3–5 项底部主导航，宽窗口切换侧边导航；交互目标不小于 44 CSS px。
- Windows 保留侧边栏，命令区在窄窗口可见、可用，不裁切关键操作。
- 复用现有 CSS 变量、Icons SVG、自测工具与依赖；测试必须先复现或失败，再实施最小修复。
- 通过仓库规定的冻结安装、类型检查、前端测试/构建/lint、Rust 格式/测试/clippy 门禁。

## File Map

- `src/styles.css`: 颜色、排版、间距、通用控件、侧栏/顶栏及共用页面表面。
- `src/nutrition.css`: 保留饮食页面规则；平台外壳移至 `src/platform.css`。
- `src/platform.css`: Windows/Android 导航、窄宽屏断点、安全区和触控目标。
- `src/App.tsx` / `src/components/Sidebar.tsx`: 壳层、导航语义、紧凑导航与 Android 更多入口。
- `src/components/FlowCanvas.tsx` 和对应测试: 流程导航节点的当前/悬停层级和间距回归。
- `src/components/KnowledgeBase.tsx`、`MemosView.tsx`、`SettingsView.tsx` 与样式: 易读布局、信息状态、长答案/长文件列表。
- `src/components/Icons.tsx`: 仅在图标覆盖不足或视觉基线不一致时扩充/修正现有 SVG 集。
- `src/**/*.test.*`: 平台壳层、交互状态和用户历史问题的行为回归。
- `tools/verify_topbar_layout.py`: 真实 Chromium/Tauri WebView 顶栏与 Android 响应布局验收。
- `docs/work-log.md`: 追加本轮四部分记录；README 文档索引链接设计说明。

## Tasks

### Task 1: 建立跨平台视觉回归基线

- [x] 给 Android 次级页面的“更多”导航补选中语义回归测试；新断言在修复前因入口不存在而失败。
- [x] 用真实 Chromium 尺寸矩阵验证 Windows 顶栏换行、标题不重叠、控件不裁切；以窄/宽屏 CSS 验证 Android 导航和安全区。
- [ ] 没有对每个页面逐一新增独立视觉快照；共享样式统一生效，个人页面逐屏实机视觉审查不在本轮自动化能力内。

### Task 2: 收敛视觉令牌和通用控件状态

- [x] 调整亮/暗主题的语义表面、正文/次级文本对比度、边框与强调色用法。
- [x] 统一共用标题、说明、间距、圆角及桌面/触控控件高度。
- [x] 加强点击、键盘焦点、按下状态反馈，并遵守系统减少动态效果及应用关闭动画设置。
- [ ] 未连接屏幕阅读器或实机逐项核验所有主题和自定义主题配色。

### Task 3: 重做 Windows 与 Android 应用壳层

- [x] Windows 顶栏命令区按可用宽度换行，标题、搜索和命令不互相覆盖。
- [x] Android 紧凑屏提供 5 项主导航和更多入口，宽屏切换侧栏；统一安全区、48px 触控高度及底栏留白。
- [x] 侧栏内容自然滚动；有溢出时可滚，无溢出时不人为强制滚动。
- [x] 运行导航回归与 Windows/Android 宽窄屏布局验收。

### Task 4: 修正主要页面层级与状态反馈

- [x] 全局语义令牌和控件规则统一作用于任务、日历、看板、组织、流程、知识库、饮食、AI、统计和设置页面；没有重写已有业务空/加载/错误状态。
- [x] 知识库答案采用限宽阅读卡片，依据区独立分隔并限制高度；此前版本已有导入进度和结果分组逻辑，本轮保留。
- [x] 保留资料卡已去重/折叠、流程导航当前/悬停状态的既有修复。
- [ ] 未对所有页面逐项做像素级重排或连接 Android 实机检查；已验证的响应布局和覆盖范围见实施记录。

### Task 5: 视觉验收与收尾

- [x] 在隔离数据目录的 Windows Tauri/WebView2 运行环境中检查真实应用截图，并用 Chromium 实际排版测量 48 组标题和命令组合。
- [x] 以真实 Chromium 的 Android CSS 视口验证 390、760、900、1280px 导航切换、触控目标和底栏留白；实体 Android 未连接。
- [x] 执行完整 AGENTS.md 门禁，处理所有失败；远端主分支 CI 与签名 Release 流程均成功。
- [x] 更新 README 文档索引与 `docs/work-log.md`；检查敏感文件和差异，按类别提交并推送。
