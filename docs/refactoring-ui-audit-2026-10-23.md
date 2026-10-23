# Lumen 全项目界面审查与整改（2026-10-23）

本轮按 `refactoring-ui` 的审查顺序检查层级、布局与间距、文字、颜色、深度、图像和收尾状态。范围是整个前端界面与跨平台壳层，不涉及 Rust 业务逻辑。

## 审查范围与证据

- 逐页切换并检查 28 个主导航视图：今天、明天、本周安排、收件箱、全部任务、已完成、回收站；每日/每周/每月/每季度/每年重复与周/月/季度/年跨度任务；日历、看板、备忘、流程、知识库问答、场景便签、食谱与采购、AI 助手、项目与分类、标签、统计、设置。
- 用隔离浏览器上下文和临时合成数据渲染，不连接用户数据库、不写用户文件、不请求外部 AI。另检查快速新建、任务卡片更多操作、统计表单、流程空态和代表性 AI / 导入弹窗。
- 视口：Android UA 的 375×812 竖屏、812×375 横屏；Windows UA 的 1280×800 与 958×780。另检查浅色和深色主题。
- 在所有 28 个视图的手机竖屏与横屏复查 document/body 横向宽度；目标宽度与视口相等，没有页面级横向溢出。图表和画布保留自身需要的内部滚动。
- 用 Chrome Lighthouse 手机快照检查统计页：无障碍 100 分。其余 SEO 项（桌面应用没有 robots.txt / llms.txt）不适用；“阻止粘贴”审计指向统计目标名称输入框，但源码中没有 paste 拦截处理，未能复现，作为工具误报待确认。
- 颜色比值由实际 CSS 令牌计算。Android 视口是浏览器模拟，不是 OnePlus Ace 2 实机。

## Findings 与整改

| # | Lens | Sev | Location | Rule | Now | Proposed | Effort | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | layout | P0 | `src/lib/appearance.ts` 自适应缩放 | §3.3.1、§3.5 | 窗口越窄，应用整体越小；WebView 缩放绕过 CSS 断点，手机控件与文字一起缩小。 | 移除按窗口宽度和 DPI 叠加的缩放，只保留用户明确设置的缩放；窄屏由响应式布局接管。 | L | rendered |
| 2 | layout | P0 | `src/styles.css` `.tasklist` | §3.6 | 默认列表左缩进把任务卡片整体推离内容边界，造成明显空列。 | 重置列表内外边距，让任务卡片与清单标题对齐。 | S | rendered |
| 3 | layout | P0 | `src/platform.css` Android `.task__check` | Beyond the Book: Target sizes ✚ | 全局 48px 触控规则把 19px 勾选圆视觉拉成椭圆。 | 保留 44px 圆形触控目标，用独立 19px 圆形绘制状态；完成状态仍有勾选图形。 | M | rendered |
| 4 | hierarchy | P0 | `src/components/TaskCard.tsx`、`src/platform.css` | §2.8；Beyond the Book: Target sizes ✚ | Android 卡片操作依赖悬停，横屏触控设备上操作不可见；竖屏常驻四个按钮又占据卡片高度。 | 所有 Android 宽度均显示“更多”触控入口；点开后再显示 44×44 的复制、编辑、详情、删除操作，并暴露展开状态。 | M | rendered |
| 5 | layout | P0 | `src/platform.css` Android 日历 | §3.3.1、§3.4.1 | 月历固定宽度的 7 列日期卡片在 375px 下挤出视口，日期与任务文字难读。 | 使用 7 个等分最小宽度网格；窄屏显示日期与任务数，点日期进入当天视图后再新建。 | M | rendered |
| 6 | layout | P1 | `src/platform.css` 移动端顶栏与 `src/App.tsx` 工具栏 | §2.8、§3.3.1 | 任务工具会挤成多行；横屏顶栏曾占 225px；无关页面还出现不适用的任务操作。 | 竖屏按标题/常用操作/搜索分层；矮屏横屏改为两行；只在适用视图显示新建、筛选、排序、搜索和导出。 | L | rendered |
| 7 | typography | P1 | `src/platform.css` `.topbar__subtitle` | §4.3、§4.3.1 | 周安排、周/月/季度任务的说明文字在窄屏被单行截断。 | 手机竖屏与横屏允许说明换行，保留完整文字。 | S | rendered |
| 8 | layout | P1 | `src/platform.css` `.memos__toolbar` | §3.3.1；Beyond the Book: Target sizes ✚ | 备忘/流程工具栏按比例缩小按钮以塞进一行，标签过小且触控面积不足。 | 改成两列自然换行，按钮保持可读字号和至少 44px 高度。 | M | rendered |
| 9 | layout | P1 | `src/components/QuickAdd.tsx`、`src/platform.css` | §3.6 | 日期、时间、优先级、周期和重复字段没有稳定列对齐；长表单的添加按钮被后置 AI 区域推远。 | 字段按两列对齐、字段名置于控件上方；在完整任务字段后提供第二组添加/取消按钮，再显示 AI 辅助。 | M | rendered |
| 10 | color | P0 | `src/styles.css` `--c-accent` 文本用法 | §5.6 | 深色主题的普通强调文字使用 `#4f46e5`，在卡片表面上仅 2.55:1。 | 增加语义色文字令牌：深色 `#a5b4fc`、浅色 `#4338ca`；背景填充仍使用现有主色。两主题在表面上的比值分别为 8.03:1、7.90:1。 | M | rendered |
| 11 | hierarchy | P1 | `src/components/StatsView.tsx` | §2.6 | 统计页从 h1 直接跳到 h3，成就标题又跳到 h4，标题结构不连续。 | 分区标题改为 h2、成就标题改为 h3，视觉字号仍由现有类控制。Lighthouse 手机无障碍从 98 提到 100。 | S | rendered |
| 12 | layout | P1 | `src/platform.css` 统计筛选栏 | §3.4.2；Beyond the Book: Target sizes ✚ | 窄屏把四个范围和刷新按钮塞在一行，刷新按钮被裁，筛选标签缩得太小。 | 四个范围按钮按四列等宽显示，刷新单独放在下一行；保留完整标签与 48px 高度。 | M | rendered |
| 13 | hierarchy | P1 | `src/styles.css`、`src/platform.css`、`src/nutrition.css`、`src/components/scene-notes.css` | §2.2.b | 正常文字和强调文字混用 400、500、550、600、650、700 六档字重，细微档位差不容易形成稳定层级。 | 把普通文字统一为 400、强调文字统一为 600，再用字号和文字颜色补足层级。 | M | rendered |

