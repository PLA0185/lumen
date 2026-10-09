# Lumen 全界面自查与可读性整改 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让备忘、流程和其他常用页面在 Windows 与 Android 尺寸下分区明确、内容完整、排版紧凑且操作清楚。

**Architecture:** 保留现有记录和 IPC 数据格式，通过两个独立 `ViewId` 将同一个编辑器组件限制为备忘或流程模式；在共享样式上修复正文、空态和响应布局，仅对实机证实的问题做页面修正。完整 Markdown 阅读由页面滚动区承载，短摘要仍保留自身滚动上限。

**Tech Stack:** React 19、TypeScript、CSS、Vitest、Tauri/WebView2、Python CDP 验收脚本。

**Spec:** `docs/superpowers/specs/2026-10-09-ui-audit-remediation.md`

## Global Constraints

- 不变更 `memo_documents` 表、备份 schema、云同步事件或已有任务/热量业务逻辑。
- Windows 导航持续可见；Android 紧凑视图维持五项底栏，流程在主导航，备忘在“更多”列表。
- 正文不得为了卡片整齐而裁掉；只有明确定义为摘要的组件可限制高度。
- 不增加运行时依赖；保留键盘焦点、减少动态效果和触控目标要求。
- 工作记录追加四部分并推送；全门禁先 `pnpm build` 后执行 `cargo clippy --all-targets --all-features -- -D warnings`。

## Review Focus

- 旧用户原有 `memos` 视图设置仍打开备忘页，历史流程可在新流程页找到。
- 备忘草稿切换到流程/其他页面时仍由同一未保存保护拦截。
- 知识库引用流程时打开正确流程和步骤，不受新筛选影响。
- 独立回收站仅显示对应类型，恢复后回到正确的记录列表。
- 小窗口和长标题、长 Markdown、空分类下操作仍完整可见。

---

### Task 1: 分离备忘与流程入口及检索范围

**Files:**
- Modify: `src/lib/types.ts`, `src/components/Sidebar.tsx`, `src/App.tsx`
- Modify: `src/components/MemosView.tsx`, `src/components/MemosView.test.tsx`
- Test: `src/components/Sidebar.test.tsx` (create if none exists)
- Test: `tools/verify_memos.py`

**Interfaces:**
- `MemosView` receives `kindFilter: 'memo' | 'flow'` from the route and applies it to list, category, count, empty text, actions, and trash.
- `ViewId` adds `flows`; existing `memos` remains the memo destination.

- [x] Add tests showing memo mode excludes flows and flow-only actions; flow mode excludes memos and memo-only actions.
- [x] Add the `flows` route, dedicated sidebar entry, route-specific search, Android navigation label/selection, and dirty-draft guard for both routes.
- [x] Route knowledge-base flow references to `flows`; preserve step focus and current query behavior.
- [x] Run targeted tests and real memo/flow route checks, including old memo data and trash.

### Task 2: 修复完整正文、空态留白与无效筛选

**Files:**
- Modify: `src/components/MemosView.tsx`, `src/components/MemosView.test.tsx`, `src/styles.css`
- Modify: `tools/verify_memos.py`

**Interfaces:**
- Shared list loads existing records and filters by `kindFilter`; no backend or persisted format changes.

- [x] Add a real WebView assertion for a long memo whose final paragraph must remain visible without nested clipping.
- [x] Compare the inherited 260px `.mdpreview` cap and 420px empty panel against the corrected natural-height reading layout.
- [x] Let memo and step reading content expand naturally; keep clipping behavior limited to actual preview components.
- [x] Remove the fixed-height empty panel and make memo/flow empty text and actions specific to the current route.
- [x] Hide category filtering while the current record type has no categories; use type-specific empty and trash labels.
- [x] Re-run unit and real WebView checks at desktop and compact widths.

### Task 3: 修正逐页检查中复现的其他显示问题

**Files:**
- Modify: `src/components/KnowledgeBase.tsx`, `src/components/KnowledgeBase.test.tsx`, `src/styles.css`
- Modify: `src/components/BoardView.tsx` or `src/styles.css` and a relevant component/UI test
- Modify: `src/components/NutritionView.tsx`, `src/components/NutritionView.test.tsx`
- Modify: `tools/verify_topbar_layout.py` or add a focused UI layout verification script

**Interfaces:**
- Preserve all existing knowledge answers, citations, board task movement and calorie calculations.

- [x] Add tests showing the knowledge privacy block is scannable with full details available on demand.
- [x] Add a zero-training case that fails on the visible `−0` summary text.
- [x] Reduce the empty board column height without removing its drag/drop target.
- [x] Audit current and modified screenshots for task, calendar, board, organize, knowledge, nutrition, AI, statistics, settings, memo and flow pages at multiple widths; fix only reproduced clipping, overflow, dense copy or misleading empty space.
- [x] Run relevant component tests and actual Chromium/WebView layout checks.

### Task 4: 完整验收、记录和发布

**Files:**
- Modify: `package.json`, versioned app files (if a release version is required)
- Modify: `README.md`, `RELEASE_NOTES.md`, `docs/work-log.md`
- Test: all existing frontend/Rust/real WebView checks

- [x] Execute `pnpm install --frozen-lockfile`, `pnpm typecheck`, `pnpm test`, `pnpm build`, `pnpm lint`.
- [x] Execute `cargo fmt --check`, `cargo test --lib`, and `cargo clippy --all-targets --all-features -- -D warnings` after build.
- [x] Run the complete real WebView audit on isolated data; capture and inspect final screenshots.
- [x] Record verified outcomes and platform limitations in four-part work-log entry; check secrets/build artifacts.
- [x] Commit each concern separately, push to `PLA0185/lumen`, and wait for remote CI before claiming completion.