## 覆盖矩阵

| 页面族 | 覆盖页面 | 结果 |
| --- | --- | --- |
| 任务清单 | 今天、明天、本周安排、收件箱、全部任务、已完成、回收站 | 竖屏/横屏无页面级横向溢出；卡片、说明和移动端操作入口已核对 |
| 重复与跨度任务 | 5 种重复视图、周/月/季度/年跨度视图 | 竖屏/横屏无页面级横向溢出；长标题说明可换行 |
| 日期与状态 | 日历、看板 | 竖屏/横屏无页面级横向溢出；月历窄屏采用日期点选进入当天 |
| 整理与资料 | 备忘、流程、知识库问答、场景便签 | 竖屏/横屏无页面级横向溢出；备忘/流程操作栏可读 |
| 专项页面 | 食谱与采购、AI 助手、项目与分类、标签、统计、设置 | 竖屏/横屏无页面级横向溢出；统计筛选和设置分组按窄屏排列 |
| 共用状态 | 快速新建、任务编辑、卡片更多操作、AI 助手弹窗、ChatGPT 导入弹窗、流程/资料空态 | 检查布局、对齐、触控面积、滚动边界、标签与空状态 |

## 验证与未覆盖项

- `pnpm install --frozen-lockfile`、`pnpm typecheck`、`pnpm test`（57 文件、421 项）、`pnpm build`、`pnpm lint` 均退出码为 0。构建仅有既存的 dynamic import 分块提示。
- `cargo fmt --check` 与 `cargo clippy --all-targets --all-features -- -D warnings` 退出码为 0。`cargo test --lib` 输出 620 passed、4 ignored、0 failed；测试汇总后进程持续运行，等待 60 秒仍未退出，手动中断后命令退出码为 1，因此不记作完整通过。
- `git diff --check` 通过。浏览器使用合成数据；未对真实数据库或用户内容执行写入。
- 本机没有 Java 与 Android SDK，未构建 Android APK；未在 OnePlus Ace 2 / Android 16 实机上验收。Android 与 Windows 共享本轮前端代码，但本轮没有发布 APK。
- 未逐一操作所有破坏性确认、系统文件选择器和原生窗口状态；这些行为依赖 Tauri 原生环境，浏览器模拟不能代替真机验收。
