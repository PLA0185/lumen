# 依赖与第三方导入格式调研（Tauri 2 + React + TypeScript + SQLite / Windows 桌面 Todo）

> **调研日期（全部访问日期）：2026-09-23**
> **方法**：版本号一律取自 `registry.npmjs.org` / `crates.io` 的 `dist-tags.latest`（机器可读的一手来源），许可证取自同一响应；行为/格式结论一律取自官方文档或官方仓库原始文件。凡官方来源未明确记载者，标注 **未核实**，不作推断。
> **文件命名说明**：本文件**未**写入 `docs/api-research.md`，因为该路径已被另一份《AI 适配层 API 事实核查（DeepSeek / OpenAI / Anthropic）》占用。本文件为其同级文件，如需合并请由人工决定。

---

## A. 前端核心与构建

### A.1 核心版本（npm `latest` dist-tag）

| 库 | 确切版本 | 发布日期 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `react` | **19.3.0** | 2026-09-09 | MIT | <https://registry.npmjs.org/react/latest> · <https://react.dev/> | 2026-09-23 |
| `react-dom` | **19.3.0** | 2026-09-09 | MIT | <https://registry.npmjs.org/react-dom/latest> | 2026-09-23 |
| `vite` | **8.3.0** | 2026-09-10 | MIT | <https://registry.npmjs.org/vite/latest> · <https://vite.dev/> | 2026-09-23 |
| `typescript` | **7.0.2** | 2026-07-08 | Apache-2.0 | <https://registry.npmjs.org/typescript/latest> · <https://www.typescriptlang.org/> | 2026-09-23 |
| `@vitejs/plugin-react` | **6.1.1** | 2026-08-28 | MIT | <https://registry.npmjs.org/@vitejs/plugin-react/latest> | 2026-09-23 |
| `@vitejs/plugin-react-swc` | **4.3.3** | 2026-07-30 | MIT | <https://registry.npmjs.org/@vitejs/plugin-react-swc/latest> | 2026-09-23 |

> **注意 React 19.3.0 与 Vite 8.3.0 的发布日期是 2026-09-09 / 09-10**，即调研前约两周，属当前活跃发布期。

### A.2 `@vitejs/plugin-react` vs `@vitejs/plugin-react-swc`：推荐选择

| 项目 | `@vitejs/plugin-react` 6.1.1 | `@vitejs/plugin-react-swc` 4.3.3 |
| --- | --- | --- |
| 许可证 | MIT | MIT |
| peerDependencies（原文） | `vite: ^8.0.0`、`oxc-transform-react: ^0.145.0`、`@rolldown/plugin-babel: ^0.1.7 \|\| ^0.2.0`、`babel-plugin-react-compiler: ^1.0.0` | `vite: ^4 \|\| ^5 \|\| ^6 \|\| ^7 \|\| ^8` |
| Vite 8 适配 | 专为 Vite 8 声明 | 兼容面更宽（含旧版 Vite） |
| 官方模板是否采用 | **是**（`create-tauri-app` 的 `template-react-ts` 使用 `@vitejs/plugin-react ^6.0.2`） | 否（Tauri 官方 React 模板未使用） |
| 官方 URL | <https://www.npmjs.com/package/@vitejs/plugin-react> | <https://www.npmjs.com/package/@vitejs/plugin-react-swc> |
| 访问日期 | 2026-09-23 | 2026-09-23 |

**推荐结论：选 `@vitejs/plugin-react`。** 依据是 Tauri 官方 React+TS 模板实际依赖它（见 A.3），且其 peer 声明已对齐 Vite 8 / Rolldown / Oxc 技术栈；`plugin-react-swc` 的 peer 范围仍回溯到 Vite 4，说明它是兼容性优先的旧通道。若追求最短冷启动可另行评估 SWC 版本，但会偏离官方模板基线。

### A.3 Tauri 官方 Vite + React 模板配置（逐字）

**官方模板 URL（可点击）**
- 模板 `vite.config.ts`：<https://github.com/tauri-apps/create-tauri-app/blob/dev/templates/template-react-ts/vite.config.ts.lte>
- 模板 `package.json`：<https://github.com/tauri-apps/create-tauri-app/blob/dev/templates/template-react-ts/package.json.lte>
- 模板 `.manifest`（定义 `devUrl`）：<https://github.com/tauri-apps/create-tauri-app/blob/dev/templates/template-react-ts/.manifest>
- 官方集成指南：<https://v2.tauri.app/start/frontend/vite/>
- 访问日期：2026-09-23

**模板原始内容（`vite.config.ts.lte`，去掉模板语法后）**

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react()],
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
```

**关键配置项核对表**

| 配置项 | 官方模板值 | 说明 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- |
| `clearScreen` | **`false`** | 防止 Vite 清屏遮蔽 Rust 编译错误 | 模板文件 | 2026-09-23 |
| `server.port` | **`1420`** | 必须与 `tauri.conf.json` 的 `devUrl` 一致 | 模板 `.manifest`：`devUrl = http://localhost:1420` | 2026-09-23 |
| `server.strictPort` | **`true`** | 端口被占用时直接失败，不静默换端口 | 模板文件 | 2026-09-23 |
| `server.host` | `process.env.TAURI_DEV_HOST \|\| false` | iOS/移动真机调试时由 Tauri 注入 | 模板文件 | 2026-09-23 |
| `server.hmr` | `{ protocol: "ws", host, port: 1421 }`（仅当 `TAURI_DEV_HOST` 存在） | HMR 走 1421 | 模板文件 | 2026-09-23 |
| `server.watch.ignored` | `["**/src-tauri/**"]` | 避免 Rust 产物触发前端重载 | 模板文件 | 2026-09-23 |
| `envPrefix` | **模板中不存在** | 仅出现在官方「Vite」集成指南页，值为 `['VITE_', 'TAURI_ENV_*']` | <https://v2.tauri.app/start/frontend/vite/> | 2026-09-23 |
| `build.target` | **模板中不存在** | 仅指南页有：`process.env.TAURI_ENV_PLATFORM == 'windows' ? 'chrome105' : 'safari13'` | 同上 | 2026-09-23 |
| `build.minify` / `build.sourcemap` | **模板中不存在** | 仅指南页有：`!process.env.TAURI_ENV_DEBUG` / `!!process.env.TAURI_ENV_DEBUG` | 同上 | 2026-09-23 |
| 指南页示例端口 | **`5173`** | ⚠️ 与模板的 **1420** 不一致；以模板 + `tauri.conf.json` 的 `devUrl` 为准 | 同上 | 2026-09-23 |

> **两个必须注意的坑**
> 1. **端口二义性**：官方文档页示例写 `5173`，而官方模板与 `tauri.conf.json` 写 `1420`。二者都能工作，但**必须三处一致**：`vite.config.ts` 的 `server.port`、`tauri.conf.json` 的 `build.devUrl`、以及 `beforeDevCommand` 启动的 dev server。本项目建议统一取模板值 **1420**。
> 2. **TS 版本落差**：官方模板 `devDependencies` 锁的是 **`typescript: ~6.0.3`**，而 npm 当前 `latest` 是 **7.0.2**。官方模板**尚未跟进 TS 7**。若直接升到 TS 7 将脱离官方基线，建议先按 `~6.0.3` 起步，单独评估 TS 7 迁移。

**模板 `package.json` 的完整依赖基线（原文）**

```jsonc
"dependencies": {
  "react": "^19.1.0",
  "react-dom": "^19.1.0",
  "@tauri-apps/api": "^2",
  "@tauri-apps/plugin-opener": "^2"
},
"devDependencies": {
  "@types/react": "^19.1.8",
  "@types/react-dom": "^19.1.6",
  "@vitejs/plugin-react": "^6.0.2",
  "typescript": "~6.0.3",
  "vite": "^8.0.16",
  "@tauri-apps/cli": "^2"
}
```

### A.4 Tauri 本体版本（供锁定参照）

| 包 / crate | 确切版本 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- |
| `@tauri-apps/cli`（npm `latest`） | 2.11.5（2026-09-20） | Apache-2.0 OR MIT | <https://registry.npmjs.org/@tauri-apps/cli/latest> | 2026-09-23 |
| `@tauri-apps/api`（npm `latest`） | 2.11.1（2026-06-17） | Apache-2.0 OR MIT | <https://registry.npmjs.org/@tauri-apps/api/latest> | 2026-09-23 |
| `tauri`（crates.io 最新稳定） | 2.11.6（2026-09-21） | MIT OR Apache-2.0 | <https://crates.io/crates/tauri> | 2026-09-23 |
| `tauri-build`（crates.io 最新稳定） | 2.6.3（2026-09-21） | MIT OR Apache-2.0 | <https://crates.io/crates/tauri-build> | 2026-09-23 |

---

## B. 具体功能库

### B.0 总表

| 库 / 包 | 确切版本 | 发布日期 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `date-fns` | **4.4.0** | 2026-05-29 | MIT | <https://registry.npmjs.org/date-fns/latest> · <https://date-fns.org/> | 2026-09-23 |
| `dayjs` | **1.11.23** | 2026-08-17 | MIT | <https://registry.npmjs.org/dayjs/latest> · <https://day.js.org/> | 2026-09-23 |
| `@date-fns/tz` | **1.5.0** | — | MIT | <https://registry.npmjs.org/@date-fns/tz/latest> | 2026-09-23 |
| `@date-fns/utc` | **2.1.1** | — | MIT | <https://registry.npmjs.org/@date-fns/utc/latest> | 2026-09-23 |
| `date-fns-tz` | **3.2.0** | — | MIT | <https://registry.npmjs.org/date-fns-tz/latest> · <https://github.com/marnusw/date-fns-tz> | 2026-09-23 |
| `rrule`（rrule.js） | **2.8.1** | 2023-11-10 | BSD-3-Clause | <https://registry.npmjs.org/rrule/latest> · <https://github.com/jakubroztocil/rrule> | 2026-09-23 |
| `@dnd-kit/core` | **6.3.1** | 2024-12-05 | MIT | <https://registry.npmjs.org/@dnd-kit/core/latest> | 2026-09-23 |
| `@dnd-kit/sortable` | **10.0.0** | 2024-12-04 | MIT | <https://registry.npmjs.org/@dnd-kit/sortable/latest> | 2026-09-23 |
| `@dnd-kit/react` | **0.5.0**（`beta` = 0.5.1-beta-20260912195958） | 2026-09-12（beta） | MIT | <https://registry.npmjs.org/@dnd-kit/react> | 2026-09-23 |
| `zustand` | **5.0.15** | 2026-08-13 | MIT | <https://registry.npmjs.org/zustand/latest> · <https://zustand.docs.pmnd.rs/> | 2026-09-23 |
| `zod` | **4.6.5** | 2026-09-13 | MIT | <https://registry.npmjs.org/zod/latest> · <https://zod.dev/> | 2026-09-23 |
| `react-markdown` | **10.1.0** | 2025-03-07 | MIT | <https://registry.npmjs.org/react-markdown/latest> · <https://github.com/remarkjs/react-markdown> | 2026-09-23 |
| `remark-gfm` | **4.0.1** | 2025-02-10 | MIT | <https://registry.npmjs.org/remark-gfm/latest> | 2026-09-23 |
| `rehype-sanitize` | **6.0.0** | 2023-08-26 | MIT | <https://registry.npmjs.org/rehype-sanitize/latest> · <https://github.com/rehypejs/rehype-sanitize> | 2026-09-23 |
| `recharts` | **3.10.1** | 2026-07-25 | MIT | <https://registry.npmjs.org/recharts/latest> · <https://recharts.org/> | 2026-09-23 |
| `@fullcalendar/react` | **7.1.0** | 2026-09-05 | MIT | <https://registry.npmjs.org/@fullcalendar/react/latest> · <https://fullcalendar.io/> | 2026-09-23 |
| `react-big-calendar` | **1.20.0** | 2026-06-01 | MIT | <https://registry.npmjs.org/react-big-calendar/latest> | 2026-09-23 |
| `@tanstack/react-virtual` | **3.14.13** | 2026-09-14 | MIT | <https://registry.npmjs.org/@tanstack/react-virtual/latest> · <https://tanstack.com/virtual> | 2026-09-23 |
| `eslint` | **10.11.0** | 2026-09-18 | MIT | <https://registry.npmjs.org/eslint/latest> · <https://eslint.org/> | 2026-09-23 |
| `prettier` | **3.9.8** | — | MIT | <https://registry.npmjs.org/prettier/latest> · <https://prettier.io/> | 2026-09-23 |
| `vitest` | **5.0.1** | 2026-09-15 | MIT | <https://registry.npmjs.org/vitest/latest> · <https://vitest.dev/> | 2026-09-23 |
| `playwright` / `@playwright/test` | **1.63.0** | 2026-09-04 | Apache-2.0 | <https://registry.npmjs.org/playwright/latest> · <https://playwright.dev/> | 2026-09-23 |

### B.1 日期处理：date-fns vs dayjs

**包体积**（来源：npm registry 各包 `dist.unpackedSize`，访问日期 2026-09-23）

| 包 | 版本 | `unpackedSize`（解压后总字节） | 运行时依赖 |
| --- | --- | --- | --- |
| `date-fns` | 4.4.0 | **10,902,084 B ≈ 10.9 MB** | 无 |
| `dayjs` | 1.11.23 | **681,693 B ≈ 682 KB** | 无 |
| `rrule` | 2.8.1 | 687,245 B ≈ 688 KB | `tslib` |

> **口径提醒**：`unpackedSize` 是**发布包解压后的总大小**，不等于打包进前端 bundle 的体积。`date-fns` 是逐函数 ESM 模块集合，可 tree-shake，实际进入 bundle 的通常只有用到的几十个函数；`dayjs` 是单一内核 + 插件。两者**不能直接用该数字比较产物体积**。若需精确 bundle 体积，须在目标工程内实测（本次**未核实**具体压缩后体积）。

**时区支持方式**

| 方案 | 版本 | 许可证 | 机制 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `@date-fns/tz` | 1.5.0 | MIT | date-fns 官方时区工具包，提供 `TZDate` / `tz` 上下文 | <https://registry.npmjs.org/@date-fns/tz/latest> | 2026-09-23 |
| `@date-fns/utc` | 2.1.1 | MIT | 官方 UTC 工具包（`UTCDate`） | <https://registry.npmjs.org/@date-fns/utc/latest> | 2026-09-23 |
| `date-fns-tz` | 3.2.0 | MIT | 第三方（`marnusw/date-fns-tz`），peer `date-fns: ^3.0.0 \|\| ^4.0.0`，基于 `Intl` API | <https://github.com/marnusw/date-fns-tz> | 2026-09-23 |
| `dayjs` 插件 | 随 `dayjs` 1.11.23 内置分发 | MIT | `utc` + `timezone` 插件，需 `dayjs.extend()` 显式启用 | <https://day.js.org/docs/en/plugin/timezone> | 2026-09-23 |

> **本项目建议**：date-fns v4 时代**优先用官方 `@date-fns/tz`**，而不是第三方的 `date-fns-tz`——官方包与 date-fns 4.x 同源、无 peer 版本漂移风险。`date-fns-tz` 的 README 仍自述为 "for date-fns v2/v3"，属兼容性通道。若选 dayjs，则时区必须靠插件，且插件能力弱于 `Intl` 原生方案。

### B.2 重复规则：rrule.js

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 当前版本 | **2.8.1**（2023-11-10 发布，此后 npm 无新版本） | <https://registry.npmjs.org/rrule> | 2026-09-23 |
| 许可证 | **BSD-3-Clause** | 同上 | 2026-09-23 |
| 是否支持 RRULE 标准 | **是**。README 原文：*"rrule.js supports recurrence rules as defined in the iCalendar RFC"*，并指向 RFC 5545 | <https://github.com/jakubroztocil/rrule> | 2026-09-23 |
| 实现来源 | *"It is a partial port of the `rrule` module from the excellent python-dateutil library"* —— **partial port（部分移植）** | 同上 | 2026-09-23 |
| **`BYSETPOS` 支持** | **支持**。源码 `src/types.ts` 中 `Options` 定义含 `bysetpos: number \| number[] \| null`，`ParsedOptions` 含 `bysetpos: number[]`；`src/rrule.ts` 中默认值 `bysetpos: null`。因此「第 N 个星期 X」（如 `FREQ=MONTHLY;BYDAY=MO;BYSETPOS=2`）**可以实现** | <https://github.com/jakubroztocil/rrule/blob/master/src/types.ts>、<https://github.com/jakubroztocil/rrule/blob/master/src/rrule.ts> | 2026-09-23 |
| `BYSETPOS` 文档化程度 | ⚠️ **README 中完全未提及 `bysetpos`**（全文检索无命中），只能从类型定义与源码确认。属"能用但无官方文档"状态 | <https://github.com/jakubroztocil/rrule/blob/master/README.md> | 2026-09-23 |
| 已知限制：时区语义 | README「Important: Use UTC dates」原文：默认处理 **"floating" times 或 UTC 时区**，*"this library simply doesn't use it at all"*（不使用 JS 内建时区偏移），返回值的「UTC」须被解释为**本地时间**，可能需要额外转换。支持用 `DTSTART;TZID=...` 解析 | README 同上 | 2026-09-23 |
| 已知限制：README 断链 | README 正文引用锚点 `#differences-from-icalendar-rfc`（声称有"与 iCalendar RFC 的若干重要差异"章节），但**该章节在当前 README 中已不存在**（全文检索 `Differences from iCalendar` 无命中，`## Differences...` 标题缺失）。故**具体差异清单无法从官方 README 核实** | README 同上 | 2026-09-23 |
| 月末 / 闰年行为 | **未核实**。官方 README、类型定义均未记载短月（如 1/31 → 2 月）或闰年 2/29 的处理规则；仓库 issue 检索接口返回 422，未能取得权威结论 | — | 2026-09-23 |
| 维护状态 | ⚠️ **基本停滞**。npm 最后发布 2.8.1 = 2023-11-10；GitHub 默认分支最后提交 = **2023-11-10**（`pushed_at` 为 2024-06-27，但最近 5 次提交全部在 2023-11-10）；未归档；3,744 stars；**214 个 open issues** | <https://api.github.com/repos/jakubroztocil/rrule> | 2026-09-23 |
| 生态定位 | README 自述为 [python-dateutil](https://labix.org/python-dateutil) `rrule` 模块的**部分**移植；另有自然语言互转（`toText` / `fromText`） | README 同上 | 2026-09-23 |

**结论**：rrule.js 是**唯一成熟的 JS RRULE 实现**，能满足「第 N 个星期 X」（BYSETPOS）需求，但处于**停止维护**状态（近 3 年无发布、214 个未决 issue），且关键边界行为（月末/闰年）无官方文档保证。**建议**：把它当作"能跑但不再演进"的依赖锁定版本使用，并在业务层对月末/闰年用例**自行补充单元测试**；Rust 侧 `rrule` crate 的成熟度对比见 §D.2。

### B.3 拖拽：dnd-kit

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `@dnd-kit/core` 版本 | **6.3.1**（2024-12-05） | <https://registry.npmjs.org/@dnd-kit/core> | 2026-09-23 |
| `@dnd-kit/sortable` 版本 | **10.0.0**（2024-12-04） | <https://registry.npmjs.org/@dnd-kit/sortable> | 2026-09-23 |
| 许可证 | MIT（两者） | 同上 | 2026-09-23 |
| React 19 兼容性（声明层面） | `@dnd-kit/core` peer = `react: >=16.8.0`、`react-dom: >=16.8.0`；`@dnd-kit/sortable` peer = `react: >=16.8.0`、`@dnd-kit/core: ^6.3.0`。**`>=16.8.0` 已涵盖 React 19**，安装不会报 peer 冲突 | 同上 | 2026-09-23 |
| `dist-tags` | `@dnd-kit/core`：`latest=6.3.1`，`next=6.3.1-next-202411517925`（**同为 2024-12-05**）。`@dnd-kit/sortable`：`latest=10.0.0`，`next=10.0.0-next-202410244445`（2024-11-24）。**两个通道都停在 2024 年** | 同上 | 2026-09-23 |
| **维护状态** | ⚠️ **仓库活跃、但经典包无发布**。GitHub `clauderic/dnd-kit`：`pushed_at = 2026-09-12`，未归档，17,662 stars，129 open issues；最近提交（2026-09-12）为 `fix(react): retain signal subscriptions across renders`、`fix: subscribe to late signal reads in Vue, Solid, and Svelte` —— 说明开发重心已转向**新架构**，而非 `core`/`sortable` | <https://api.github.com/repos/clauderic/dnd-kit> | 2026-09-23 |
| **新架构包** | `@dnd-kit/react`：`latest = 0.5.0`，`beta = 0.5.1-beta-20260912195958`（**2026-09-12**）。peer = `react: ^18.0.0 \|\| ^19.0.0`、`react-dom: ^18.0.0 \|\| ^19.0.0` —— **明确支持 React 19**，但版本号 `0.x` 仍属**未 GA 的 beta** | <https://registry.npmjs.org/@dnd-kit/react> | 2026-09-23 |
| 官方文档 | 经典版文档 <https://docs.dndkit.com/> | — | 2026-09-23 |

**结论与风险提示**
- **能用**：`@dnd-kit/core@6.3.1` + `@dnd-kit/sortable@10.0.0` 的 peer 范围覆盖 React 19，可直接用于当前技术栈。
- **但要知道**：这套「经典组合」自 **2024-12 起再未发布新版本**（距今约 21 个月），而同仓库开发已转向 `@dnd-kit/react`（0.5.x，beta）。即：**要么锁经典版承担"不再更新"的风险，要么用未 GA 的 0.5.x 承担 API 变动风险**。
- **本次未核实**：React 19 下运行时的**实际**行为缺陷（peer 声明与实际兼容性不一定一致）；官方仓库 129 个 open issues 中是否有 React 19 相关缺陷未逐条核实。
- **备选**：若不能接受上述风险，可评估 `react-beautiful-dnd` 的社区维护分支或原生 HTML5 DnD，但本次未调研，属**未核实**。

### B.4 状态管理：zustand

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **5.0.15**（2026-08-13） | <https://registry.npmjs.org/zustand/latest> | 2026-09-23 |
| 许可证 | MIT | 同上 | 2026-09-23 |
| peerDependencies | `react: >=18.0.0`、`@types/react: >=18.0.0`、`immer: >=9.0.6`、`use-sync-external-store: >=1.2.0`（后两者为可选 peer） | 同上 | 2026-09-23 |
| React 19 | `>=18.0.0` 已涵盖 19 | 同上 | 2026-09-23 |
| 官方文档 | <https://zustand.docs.pmnd.rs/> | — | 2026-09-23 |

### B.5 数据校验：zod

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **4.6.5**（2026-09-13） | <https://registry.npmjs.org/zod/latest> | 2026-09-23 |
| 许可证 | MIT | 同上 | 2026-09-23 |
| 运行时依赖 | 无 | 同上 | 2026-09-23 |
| 官方文档 | <https://zod.dev/> | — | 2026-09-23 |

> zod 目前处于 **4.x**（zod 4 已是主线），版本节奏活跃（最近发布于调研前 10 天）。

### B.6 Markdown 渲染与 XSS 安全

| 包 | 版本 | 发布日期 | 许可证 | peerDependencies | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- | --- |
| `react-markdown` | **10.1.0** | 2025-03-07 | MIT | `react: >=18`、`@types/react: >=18` | <https://github.com/remarkjs/react-markdown> | 2026-09-23 |
| `remark-gfm` | **4.0.1** | 2025-02-10 | MIT | — | <https://github.com/remarkjs/remark-gfm> | 2026-09-23 |
| `rehype-sanitize` | **6.0.0** | 2023-08-26 | MIT | — | <https://github.com/rehypejs/rehype-sanitize> | 2026-09-23 |

> 注：`react-markdown` 的 `next` dist-tag 仍是远古的 `3.0.0-rc4`，属历史遗留标签，**不要**据它选版本。

**`rehype-sanitize` 的默认白名单行为（官方 README 原文）**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 默认策略 | *"It drops anything that isn't explicitly allowed by a schema (**defaulting to how `github.com` works**)."* —— **默认白名单 = GitHub 的渲染白名单**，非白名单内容一律丢弃 | <https://github.com/rehypejs/rehype-sanitize> | 2026-09-23 |
| 导出的 `defaultSchema` | README：*"Default schema (`Options`). **Follows GitHub style sanitation.**"* 包同时导出 `defaultSchema` 标识符，默认导出为 `rehypeSanitize` 插件 | 同上 | 2026-09-23 |
| 底层实现 | 基于 `hast-util-sanitize`（README 原文：*"This plugin is built on `hast-util-sanitize`, which cleans hast syntax trees"*），schema 结构见 `hast-util-sanitize` 的 `Schema` 文档 | 同上 | 2026-09-23 |
| 自定义方式 | `unified().use(rehypeSanitize, schema)`，schema 为 `Options`（`hast-util-sanitize` 的 `Schema` 类型） | 同上 | 2026-09-23 |
| 何时使用（官方建议） | *"It's recommended to sanitize your HTML any time you do not completely trust authors **or the plugins being used**."* —— 注意后半句：**连插件本身也不完全信任**时才需要 | 同上 | 2026-09-23 |
| 已知安全示例：DOM clobbering | README 专设章节 *"Example: headings (DOM clobbering)"*，说明通过 `id`/`name` 覆写 `window` 全局属性的攻击，常发生在**用用户内容生成 heading ID** 时 | 同上 | 2026-09-23 |
| 其它官方示例 | *"Example: math"*、*"Example: syntax highlighting"* —— 说明数学公式与代码高亮**在默认 schema 下会被丢弃，必须按官方示例扩展 schema** | 同上 | 2026-09-23 |
| ESM 要求 | 包为 **ESM only**；README 示例用 `import rehypeSanitize from 'rehype-sanitize'` | 同上 | 2026-09-23 |
| 维护状态 | ⚠️ 最后发布 **6.0.0 = 2023-08-26**（约 3 年前） | <https://registry.npmjs.org/rehype-sanitize> | 2026-09-23 |

**推荐配置（基于 README 的官方事实推导）**

```ts
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'

// 默认即 GitHub 白名单；仅在确有必要时按 hast-util-sanitize 的 Schema 扩展
<ReactMarkdown
  remarkPlugins={[remarkGfm]}
  rehypePlugins={[[rehypeSanitize, defaultSchema]]}
>
  {markdown}
</ReactMarkdown>
```

> **本项目安全要点**
> 1. **必须显式挂 `rehype-sanitize`**。`react-markdown` 默认**不**做 HTML 净化（它默认不渲染原始 HTML，但一旦为支持任务列表/表格等引入 `rehype-raw`，就必须同时挂 `rehype-sanitize`，否则等于开放 XSS）。
> 2. 若要支持**代码高亮**或**数学公式**，默认 schema 会**丢弃**相关节点，需按 README 的两个 Example 章节扩展 schema——这是最容易"照抄配置却渲染不出来"的坑。
> 3. 若要给 heading 生成 `id`，务必参考 README 的 DOM clobbering 章节，避免用用户内容直接做 `id`/`name`。

### B.7 图表：recharts

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **3.10.1**（2026-07-25） | <https://registry.npmjs.org/recharts/latest> | 2026-09-23 |
| 许可证 | MIT | 同上 | 2026-09-23 |
| **React 19 兼容性（声明层面）** | peer = `react: ^16.8.0 \|\| ^17.0.0 \|\| ^18.0.0 \|\| **^19.0.0**`，`react-dom` 与 `react-is` 同范围 —— **明确声明支持 React 19** | 同上 | 2026-09-23 |
| 运行时依赖（较重） | `clsx`、`immer`、`reselect`、`es-toolkit`、`react-redux`、`eventemitter3`、`tiny-invariant`、`victory-vendor`、`@reduxjs/toolkit`、`decimal.js-light`、`use-sync-external-store` | 同上 | 2026-09-23 |
| `unpackedSize` | **7,452,998 B ≈ 7.45 MB** | 同上 | 2026-09-23 |
| 官方文档 | <https://recharts.org/> | — | 2026-09-23 |

> **注意**：recharts 3.x **引入了 `@reduxjs/toolkit` + `react-redux` 作为运行时依赖**（内部状态管理），会显著增加 bundle。这对"统计图表只是次要功能"的 Todo 应用是一个**体积成本信号**，建议评估是否改用更轻的图表库或纯 SVG 自绘（备选未核实）。

### B.8 日历视图：FullCalendar vs react-big-calendar

**包族与许可证（关键：标准版 vs 高级版）**

> **⚠️ v7 是破坏性的包结构重组**：FullCalendar v7 改为 **headless 架构**（`@fullcalendar/core` + `@full-ui/headless-calendar`，vanilla 包额外依赖 `preact@^10.29.8`），并且**把标准视图插件（`daygrid` / `timegrid` / `list` / `interaction` / `multimonth`）合并进了 core，这些包不再单独发布 v7**——它们停在 `6.1.21` 属于**历史遗留**，不是"尚未跟进"。同时 **Premium 在 v7 更名**为 `fullcalendar-scheduler` / `@fullcalendar/react-scheduler`。
> 依赖图实证（来源：各包 `latest` 的 `dependencies` / `peerDependencies`，访问日期 2026-09-23）：
> ```
> @fullcalendar/react@7.1.0  → @fullcalendar/core@7.1.0, @full-ui/headless-calendar@7.1.0
> @fullcalendar/core@7.1.0   → @full-ui/headless-calendar@7.1.0
> fullcalendar@7.1.0         → preact@^10.29.8, @fullcalendar/core@7.1.0, @full-ui/headless-calendar@7.1.0
> @fullcalendar/rrule@7.1.0  → @fullcalendar/core@7.1.0, @full-ui/headless-calendar@7.1.0
>                              peer: rrule@^2.6.0, temporal-polyfill@^1.0.1
> ```
> **即：v7 下只需装 `@fullcalendar/react`，月/周/日视图已内置，无需再装 `daygrid`/`timegrid`/`interaction`。**

**v7 包族与许可证（关键：标准版 vs 高级版）**

| 包 | 版本 | 发布日期 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `@fullcalendar/core` | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/@fullcalendar/core> | 2026-09-23 |
| `@fullcalendar/react` | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/@fullcalendar/react> | 2026-09-23 |
| `fullcalendar`（vanilla JS） | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/fullcalendar> | 2026-09-23 |
| `@full-ui/headless-calendar` | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/@full-ui/headless-calendar> | 2026-09-23 |
| `@fullcalendar/web-component` | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/@fullcalendar/web-component> | 2026-09-23 |
| `@fullcalendar/bootstrap5` | **7.1.0** | 2026-09-05 | **MIT** | <https://registry.npmjs.org/@fullcalendar/bootstrap5> | 2026-09-23 |
| **`@fullcalendar/rrule`** | **7.1.0** | 2026-09-05 | **MIT**（peer `rrule@^2.6.0`） | <https://registry.npmjs.org/@fullcalendar/rrule> | 2026-09-23 |
| **`fullcalendar-scheduler`**（v7 Premium） | 7.1.0 | 2026-09-05 | **`SEE LICENSE IN LICENSE.md`（专有）** | <https://registry.npmjs.org/fullcalendar-scheduler> | 2026-09-23 |
| **`@fullcalendar/react-scheduler`**（v7 Premium） | 7.1.0 | 2026-09-05 | **`SEE LICENSE IN LICENSE.md`（专有）** | <https://registry.npmjs.org/@fullcalendar/react-scheduler> | 2026-09-23 |
| `@full-ui/headless-grid` | 7.1.0 | 2026-09-05 | **`SEE LICENSE IN LICENSE.md`（专有）** | <https://registry.npmjs.org/@full-ui/headless-grid> | 2026-09-23 |
| ~~`@fullcalendar/resource-timeline`~~ | 6.1.21 | 2026-06-18 | 专有（**v6 遗留**） | <https://registry.npmjs.org/@fullcalendar/resource-timeline> | 2026-09-23 |
| ~~`@fullcalendar/resource-timegrid`~~ | 6.1.21 | 2026-06-18 | 专有（**v6 遗留**） | <https://registry.npmjs.org/@fullcalendar/resource-timegrid> | 2026-09-23 |
| ~~`@fullcalendar/adaptive`~~ | 6.1.21 | 2026-06-18 | 专有（**v6 遗留**） | <https://registry.npmjs.org/@fullcalendar/adaptive> | 2026-09-23 |
| `react-big-calendar` | **1.20.0** | 2026-06-01 | **MIT** | <https://registry.npmjs.org/react-big-calendar> | 2026-09-23 |

> **💡 对重复任务的重要发现**：`@fullcalendar/rrule@7.1.0` 是 **MIT 免费**的官方插件，peer 依赖 `rrule@^2.6.0` —— 与 §B.2 的 rrule.js 2.8.1 **版本兼容**（`^2.6.0` 覆盖 `2.8.1`）。这意味着「重复任务 + 日历视图」可以在**纯 MIT 组合**下实现，无需购买 Premium。

**哪些功能需要付费（官方定价页）** — 来源：<https://fullcalendar.io/pricing>，访问日期 2026-09-23

| 档位 | 许可证 | 价格 | 含哪些功能 |
| --- | --- | --- | --- |
| **Standard** | **MIT 开源** | 免费 | *"All the features in the documentation that are NOT marked as Premium"*（文档中**未**标 Premium 的全部功能） |
| **Premium** | **商业许可**（另有非商业/开源选项） | **起价 $480** | **Timeline View**、**Vertical Resource View**、**Printer-friendly rendering**、**Connectors for React / Vue / Angular**（定价页把 connectors 列在 Premium 列） |
| **OEM** | 定制条款 | Custom pricing | 再分发权（redistribution） |

**许可证细则（定价页原文要点）**
- Standard：*"FullCalendar's standard features are released under an open-source MIT license."*
- Premium：*"most commonly licensed under a commercial license, but there are additional options for non-commercial and open-source use."*
- 席位：Premium 按**使用 Premium 源码/JS API/自定义 CSS 的开发者**计席位；1–10 席或无限席。
- 续费：鼓励按年续费；提前续费 5 折，过期后续费 7.5 折；**停止续费仍可永久使用到期前最后发布的版本**。
- 再分发：若不向客户开放源码编辑，公司内一份 Premium 许可即可交付多个客户项目；**`@fullcalendar/resource-*` / `adaptive` 的 npm 许可证字段为 `SEE LICENSE IN LICENSE.md`，即专有，不是 MIT**。

> **⚠️ 锁版本要点**
> v7 下 **Premium 的 npm 包名已换成 `fullcalendar-scheduler` / `@fullcalendar/react-scheduler`**（均为 `7.1.0`，专有许可），而旧的 `resource-*` / `adaptive` / `timegrid` / `daygrid` 等仍是 6.1.21 的 v6 遗留包。**若照抄 v6 时代的 FullCalendar 安装命令（安装 `@fullcalendar/daygrid` 等），会装到 v6 包并与 v7 core 混用——混用是否可用本次未核实。**
> **对本项目的影响**：Todo 应用只需**月/周/日视图 + 拖拽**，用 MIT 的 `@fullcalendar/react@7.1.0`（+ `temporal-polyfill`）即可，**不需要任何视图插件**，**不要引入 `*-scheduler` / `headless-grid`**（付费）。

**`@fullcalendar/react` 7.1.0 的额外 peer 依赖（易踩坑）**

```
peerDependencies: {
  react: "^17 || ^18 || ^19",
  react-dom: "^17 || ^18 || ^19",
  "temporal-polyfill": "^1.0.1"     // ← v7 新增，必须一起安装
}
```
来源：<https://registry.npmjs.org/@fullcalendar/react/latest>，访问日期 2026-09-23。
**React 19 在支持范围内**；但 `temporal-polyfill ^1.0.1` 是 v7 引入的**必需 peer**。

**社区替代：react-big-calendar**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **1.20.0**（2026-06-01） | <https://registry.npmjs.org/react-big-calendar/latest> | 2026-09-23 |
| 许可证 | **MIT**（全功能，无付费层） | 同上 | 2026-09-23 |
| React 19 兼容 | peer = `react: ^16.14.0 \|\| ^17 \|\| ^18 \|\| ^19`、`react-dom` 同 —— **支持 React 19** | 同上 | 2026-09-23 |
| 维护状态 | 有 2026-06-01 的发布，**仍在维护**（发布节奏低于 FullCalendar）。更细的 issue/PR 活跃度**未核实** | — | 2026-09-23 |
| 官方仓库 | <https://github.com/jquense/react-big-calendar> | — | 2026-09-23 |

**选型建议**
- **推荐 FullCalendar v7 路线**：`@fullcalendar/react@7.1.0`（MIT）+ `temporal-polyfill@^1.0.1`（必需 peer）+ **`@fullcalendar/rrule@7.1.0`（MIT，与 rrule.js 2.8.1 兼容）**。这一组合能在**零许可成本**下同时满足「日历视图」与「重复任务展开」。
- **备选 `react-big-calendar` 1.20.0**（MIT，全功能，React 19 支持，无 peer 门槛）：更省心但生态与交互成熟度低于 FullCalendar，且**没有官方 RRULE 集成**，重复事件需自行展开。
- 需要 **Timeline / 资源视图 / 打印友好渲染** 时必须购买 Premium（v7 包名 `fullcalendar-scheduler` / `@fullcalendar/react-scheduler`，起价 **$480**）——**对 Todo 应用通常没有必要**。

### B.9 虚拟列表：@tanstack/react-virtual

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 | **3.14.13**（2026-09-14） | <https://registry.npmjs.org/@tanstack/react-virtual/latest> | 2026-09-23 |
| 许可证 | MIT | 同上 | 2026-09-23 |
| React 19 兼容 | peer = `react: ^16.8.0 \|\| ^17.0.0 \|\| ^18.0.0 \|\| ^19.0.0`、`react-dom` 同 —— **支持 React 19** | 同上 | 2026-09-23 |
| 体积 | `unpackedSize` = **60,900 B ≈ 61 KB**（极轻） | 同上 | 2026-09-23 |
| 运行时依赖 | 仅 `@tanstack/virtual-core` | 同上 | 2026-09-23 |
| 官方文档 | <https://tanstack.com/virtual> | — | 2026-09-23 |

> 应对上千条任务是合适的：**61 KB、单一依赖、明确支持 React 19、近期仍在发布**。

### B.10 代码质量工具链

| 工具 | 确切版本 | 发布日期 | 许可证 | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- |
| `eslint` | **10.11.0** | 2026-09-18 | MIT | <https://registry.npmjs.org/eslint/latest> · <https://eslint.org/> | 2026-09-23 |
| `prettier` | **3.9.8** | — | MIT | <https://registry.npmjs.org/prettier/latest> · <https://prettier.io/> | 2026-09-23 |
| `vitest` | **5.0.1** | 2026-09-15 | MIT | <https://registry.npmjs.org/vitest/latest> · <https://vitest.dev/> | 2026-09-23 |
| `playwright` | **1.63.0** | 2026-09-04 | Apache-2.0 | <https://registry.npmjs.org/playwright/latest> · <https://playwright.dev/> | 2026-09-23 |
| `@playwright/test` | **1.63.0** | 2026-09-04 | Apache-2.0 | 同上 | 2026-09-23 |

**ESLint flat config 的当前状态（关键更正）**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| **ESLint 当前大版本** | **v10（10.11.0）**，不是 v9 | <https://eslint.org/blog/> | 2026-09-23 |
| **ESLint 9.x 生命周期** | ⚠️ **已于 2026-08-06 停止维护（end-of-life）**。官网横幅原文：*"ESLint v9.x reached end-of-life on 2026-08-06 and is no longer maintained. Upgrade or consider long-term support options"* | <https://eslint.org/blog/> | 2026-09-23 |
| v9 最后版本 | `9.39.5`（官方文档版本切换器中的 v9 分支） | <https://eslint.org/docs/latest/use/configure/configuration-files> | 2026-09-23 |
| v10 最后版本 | `10.11.0`（2026-09-18） | <https://eslint.org/blog/> | 2026-09-23 |
| flat config 状态 | flat config（`eslint.config.js`）是**当前唯一受支持的配置体系**；官方文档导航含 **"Migration to Flat Config"** 与 **"Configuration Migration Guide"** 专页；`eslintrc` 体系已退役 | <https://eslint.org/docs/latest/use/configure/configuration-files> | 2026-09-23 |
| 官方迁移资源 | 官方文档含 *"Migrate to v10.x"* 与 *"Migration to Flat Config"*；另有官方公告 *"Automating ESLint migrations with Codemod"*（ESLint 与 Codemod 合作为 ESLint 迁移提供官方 codemod，2026-07-16） | <https://eslint.org/blog/> | 2026-09-23 |

> **结论**：**不要再按「ESLint 9 + flat config」立项**——ESLint 9 已 EOL（2026-08-06），应直接采用 **ESLint 10.11.0 + flat config**。若必须留在 v9，官方提示需走 long-term support 选项。

**Vitest 5 的关键 peer 约束（锁版本时需注意）**

```
peerDependencies: {
  vite: "^6.4.0 || ^7.0.0 || ^8.0.0",   // ← 覆盖 Vite 8，与 A 节一致
  jsdom: "*", "happy-dom": "*",
  "@vitest/ui": "5.0.1",
  "@types/node": "^22.0.0 || >=24.0.0",
  "@vitest/coverage-v8": "5.0.1",
  "@vitest/browser-playwright": "5.0.1", ...
}
```
来源：<https://registry.npmjs.org/vitest/latest>，访问日期 2026-09-23。
**注意**：`@vitest/*` 附属包与主包**必须同版本**（peer 写死为 `5.0.1`）。

---

## C. 数据与迁移（第三方任务导入）

> 详细版见同级文件 **`docs/third-party-task-import-verification.md`**（30,987 bytes，含逐条证据与「未核实清单」）。本节为其结论摘要 + 微软 To Do 自核实部分。
> **访问日期统一为 2026-09-23**（实际抓取时刻；子报告早期误标 09-21，以本表为准）。

### C.0 总览

| 平台 | 当前 API 版本 / 结论 | 自建 OAuth 可行？ | 客户端类型 / PKCE | 无 OAuth 官方导出？ | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- | --- |
| **Todoist** | **Todoist API v1**，base `https://api.todoist.com/api/v1`（统一了 Sync v9 + REST v2） | ✅ 可以 | **机密客户端，无 PKCE**（必须内置 `client_secret`） | ✅ **CSV**（每项目，官方模板 16 列）+ Pro/Business 级 **ZIP（内含 CSV）** | <https://developer.todoist.com/api/v1/> | 2026-09-23 |
| **Microsoft To Do** | **Microsoft Graph To Do API（v1.0）**；**未宣布停用** | ✅ 可以 | 委托权限 + **公共客户端**（`http://localhost` 重定向） | ❌ **无官方内置导出** | <https://learn.microsoft.com/en-us/graph/api/resources/todo-overview> | 2026-09-23 |
| **Google Tasks** | **Google Tasks API v1**，base `https://tasks.googleapis.com/tasks/v1` | ✅ 可以（**路径最清晰**） | **Desktop app 公共客户端 + PKCE + loopback `http://127.0.0.1:port`** | ✅ **Google Takeout**（Zip/Tgz） | <https://developers.google.com/workspace/tasks/reference/rest> | 2026-09-23 |
| **Notion** | `Notion-Version: **2026-03-11**`；数据库查询改用 **data sources** 端点 | ✅ 可以 | **机密客户端，无 PKCE**（必须内置 `client_secret`） | ✅ 人工导出 **ZIP(Markdown+CSV)/HTML/PDF** | <https://developers.notion.com/reference/versioning> | 2026-09-23 |

### C.1 Todoist

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 当前 API 版本 | **Todoist API v1**。官方原文：*"The Todoist API v1 is a new API that unifies the Sync API v9 and the REST API v2"* | <https://developer.todoist.com/api/v1/> | 2026-09-23 |
| Base URL | `https://api.todoist.com/api/v1`。旧 subdomain 已废弃（官方原文：*"we will only focus on `api.todoist.com`"*） | 同上 | 2026-09-23 |
| 旧文档状态 | `/rest/v2/`、`/sync/v9/`、`/guides/` 与 `/api/v1/` 返回**字节完全相同**的 HTML（均 4,167,217 bytes，SHA-256 前 32 位 `9A7DC3A8CEA9E6F24A5E09742B7BAB33`）→ 旧版独立参考文档已不再单独提供 | 同上 | 2026-09-23 |
| OAuth 授权端点 | `https://app.todoist.com/oauth/authorize`（必填 `client_id`/`scope`/`state`；配置多个回调时 `redirect_uri` 必填） | 同上 | 2026-09-23 |
| OAuth 令牌端点 | `POST https://api.todoist.com/oauth/access_token`（`client_id` + `client_secret` + `code`） | 同上 | 2026-09-23 |
| **PKCE** | ❌ **不支持**（机密客户端模型，必须内置 `client_secret`，桌面应用需自行保护该密钥） | 同上 | 2026-09-23 |
| Token 有效期 | 新建应用默认启用 refresh token（`expires_in` = 3600 + `refresh_token`）；旧应用 `expires_in` = 315360000 且无 `refresh_token` | 同上 | 2026-09-23 |
| Scope 列表 | `task:add`、`data:read`、`data:read_write`、`data:delete`、`project:delete`、`backups:read` | 同上 | 2026-09-23 |
| 个人 API 令牌 | ✅ **仍可用，无弃用声明**（*"obtain your personal API token from the integrations settings"*）；`GET /api/v1/backups` 明确接受个人令牌 | 同上 | 2026-09-23 |
| ⚠️ 易混点 | `POST /api/v1/access_tokens/migrate_personal_token` 迁移的是**旧 email/password 认证方式**产生的令牌，**不是** Integrations 页面里的个人 API 令牌 | 同上 | 2026-09-23 |
| **官方 CSV 导入模板** | ✅ **有文档化**。列：`TYPE, CONTENT, DESCRIPTION, PRIORITY, INDENT, AUTHOR, RESPONSIBLE, DATE, DATE_LANG, TIMEZONE, DURATION, DURATION_UNIT, meta, DEADLINE, DEADLINE_LANG, IS_COLLAPSED`。`TYPE` ∈ `task`/`section`/`note`（**必须小写**），`PRIORITY` 1–4（留空 = p1），必须 **UTF-8**，单项目上限 **300 任务** | <https://www.todoist.com/help/articles/import-or-export-a-project-as-a-csv-file-in-todoist-YC8YvN>（Last updated September 18, 2026） | 2026-09-23 |
| 用户级备份导出 | Settings → Backups（**仅 Pro/Business**）：**ZIP，内含每个活动项目一个 CSV**，最多 21 份，**不含已完成任务与已归档项目** | <https://developer.todoist.com/api/v1/> | 2026-09-23 |
| API 导出端点 | `GET /api/v1/backups`、`GET /api/v1/backups/download?file=<...>.zip`（需 `data:read_write`，302 跳转到 1 分钟过期的签名 CloudFront URL）；项目模板 `GET /api/v1/templates/file` → CSV | 同上 | 2026-09-23 |

**两问回答**
1. **可在桌面应用内通过用户自建 OAuth 应用访问？** ✅ **可以**，authorization code 流程（机密客户端）。
   **⚠️ 未核实**：redirect URL 是否允许 `http://127.0.0.1` 回环或自定义 scheme —— 官方未记载（App Management Console 需登录且页面加载失败）。**这是落地前必须实机验证的第一风险点**。
2. **是否有官方导出文件作为无 OAuth 降级路径？** ✅ **有**。每项目 CSV（与导入模板同源，可直接回导）+ Pro/Business 的 ZIP（内含 CSV）。均为**用户手动导出**。

### C.2 Microsoft To Do（含停用传闻核实 —— 本项为本次调研的重点更正）

**结论一：微软并未宣布 Microsoft To Do 停用。** 任务前提不成立。

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| **Microsoft To Do 是否停用** | ❌ **没有**。官方 FAQ「Information for Microsoft To Do users」章节原文：*"**There is no impact on existing user scenarios or functionality of To Do.** To Do capabilities, such as My Day, My Tasks, Flagged email, and more, are now available in Planner in Teams."* | <https://support.microsoft.com/en-us/planner/frequently-asked-questions-about-microsoft-planner> | 2026-09-23 |
| 旁证 | 官方「To Do vs. Planner」页面**至今仍在维护**，仍将二者描述为互补产品（*"you can use To Do and Planner together, to compliment each other"*） | <https://support.microsoft.com/en-us/todo/to-do-vs-planner> | 2026-09-23 |
| 真正被停用的是 **Project for the web** | **已于 2025 年 8 月停用**，用户被重定向到 Planner。官方原文：*"As of August 2025, we retired Project for the web and the Project and Roadmap apps in Microsoft Teams and have transitioned users to Planner for the web and Planner in Teams."* | 官方 Planner FAQ（同上） | 2026-09-23 |
| 真正被停用的是 **Project Online** | **2026-09-30** 正式停用（2025-10-01 起新客户停售）。官方 FAQ 原文：*"**September 30, 2026**: Official retirement date."* | 官方 Planner FAQ（同上） | 2026-09-23 |
| 真正被改名的是 **Teams 内应用** | 「Tasks by Planner and To Do」应用于 **2024 年 4 月**改名为 Planner，并被新 Planner 体验取代 | 官方 Planner FAQ（同上） | 2026-09-23 |
| MC1193421 停用的范围 | 停用的是 Planner 的若干**功能**（旧任务评论、premium 白板 tab、Loop 中的 Planner 组件、Viva Goals 集成、**iCalendar feed**），**不是 To Do 应用**。发布 2025-12-09，最后更新 2026-02-19，影响窗口 2026 年 1 月中至 2 月中 | <https://mc.merill.net/message/MC1193421> | 2026-09-23 |

> **对项目决策的影响**：To Do 导入功能**不必**按「赶在应用下线前抢救用户数据」来排优先级。微软当前的投入重心是 Planner，To Do 处于「维护中但不演进」状态 —— 这仍然是**做导入集成的合理理由**（用户可能想迁出），但**不是紧急项**。

**结论二：Graph To Do API 的端点与权限**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| API 归属 | **Microsoft Graph v1.0** 的 To Do API（`microsoft.graph` 命名空间） | <https://learn.microsoft.com/en-us/graph/api/resources/todo-overview?view=graph-rest-1.0> | 2026-09-23 |
| 资源类型 | `todoTaskList`、`todoTask`、`checklistItem`、`linkedResource` | 同上 | 2026-09-23 |
| 列任务列表 | `GET /me/todo/lists` | 同上 | 2026-09-23 |
| 列任务 | `GET /me/todo/lists/{todoTaskListId}/tasks` | 同上 | 2026-09-23 |
| 子任务 | `GET /me/todo/lists/{todoTaskListId}/tasks/{todoTaskId}/checklistItems/{checklistItems}` | 同上 | 2026-09-23 |
| 关联资源 | `GET /me/todo/lists/{todoTaskListId}/tasks/{todoTaskId}/linkedresources/{linkedResourceId}` | 同上 | 2026-09-23 |
| **增量同步** | ✅ 支持 **delta query**：`todoTask` 集合与 `todoTaskList` 均支持 —— **对导入/双向同步极有价值**（避免全量拉取） | 同上 | 2026-09-23 |
| 权限模型 | 官方原文：*"The API supports both delegated and application permissions."* | 同上 | 2026-09-23 |
| **`Tasks.ReadWrite` 确切信息** | 类别 **Application + Delegated**；Identifier **`2219042f-cab5-40cc-b0d2-16b1540b4c5f`**；DisplayText *"Create, read, update, and delete user's tasks and task lists"*；描述 *"Allows the app to create, read, update, and delete the signed-in user's tasks and task lists, including any shared with the user."* | <https://learn.microsoft.com/en-us/graph/permissions-reference> | 2026-09-23 |
| **个人账户支持（关键）** | ✅ 官方原文：*"The `Tasks.ReadWrite` delegated permission is available for consent in **personal Microsoft accounts**."* —— 意味着**个人 MSA 用户也能授权**，不必是企业租户 | 同上 | 2026-09-23 |
| 文档最后更新 | 2024-03-06（`todo-overview` 页面） | <https://learn.microsoft.com/en-us/graph/api/resources/todo-overview?view=graph-rest-1.0> | 2026-09-23 |

**结论三：桌面应用内自建 OAuth 的官方指引**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 可自建应用 | ✅ 在 Microsoft Entra 注册应用，暴露 `Tasks.ReadWrite` 委托权限 | <https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration> | 2026-09-23 |
| 平台配置路径 | `Authentication` → `Add a platform` → **`Mobile and desktop applications`** | 同上 | 2026-09-23 |
| **桌面重定向 URI（官方确切值）** | 内嵌浏览器：`https://login.microsoftonline.com/common/oauth2/nativeclient`；**系统浏览器：`http://localhost`** | 同上 | 2026-09-23 |
| 公共客户端设置 | `Authentication` → `Advanced settings` → **`Allow public client flows` = `Yes`**，否则无法使用设备码/交互式流程 | 同上 | 2026-09-23 |
| 授权码流程 | 支持 OAuth 2.0 authorization code flow（官方把 *"Desktop and mobile apps"* 列为适用场景） | <https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow> | 2026-09-23 |
| 是否需 `client_secret` | ❌ **不需要**（公共客户端，`Allow public client flows` 即可） | 同上 | 2026-09-23 |

**结论四：官方导出文件**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| Microsoft To Do 内置导出 | ❌ **无**。To Do 应用未提供导出功能 | — | 2026-09-23 |
| 社区/AI 检索到的唯一路径 | 借道 **Outlook 经典桌面版**的 `File → Open & Export → Import/Export → Export to a file → Comma Separated Values (CSV)`。**这是 Outlook 的导出能力，不是 To Do 的**，且要求用户装有经典 Outlook | <https://learn.microsoft.com/en-us/answers/questions/5687598/> | 2026-09-23 |
| ⚠️ 结论 | **Microsoft To Do 没有官方文件导出降级路径**。唯一的官方程序化访问途径是 **Graph API + OAuth**。若坚持支持 To Do 导入，**OAuth 是必需项，没有退路** | — | 2026-09-23 |

**两问回答**
1. **可在桌面应用内通过用户自建 OAuth 应用访问？** ✅ **可以，且是四家中最规范的**：公共客户端（无需 `client_secret`）、官方明确给出桌面重定向 URI（`http://localhost`）、支持个人 MSA 账户、支持 delta query 增量同步。
2. **是否有官方导出文件（CSV/JSON/ZIP）作为无 OAuth 降级路径？** ❌ **没有**。To Do 无内置导出；Outlook CSV 导出属另一产品能力，不可作为 To Do 的降级路径。

### C.3 Google Tasks

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| API 版本 | **Google Tasks API v1**，base `https://tasks.googleapis.com/tasks/v1` | <https://developers.google.com/workspace/tasks/reference/rest> | 2026-09-23 |
| Scope | `https://www.googleapis.com/auth/tasks`（读写）、`https://www.googleapis.com/auth/tasks.readonly`（只读） | <https://developers.google.com/workspace/tasks/auth> | 2026-09-23 |
| 列任务 | `GET https://tasks.googleapis.com/tasks/v1/lists/{tasklist}/tasks`（`maxResults` 默认 20、最大 100） | <https://developers.google.com/workspace/tasks/reference/rest/v1/tasks/list> | 2026-09-23 |
| 列任务列表 | `GET https://tasks.googleapis.com/tasks/v1/users/@me/lists`（默认 1000、最大 1000） | 同上 | 2026-09-23 |
| 容量上限 | 每 list **20,000** 个未隐藏任务；账号总计 **100,000** 任务；最多 **2000** 个 list。官方原文：*"A user can have up to 20,000 non-hidden tasks per list and up to 100,000 tasks in total at a time."* | 同上 | 2026-09-23 |
| 其它端点 | `POST /tasks/v1/lists/{tasklist}/tasks`（insert）、`PATCH`（update）、`DELETE`（delete）、`POST .../tasks/{task}/move`（move）、`POST .../tasks/clear`（clear） | <https://developers.google.com/workspace/tasks/reference/rest/v1/tasks> | 2026-09-23 |
| **桌面自建 OAuth** | ✅ **可以**。client type 选 **Desktop app**（官方推荐 macOS/Linux/**Windows** desktop，不含 UWP） | <https://developers.google.com/workspace/tasks/auth> | 2026-09-23 |
| **重定向 URI** | **loopback IP**：`http://127.0.0.1:port` 或 `http://[::1]:port`（用 `localhost` 也可但可能受防火墙影响） | 同上 | 2026-09-23 |
| **PKCE** | ✅ **支持**，流程含 code verifier / challenge | 同上 | 2026-09-23 |
| **OOB 政策（已停用）** | ❌ 官方原文：*"The manual copy/paste option, also referred to as an out of band (OOB) redirect method, is **no longer supported**"*。时间线：2022-02-28 阻止新用法 → 2022-09-05 用户警告 → 2022-10-03 对旧客户端弃用 → **2023-01-31 所有既有客户端被阻止** | <https://developers.google.com/identity/protocols/oauth2/resources/oob-migration> | 2026-09-23 |
| loopback 弃用范围澄清 | loopback 的弃用**只针对 Android / Chrome app / iOS**，**不影响 Windows 桌面应用** | 同上 | 2026-09-23 |
| **官方导出：Takeout** | ✅ **Google Takeout 支持 Tasks**，归档文件类型为 **Zip 或 Tgz**，含官方字段清单（List IDs / List titles / Parent IDs / Due dates / Completed timestamps / Links / Task types 等） | <https://support.google.com/tasks/answer/10017961>、<https://support.google.com/accounts/answer/3024190> | 2026-09-23 |
| 文档更新时间 | auth/overview 页 Last updated **2026-09-03 UTC** | <https://developers.google.com/workspace/tasks/auth> | 2026-09-23 |

**两问回答**
1. **可在桌面应用内通过用户自建 OAuth 应用访问？** ✅ **可以，且是四家中路径最清晰的**：官方明确支持 **Desktop app 公共客户端 + PKCE + loopback `http://127.0.0.1`**，无需 `client_secret`，且有官方 OOB→loopback 迁移指南背书。
2. **是否有官方导出文件作为无 OAuth 降级路径？** ✅ **有**，**Google Takeout**（Zip/Tgz，含 Tasks）。
   **⚠️ 未核实**：Tasks 数据在归档内的**确切文件名/扩展名**（很可能 JSON，官方未写明）；是否存在官方导入功能。

### C.4 Notion

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| **`Notion-Version` 确切最新值** | **`2026-03-11`** | <https://developers.notion.com/reference/versioning> | 2026-09-23 |
| 取值证据（四重） | ① versioning 页编译产物 `const latestApiVersion=<code>2026-03-11</code>`；② 同页 cURL 示例 `-H "Notion-Version: 2026-03-11"`；③ Authentication 页示例同为 `2026-03-11`；④ changes-by-version 版本清单（`2026-03-11` / `2025-09-03` / `2022-06-28` / `2022-02-22` / `2021-08-16` / `2021-05-13`）中最新 | 同上 | 2026-09-23 |
| ⚠️ 两个易误判项 | `Notion-Beta: notion-as-code-2026-07-31` 是 **beta 功能开关**，不是 API 版本；页面上出现的 `2026-09-22` / `2026-07-29` 只是页面 `dateModified` 元数据 | 同上 | 2026-09-23 |
| 版本头是否必需 | ✅ 官方原文：*"Setting this header is **required**."* | 同上 | 2026-09-23 |
| pin 版本的局限 | 新增字段/端点等**兼容性变更对所有版本同时生效**，pin 版本**无法延迟**。官方原文：*"Additive changes apply to every API version at the same time, including older ones: pinning Notion-Version does not delay them."* 并明确警告**打包的桌面应用**是"严格解析拒绝未知键"风险最高的一类客户端 | 同上 | 2026-09-23 |
| 旧查询端点 | ❌ `POST /v1/databases/{database_id}/query` **已弃用**。官方原文：*"**Deprecated as of version 2025-09-03** … the concepts of databases and data sources were split up"* | <https://developers.notion.com/reference/post-database-query> | 2026-09-23 |
| **新查询端点** | ✅ **`POST https://api.notion.com/v1/data_sources/{data_source_id}/query`**（支持 `?filter_properties[]=`） | <https://developers.notion.com/reference/post-data-source-query> | 2026-09-23 |
| 查询前置条件 | 必须先把 data source **共享给该连接**，否则 **404**；缺少 read content capability 则 **403** | 同上 | 2026-09-23 |
| 认证方式 | Bearer token，三种来源：**internal connection**（静态 installation token，bot 身份）/ **personal access token**（用户级）/ **public connection**（OAuth 2.0） | <https://developers.notion.com/reference/authentication> | 2026-09-23 |
| 自建 OAuth | ✅ **可以**。在 Developer portal（`https://app.notion.com/developers/connections`）创建 **public connection**，填 Redirect URI(s) + installation scope（`Any workspace` / `Selected workspaces only`，**创建后不可更改**） | 同上 | 2026-09-23 |
| OAuth 授权端点 | `https://api.notion.com/v1/oauth/authorize?owner=user&client_id=...&redirect_uri=...&response_type=code` | 同上 | 2026-09-23 |
| OAuth 令牌端点 | `POST https://api.notion.com/v1/oauth/token`，认证用 **HTTP Basic（`client_id:client_secret`）** | 同上 | 2026-09-23 |
| **PKCE** | ❌ **未文档化**（authorization 指南与 token 文档中 `code_challenge`/`code_verifier` 命中 0；PKCE 仅见于 MCP 客户端指南）→ 机密客户端模型 | 同上 | 2026-09-23 |
| **官方导出格式** | ✅ Export 支持 **PDF / HTML / Markdown & CSV（打包为 ZIP；整页数据库导出为 CSV）**；整库用 `Export all workspace content`（HTML/Markdown/CSV，链接 **7 天过期**，最长约 **30 小时**，**仅桌面/网页端**） | <https://www.notion.com/help/export-your-content> | 2026-09-23 |
| ⚠️ API 导出限制 | **标准 REST API 无整库导出端点**。官方把 API 导出限定在 **Admin API**：*"it's in beta on the Enterprise Plan, and an organization owner has to set up an admin bot and token first"*；端点 `POST https://api.notion.com/admin/v1/spaces/{space_id}/exports`，scope `workspace:export`，`export_type` ∈ `html`/`markdown`/`pdf` | <https://developers.notion.com/reference/create-a-space-export> | 2026-09-23 |

**两问回答**
1. **可在桌面应用内通过用户自建 OAuth 应用访问？** ✅ **可以**，但为**机密客户端**（必须内置 `client_secret`；桌面应用需自行保护该密钥）。
   **⚠️ 未核实**：public connection 是否接受**回环重定向**（官方指南中 `localhost`/`127.0.0.1`/`loopback` 命中 0，示例仅 HTTPS 域名）；是否支持 PKCE（文档未记载）。
2. **是否有官方导出文件作为无 OAuth 降级路径？** ✅ **有**，人工导出 **ZIP(Markdown+CSV)/HTML/PDF**。反之，**通过 API 做整库导出不属于标准 API 能力**（仅 Enterprise Admin API beta）。

### C.5 桌面自建 OAuth 可行性总排序（对导入功能选型的直接影响）

| 平台 | 客户端模型 | 需 `client_secret`？ | PKCE | 回环重定向官方支持 | 落地难度 |
| --- | --- | --- | --- | --- | --- |
| **Google Tasks** | 公共客户端（Desktop app） | ❌ 否 | ✅ 支持 | ✅ **官方明确支持 loopback IP** | **最低** |
| **Microsoft To Do** | 公共客户端（Mobile and desktop） | ❌ 否 | — （官方未强调） | ✅ **官方明确给出 `http://localhost`** | 低 |
| **Todoist** | 机密客户端 | ✅ **是** | ❌ 无 | ⚠️ **未核实** | 中（需内置密钥 + 验证回环） |
| **Notion** | 机密客户端 | ✅ **是** | ❌ 未文档化 | ⚠️ **未核实** | 中（同上） |

> **选型建议**
> - **优先做 OAuth 集成**：Google Tasks 与 Microsoft To Do（公共客户端，官方文档完整，无密钥保护难题）。
> - **优先做文件导入**：Todoist（**CSV 列名与语义官方逐字文档化，且同格式可直接回写导入** —— 四家中最稳的结构化解析目标）与 Notion（人工导出 ZIP）。
> - **风险前置**：Todoist 与 Notion 的「回环重定向是否被接受」**均无官方记载**，必须在功能立项后**第一件事做实机验证**，不要等到实现末期。
> - **安全提醒**：Todoist 与 Notion 要求把 `client_secret` 打进桌面客户端。桌面二进制中的密钥**可被提取**，应视为"混淆"而非"保密"；若平台允许，优先让用户自建应用并填入自己的 client_id/secret（BYO-App 模式），以规避分发密钥的合规风险。

---

## D. Rust 侧 crates

> 详细版见同级文件 **`docs/api-research-rust-crates.md`**（含 D1–D6 逐题核实、MSRV 汇总、15 项未核实清单）。
> **访问日期统一为 2026-09-23**。版本号取自 `crates.io/api/v1/crates/<name>` 的 `max_stable_version`，许可证与 MSRV 取自同版本记录。

### D.0 版本总表

| crate | 确切版本 | 发布日期 | 许可证 | MSRV | 官方 URL | 访问日期 |
| --- | --- | --- | --- | --- | --- | --- |
| `sqlx` | **0.9.0** | 2026-07-20 | MIT OR Apache-2.0 | **1.94.0** | <https://docs.rs/sqlx/0.9.0/sqlx/> · <https://crates.io/crates/sqlx> | 2026-09-23 |
| `serde` | **1.0.229** | — | MIT OR Apache-2.0 | — | <https://crates.io/crates/serde> | 2026-09-23 |
| `serde_json` | **1.0.151** | — | MIT OR Apache-2.0 | — | <https://crates.io/crates/serde_json> | 2026-09-23 |
| `chrono` | **0.4.45** | — | MIT OR Apache-2.0 | — | <https://crates.io/crates/chrono> | 2026-09-23 |
| `chrono-tz` | **0.10.4** | — | MIT OR Apache-2.0 | — | <https://docs.rs/chrono-tz/0.10.4/chrono_tz/> | 2026-09-23 |
| `rrule` | **0.14.0** | 2025-04-20 | MIT OR Apache-2.0 | — | <https://docs.rs/rrule/0.14.0/rrule/> | 2026-09-23 |
| `uuid` | **1.26.1** | — | Apache-2.0 OR MIT | 1.85.0 | <https://crates.io/crates/uuid> | 2026-09-23 |
| `tokio` | **1.53.1** | — | **MIT（单许可，非双许可）** | — | <https://crates.io/crates/tokio> | 2026-09-23 |
| `keyring` | **4.2.0** | — | MIT OR Apache-2.0 | 1.88.0 | <https://docs.rs/keyring/4.2.0/keyring/> | 2026-09-23 |
| `reqwest` | **0.13.5** | 2026-09-08 | MIT OR Apache-2.0 | **1.85.0** | <https://docs.rs/reqwest/0.13.5/reqwest/> | 2026-09-23 |
| `thiserror` | **2.0.20** | — | MIT OR Apache-2.0 | — | <https://crates.io/crates/thiserror> | 2026-09-23 |
| `anyhow` | **1.0.104** | — | MIT OR Apache-2.0 | — | <https://crates.io/crates/anyhow> | 2026-09-23 |
| `csv` | **1.4.0** | 2025-10-17 | **Unlicense/MIT** | 1.73 | <https://docs.rs/csv/1.4.0/csv/> | 2026-09-23 |
| `printpdf` | **0.12.8** | 2026-09-05 | MIT | 1.88.0 | <https://docs.rs/printpdf/0.12.8/printpdf/> | 2026-09-23 |
| `genpdf` | **0.2.0** | **2021-06-17** | Apache-2.0 OR MIT | — | <https://docs.rs/genpdf/0.2.0/genpdf/> | 2026-09-23 |
| `typst` | **0.15.1** | 2026-07-17 | Apache-2.0 | 1.92 | <https://crates.io/crates/typst> | 2026-09-23 |

### D.1 sqlx（SQLite + 迁移 + 事务）

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 / MSRV | 0.9.0 / MIT OR Apache-2.0 / **MSRV 1.94.0** | <https://crates.io/crates/sqlx/0.9.0> | 2026-09-23 |
| `migrate!` 签名 | `macro_rules! migrate { ($dir:literal) => {...}; () => {...} }` | <https://docs.rs/sqlx/latest/sqlx/macro.migrate.html> | 2026-09-23 |
| **必需 feature** | **仅 `macros` + `migrate` 两 feature 下可用**（文档原文：*"Available on crate features `macros` and `migrate` only."*） | 同上 | 2026-09-23 |
| 默认目录 | **`"./migrations"`** | 同上 | 2026-09-23 |
| 路径基准 | **相对于含 `Cargo.toml` 的项目根**，而非调用处源文件（官方原文：*"The directory must be relative to the project root (the directory containing Cargo.toml), unlike `include_str!()`"*） | 同上 | 2026-09-23 |
| 官方用法 | `sqlx::migrate!("db/migrations").run(&pool).await?;` 或 `static MIGRATOR: Migrator = sqlx::migrate!();` | 同上 | 2026-09-23 |
| 迁移文件名格式 | `<VERSION>_<DESCRIPTION>.sql`；`VERSION` 需可解析为 **i64 且 > 0**，**不匹配者被静默忽略** | 同上 | 2026-09-23 |
| 配置文件 | 支持 `sqlx.toml`（可改迁移表名、重定位目录、忽略哈希字符）；文档含 *"Triggering Recompilation on Migration Changes"*（`build.rs` 打印 `cargo:rerun-if-changed`，或 nightly 用 cfg flag） | 同上 | 2026-09-23 |
| `sqlite` feature 的真实含义 | ⚠️ **`sqlite = [sqlite-bundled, sqlite-deserialize, sqlite-load-extension, sqlite-unlock-notify]`** —— 即 **`sqlite` 默认就是 `sqlite-bundled`：从源码静态编译 SQLite，需要 C 构建工具链**。动态链接系统 SQLite 需显式改用 **`sqlite-unbundled`** | <https://crates.io/api/v1/crates/sqlx/0.9.0>（`version.features`） | 2026-09-23 |
| `default` feature 展开 | `default = [any, macros, migrate, json]` | 同上 | 2026-09-23 |
| `SqlitePoolOptions` | **是类型别名** `pub type SqlitePoolOptions = PoolOptions<Sqlite>;` —— 因此 docs.rs 上 `struct.SqlitePoolOptions.html` 会 404，应查 **`PoolOptions`** 页 | <https://docs.rs/sqlx/latest/sqlx/pool/struct.PoolOptions.html> | 2026-09-23 |
| SQLite 事务 | `Pool::begin` / `try_begin` 正常可用 | 同上 | 2026-09-23 |
| 事务的迁移哈希跨平台风险 | Windows 的 CRLF 会导致迁移校验哈希在跨平台间不可复现 → 建议 `.gitattributes` 写 **`*.sql text eol=lf`** | 官方迁移文档 | 2026-09-23 |

### D.2 序列化 / 时间 / 重复规则 / ID / 异步运行时

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `serde` derive | feature 名就是 **`derive`**：`serde = { version = "1", features = ["derive"] }` | <https://crates.io/crates/serde> | 2026-09-23 |
| `chrono-tz` 机制 | build script 用 **IANA 时区数据静态生成** `Tz` 枚举；**"Dynamic tzdata loading" 被官方 README 列在 "Future Improvements"** → 反证当前**不读 OS 时区库** | <https://docs.rs/chrono-tz/0.10.4/chrono_tz/> | 2026-09-23 |
| chrono-tz 的 Windows 专属 caveat | **官方无记载 → 未核实**。另注：官方**没有**"does not read the OS timezone database"这类逐字表述，上述结论是从 Future Improvements 推出的 | 同上 | 2026-09-23 |
| **`rrule` crate 0.14.0 能力** | ✅ **`BYSETPOS` 支持**（`by_set_pos` / `get_by_set_pos`）；✅ **数字前缀 `BYDAY` 支持**（`pub enum NWeekday { Every(Weekday), Nth(i16, Weekday) }`，文档原文举例 `Nth(-1, MO)` = 最后一个周一）；`RRuleSet` 含 RDATE/EXDATE/EXRULE（`EXRULE` 需 `exrule` feature，`BYEASTER` 需 `by-easter`） | <https://docs.rs/rrule/0.14.0/rrule/> | 2026-09-23 |
| **`rrule` crate 维护状态** | ⚠️ **上游已停更**：0.14.0 发布 **2025-04-20**，**仓库最后 push 与发布同日**，此后约 17 个月无推送；积压 **34 个 open issues**；docs.rs 显示**仅 27.55% 有文档** | 同上 + GitHub API | 2026-09-23 |
| **rrule.js vs rrule crate 对比** | **两者都处于停更状态，但 rrule.js 停更更久**：rrule.js 2.8.1 = **2023-11-10**（近 3 年零发布，214 open issues）；rrule crate 0.14.0 = 2025-04-20（约 17 个月）。**"谁的功能更完整"本次未下断言**（rrule.js 侧能力未逐项核实） | — | 2026-09-23 |
| `uuid` features | `v4`、`v7`、`serde` **均存在** | <https://crates.io/crates/uuid> | 2026-09-23 |
| `tokio` 许可证 | ⚠️ **单 `MIT`**，不是常见的 `MIT OR Apache-2.0` 双许可 —— 影响许可证合规清单的写法 | <https://crates.io/crates/tokio> | 2026-09-23 |
| `tokio` 最小 feature 集（Tauri 应用） | `rt-multi-thread`、`macros`、`sync`、`time`、`fs` | 官方文档 | 2026-09-23 |

### D.3 keyring（Windows 凭据管理器）

**⭐ keyring v4 是一次"库 + 后端拆分"的重构，feature 名与 v3 完全不同。**

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 当前版本 / 许可证 | **4.2.0** / MIT OR Apache-2.0 | <https://docs.rs/keyring/4.2.0/keyring/> | 2026-09-23 |
| **v4.2.0 的完整 feature 列表（逐字，权威）** | 仅三个：<br>`default = ["v1"]`<br>`v1 = ["apple-native-keyring-store/keychain", "windows-native-keyring-store", "zbus-secret-service-keyring-store"]`<br>`cli = [android-native-keyring-store, apple-native-keyring-store/keychain, apple-native-keyring-store/protected, db-keystore, dbus-secret-service-keyring-store, keyring-core/sample, linux-keyutils-keyring-store, windows-native-keyring-store, zbus-secret-service-keyring-store]` | <https://crates.io/api/v1/crates/keyring/4.2.0>（`version.features`） | 2026-09-23 |
| **❌ `windows-native` feature 在 v4 中不存在** | 该名字是 **v3.6.3** 的：`windows-native = ["dep:windows-sys", "dep:byteorder"]`（v2.3.3 时代还叫 `platform-windows`）。**在 v4 中写 `features = ["windows-native"]` 会导致 cargo 报错 "does not have these features"** | <https://crates.io/api/v1/crates/keyring/3.6.3> | 2026-09-23 |
| **✅ Windows 正确用法（v4）** | **`keyring = "4"` 即可** —— 默认 feature `v1` 已包含 `windows-native-keyring-store`，**Windows 上自动使用 Windows 凭据管理器**，无需任何额外 feature | <https://crates.io/api/v1/crates/keyring/4.2.0> | 2026-09-23 |
| 官方推荐的进阶用法 | 需要精细控制 store 的应用，官方明确说**不应链接 `keyring` 本身**，而应链 **`keyring-core`** + 具体 store crate，并初始化：<br>`keyring_core::set_default_store(windows_native_keyring_store::Store::new().unwrap());` | <https://docs.rs/keyring/4.2.0/keyring/> | 2026-09-23 |
| v1 / cli 两种模式 | `v1`：行为与 keyring v1 一致，提供跨平台读写密码的 `Entry` API；`cli`：为 CLI 示例应用/`rust-native-keyring`/`keyring-demo` 提供"接管所有可用凭据库"的粘合层。文档警告 `cli` 会给应用拖入大量不需要的依赖模块 | 同上 | 2026-09-23 |
| 是否该钉 v3 | **不推荐**。v3 末版 3.6.3（2025-07-27）后停止演进；仅在"不介意无修复、且想保低 MSRV(1.75)"时才考虑 | <https://crates.io/crates/keyring/versions> | 2026-09-23 |
| **`tauri-plugin-keyring` 非官方** | ⚠️ 第三方个人插件（owner **HuakunShen**，`repository` 字段为 **null**）。官方 `tauri-apps/plugins-workspace/plugins` 的 30 个目录（autostart…stronghold…window-state）中**没有 keyring**。它**只发过 1 个版本**（0.1.0，2024-12-23），且**依赖 `keyring ^3.6.1`**，与 keyring 4.x 不兼容 | <https://crates.io/crates/tauri-plugin-keyring> | 2026-09-23 |
| 官方对应的安全存储能力 | 是 **`stronghold`**（官方插件） | <https://v2.tauri.app/plugin/stronghold/> | 2026-09-23 |

### D.4 reqwest

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| 版本 / 许可证 / MSRV | **0.13.5** / MIT OR Apache-2.0 / **MSRV 1.85.0** | <https://crates.io/crates/reqwest/0.13.5> | 2026-09-23 |
| **⭐ 默认 TLS 后端已变更** | **0.13 起默认 TLS 是 `rustls`**，不再是 `native-tls`/SChannel。官方 CHANGELOG v0.13.0 破例性变更原文：*"`rustls` is now the default TLS backend, instead of `native-tls`."* / *"`rustls-tls` has been renamed to `rustls`."* | <https://docs.rs/reqwest/0.13.5/reqwest/> | 2026-09-23 |
| **feature 命名变更** | ❌ **`rustls-tls` 在 0.13 已不存在**；✅ 正确名为 **`rustls`**。0.12 时代的整个 `rustls-tls-*` 系列（如 `rustls-tls-webpki-roots`）在 0.13 中**全部移除** | <https://crates.io/api/v1/crates/reqwest/0.13.5>（`version.features`） | 2026-09-23 |
| **0.13.5 完整 feature 列表（逐字）** | `__native-tls, __native-tls-alpn, __rustls, __rustls-aws-lc-rs, __tls, blocking, brotli, charset, cookies, default, default-tls, deflate, form, gzip, hickory-dns, http2, http3, json, multipart, native-tls, native-tls-no-alpn, native-tls-vendored, native-tls-vendored-no-alpn, query, rustls, rustls-no-provider, socks, stream, system-proxy, zstd` | 同上 | 2026-09-23 |
| **⭐ `webpki-roots` 不是有效 feature 名** | ❌ 0.13.5 的 feature 列表中**没有 `webpki-roots`**。历史上该名字也从未作为独立 feature 存在——reqwest 0.11/0.12 的对应 feature 名为 **`rustls-tls-webpki-roots`**（0.11.27 与 0.12.24 均已核实存在），0.13 则整体改为 `rustls`（内部使用 `rustls-platform-verifier`）。**写出 `features = ["webpki-roots"]` 必然导致 cargo 报错** | 同上 + <https://crates.io/api/v1/crates/reqwest/0.11.27>、`/0.12.24` | 2026-09-23 |
| `default` feature 展开 | `default = ["default-tls", "charset", "http2", "system-proxy"]`；`default-tls = ["rustls"]` | 同上 | 2026-09-23 |
| **⚠️ `default-features = false` 的连带损失** | 会同时丢掉 **`system-proxy`**（`hyper-util/client-proxy-system`）与 **`http2`**。**`system-proxy` 的丢失意味着走系统代理的用户会静默直连失败**（对中国大陆用户访问境外 API 影响显著）。修法：去掉 `default-features = false`，或显式补上 `"http2"`、`"system-proxy"`（`charset` 视需要） | 同上 | 2026-09-23 |
| **超时方法（均已核实存在）** | `timeout(Duration)` —— **总截止时间，默认无超时**；`connect_timeout(Duration)` —— 默认 `None`，需 tokio timer；`read_timeout(Duration)` —— **每次读操作**的超时，成功读后重置；`pool_idle_timeout<D: Into<Option<Duration>>>(D)` —— **默认 90 秒**；`pool_max_idle_per_host(usize)` | <https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html> | 2026-09-23 |
| **流式（SSE）超时建议** | **不要给流式请求设总 `timeout`**（会在长响应中途掐断），应改用 **`read_timeout`** | 同上 | 2026-09-23 |

### D.5 thiserror / anyhow

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| `thiserror` | 2.0.20，MIT OR Apache-2.0 | <https://crates.io/crates/thiserror> | 2026-09-23 |
| `anyhow` | 1.0.104，MIT OR Apache-2.0 | <https://crates.io/crates/anyhow> | 2026-09-23 |
| thiserror 2.x 能力 | `#[derive(Error, Debug)]` + `#[error("...")]` + `#[from]` 均已确认（官方原文：*"A `From` impl is generated for each variant that contains a `#[from]` attribute."*） | <https://docs.rs/thiserror/2.0.20/thiserror/> | 2026-09-23 |
| 选用建议 | **库/领域层用 `thiserror`**（类型化错误），**应用/命令层用 `anyhow`** | 同上 | 2026-09-23 |
| **⚠️ Tauri 边界坑** | **`anyhow::Error` 不实现 `Serialize`** —— Tauri command 的返回错误**必须转成可序列化的类型**，不能直接把 `anyhow::Error` 抛过 IPC 边界 | 同上 | 2026-09-23 |

### D.6 CSV 与 PDF

| 项目 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| CSV 库 | **`csv` 1.4.0**（2025-10-17），许可证 **Unlicense/MIT**，MSRV 1.73，**无任何 feature**（serde 集成内置） | <https://docs.rs/csv/1.4.0/csv/> | 2026-09-23 |
| **⚠️ csv 易错点** | **`serialize` 挂在 `Writer` 上，不在 `WriterBuilder` 上**。正确写法：`WriterBuilder::new().from_writer(...)` / `.from_path(...)` 先拿到 `Writer`，再 `writer.serialize(record)` | 同上 | 2026-09-23 |
| `printpdf` | **0.12.8**（2026-09-05），MIT，MSRV 1.88 —— **活跃维护**（发版日仓库同日 push）。**低层 API** | <https://docs.rs/printpdf/0.12.8/printpdf/> | 2026-09-23 |
| `genpdf` | **0.2.0**，Apache-2.0 OR MIT —— ⚠️ **事实上已停止维护**：最后发版 **2021-06-17**，全部历史仅 5 个版本，SourceHut 仓库最新提交显示 *"5 years ago"*。**高层 API**（建立在 printpdf + rusttype 之上） | <https://docs.rs/genpdf/0.2.0/genpdf/> | 2026-09-23 |
| `typst`（参考） | **0.15.1**，Apache-2.0，MSRV 1.92，56k stars，2026-09-22 仍在 push —— **极活跃** | <https://crates.io/crates/typst> | 2026-09-23 |
| `wkhtmltopdf`（参考） | ❌ **仓库已 archived**（最后 push 2022-11-22） | — | 2026-09-23 |

**⭐ PDF 方案推荐：优先在前端生成**

| 论据 | 结论 | 官方 URL | 访问日期 |
| --- | --- | --- | --- |
| printpdf 内置字体对非 ASCII 的表现 | **直接乱码**。issue #273 原文：*"Built-in fonts emit UTF-8 bytes into a WinAnsiEncoding content stream (all non-ASCII text is mojibake'd)"*（后随 PR #274 修复为 WinAnsi 编码）→ **中文必须自带并嵌入 TTF/OTF 字体** | printpdf GitHub issue #273 | 2026-09-23 |
| CJK 子集化的历史稳定性 | 反复出 bug（issue #212 / #222 / #250）；**#283** *"examples: add multiple_fonts POC — single CJK OTC, per-language glyph variants, subsetting"* **至今仍是 open** | printpdf GitHub issues | 2026-09-23 |
| genpdf | 同样要求显式提供字体目录，且已停维护（见上） | <https://docs.rs/genpdf/0.2.0/genpdf/> | 2026-09-23 |
| 前端方案的优势 | 浏览器侧**零字体工程**（Windows 自带中文字体），无需子集化 | — | 2026-09-23 |
| **⚠️ 该推荐的关键前提未核实** | **Tauri 2 / Windows WebView2 上 `window.print()` 的确切行为未核实**（是否弹打印对话框、能否直接导出 PDF、静默打印是否需额外权限）。**建议实现前先做最小验证** | — | 2026-09-23 |

---

## 附：项目现状对照（本次调研对 `D:\Todo` 实际配置的直接核对）

> 本节是对仓库中**既有文件实际内容**的核对结果，与上文的"应该用什么版本"互为对照。
> **注意**：`src-tauri/Cargo.toml` 的当前内容**已经**正确处理了 keyring 与 reqwest TLS 的改名问题（文件内有明确注释），但仍有下文标注的**真实缺陷**。

### 附.1 `package.json` 依赖现状（`D:\Todo\package.json`，访问日期 2026-09-23）

| 包 | 项目声明 | 当前 latest | 状态 |
| --- | --- | --- | --- |
| `react` / `react-dom` | `^19.3.0` | 19.3.0 | ✅ 一致 |
| `@vitejs/plugin-react` | `^6.1.1` | 6.1.1 | ⚠️ **见附.2 的 peer 冲突** |
| `vite` | `^7.1.0`（lockfile 实装 **7.3.6**） | **8.3.0** | ⚠️ 落后一个大版本 |
| `typescript` | `^5.9.0`（lockfile 实装 **5.9.3**） | **7.0.2** | ⚠️ 落后两个大版本（官方模板锁 `~6.0.3`） |
| `vitest` | `^3.2.4`（lockfile 实装 **3.2.7**） | **5.0.1** | ⚠️ 落后两个大版本 |
| `date-fns` | `^4.4.0` | 4.4.0 | ✅ 一致 |
| `rrule` | `^2.8.1` | 2.8.1 | ✅ 一致（但注意该包已停更，见 §B.2） |
| `zustand` | `^5.0.15` | 5.0.15 | ✅ 一致 |
| `zod` | `^4.6.5` | 4.6.5 | ✅ 一致 |
| `recharts` | `^3.10.1` | 3.10.1 | ✅ 一致 |
| `react-markdown` | `^10.1.0` | 10.1.0 | ✅ 一致 |
| `remark-gfm` | `^4.0.1` | 4.0.1 | ✅ 一致 |
| `@dnd-kit/core` / `sortable` | `^6.3.1` / `^10.0.0` | 6.3.1 / 10.0.0 | ✅ 一致（但注意已停更，见 §B.3） |
| **`rehype-sanitize`** | **未声明** | 6.0.0 | ❌ **缺失**。若渲染不可信 Markdown，必须补上（见 §B.6） |
| **`@tanstack/react-virtual`** | **未声明** | 3.14.13 | ❌ **缺失**。任务是应对上千条任务（见 §B.9） |
| **`eslint` / `prettier`** | **未声明** | 10.11.0 / 3.9.8 | ❌ **缺失**，但 `package.json` 的 `scripts` 里已有 `lint`/`format` 命令 → **命令会失败** |
| **日历库** | **未声明** | — | 需要日历视图时再选型（见 §B.8） |

### 附.2 ⚠️ 真实的 peer 依赖冲突（高优先级）

| 项目 | 事实 | 来源 | 访问日期 |
| --- | --- | --- | --- |
| 冲突 | 项目装 **`vite@^7.1.0`**（实装 7.3.6），但 **`@vitejs/plugin-react@6.x` 的全部版本（6.0.0–6.1.1）peer 均要求 `vite: ^8.0.0`** | <https://crates.io> → npm registry `@vitejs/plugin-react` 各版本 `peerDependencies` | 2026-09-23 |
| 逐版本证据 | `6.0.0` / `6.0.1` / `6.0.2` / `6.0.3` / `6.0.4` / `6.0.5` / `6.1.0` / `6.1.1` → **全部** `vite=^8.0.0` | 同上 | 2026-09-23 |
| 兼容 Vite 7 的最后版本 | **`@vitejs/plugin-react@5.0.4`**（2025-09-27），peer = `vite: ^4.2.0 \|\| ^5.0.0 \|\| ^6.0.0 \|\| ^7.0.0` | 同上 | 2026-09-23 |
| 结论 | 二者必居其一：**① 把 Vite 升到 8.x**（与 A 节官方模板基线一致，推荐）**② 把 plugin-react 降到 5.0.4**。当前组合在 pnpm 严格 peer 检查下会报警告/失败 | — | 2026-09-23 |

### 附.3 ⚠️ `src-tauri/Cargo.toml` 的真实缺陷（已逐条实证）

| # | 位置 | 现状 | 问题 | 严重度 |
| --- | --- | --- | --- | --- |
| 1 | 第 72–78 行 `reqwest` features | `features = ["json", "rustls", "webpki-roots", "stream", "gzip"]` | ❌ **`webpki-roots` 不是有效的 reqwest feature 名**。0.13.5 的 feature 列表中**没有**它（完整列表见 §D.4），历史上也从未作为独立 feature 存在（0.11/0.12 叫 `rustls-tls-webpki-roots`，0.13 已整体移除）。**这会导致 `cargo build` 直接报 "does not have these features"** | 🔴 **必然编译失败** |
| 2 | 第 72 行 `reqwest` | `default-features = false` | ⚠️ 连带丢掉 **`system-proxy`** 与 **`http2`**。前者丢失会让**走系统代理的用户静默直连失败**（对国内访问境外 AI API 影响显著）。修法：去掉 `default-features = false`，或显式补 `"http2"`、`"system-proxy"` | 🟠 功能静默失效 |
| 3 | 第 7 行 `rust-version` | `rust-version = "1.77"` | ❌ **与依赖的 MSRV 不自洽**：`sqlx 0.9.0` 要求 **1.94.0**（另 `keyring 4.2.0`/`printpdf 0.12.8` = 1.88，`reqwest 0.13.5`/`uuid 1.26.1` = 1.85）。应改为 **`1.94`** | 🟠 声明错误 |
| 4 | 迁移文件的换行符 | 无 `.gitattributes` | ⚠️ Windows 的 **CRLF 会导致 sqlx 迁移哈希跨平台不可复现**（sqlx 官方文档明示）。建议加 `.gitattributes`：`*.sql text eol=lf`；并让 `build.rs` 打印 `cargo:rerun-if-changed=migrations` | 🟡 跨平台隐患 |
| 5 | 第 68 行 `keyring = "4"` | ✅ **正确** | 文件内注释已正确指出"keyring 4.2.0 仅暴露 `cli`/`default`/`v1`，不存在 `windows-native`"。`keyring = "4"` 的默认 `v1` feature 已含 `windows-native-keyring-store`，**Windows 上自动使用凭据管理器** | ✅ 无需修改 |
| 6 | 第 72–78 行 reqwest `"rustls"` | ✅ **正确** | 已正确使用 0.13 的新名 `rustls`（而非旧 `rustls-tls`） | ✅ 无需修改 |

> **关于 def 1 的补充说明**：`rustls` 在 0.13.5 中展开为 `rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]` —— **证书校验走 `rustls-platform-verifier`（即系统信任库）**。因此原先想用 `webpki-roots` 达成的"不依赖系统证书库"目标，在 0.13 中需要重新评估实现路径（本次**未核实** 0.13 是否还提供等价的 webpki-roots 选项）。

### 附.4 `vite.config.ts` 与官方模板的差异（`D:\Todo\vite.config.ts`）

| 配置项 | 项目实际值 | 官方模板/指南 | 评价 |
| --- | --- | --- | --- |
| `clearScreen` | `false` | `false` | ✅ 一致 |
| `server.port` | `1420` | `1420`（模板） | ✅ 一致 |
| `server.strictPort` | `true` | `true` | ✅ 一致 |
| `server.host` / `hmr` | `host \|\| false`；`hmr` 走 1421 | 同 | ✅ 一致 |
| `server.watch.ignored` | `['**/src-tauri/**']` | 同 | ✅ 一致 |
| `envPrefix` | `['VITE_', 'TAURI_ENV_']` | 指南为 `['VITE_', 'TAURI_ENV_*']` | ⚠️ 少了 `*`。Vite 的 `envPrefix` 为前缀匹配，**此差异大概率不影响功能**，但建议与官方写法对齐以降低歧义 |
| `build.target` | `'chrome120'` | 指南为 `'chrome105'`（Windows） | ⚠️ 更激进。**要求用户 WebView2 版本足够新**；官方基线是 chrome105。若需兼容旧 WebView2 应下调 |
| `build.minify` | `process.env.TAURI_ENV_DEBUG ? false : 'esbuild'` | 指南为 `!process.env.TAURI_ENV_DEBUG`（默认 minifier） | ⚠️ 显式指定 `'esbuild'`。**Vite 8 已转向 Rolldown/Oxc**，升级 Vite 8 时该值可能需要改（本次**未核实** Vite 8 是否仍接受 `'esbuild'`） |
| `build.sourcemap` | `!!process.env.TAURI_ENV_DEBUG` | 同 | ✅ 一致 |
| `test` 字段 | 内联在 `vite.config.ts`，`defineConfig` 来自 `'vite'` | — | ⚠️ `test` 是 Vitest 配置。用 `vite` 的 `defineConfig` **不会有 `test` 的类型**，应改从 **`vitest/config`** 导入 `defineConfig`，否则 TS 会报错 |
| `manualChunks` | `{ react: [...], charts: ['recharts'] }` | 模板无 | ✅ 合理增强（recharts 依赖重，单独分包正确） |

---

## 一页速查：建议锁定的版本

```jsonc
// package.json（与 2026-09-23 的 latest 对齐）
"dependencies": {
  "react": "^19.3.0",
  "react-dom": "^19.3.0",
  "zustand": "^5.0.15",
  "zod": "^4.6.5",
  "date-fns": "^4.4.0",
  "@date-fns/tz": "^1.5.0",
  "rrule": "^2.8.1",
  "@dnd-kit/core": "^6.3.1",
  "@dnd-kit/sortable": "^10.0.0",
  "react-markdown": "^10.1.0",
  "remark-gfm": "^4.0.1",
  "rehype-sanitize": "^6.0.0",          // 当前缺失，渲染不可信 Markdown 必装
  "@tanstack/react-virtual": "^3.14.13", // 当前缺失，上千条任务必装
  "recharts": "^3.10.1",
  "@tauri-apps/api": "^2.11.1"
},
"devDependencies": {
  "vite": "^8.3.0",                      // 必须 >=8 才能配 plugin-react 6.x
  "@vitejs/plugin-react": "^6.1.1",
  "typescript": "~6.0.3",                // 官方模板基线；latest 为 7.0.2，升级需单独评估
  "vitest": "^5.0.1",
  "@playwright/test": "^1.63.0",
  "eslint": "^10.11.0",                  // 注意：9.x 已于 2026-08-06 EOL
  "prettier": "^3.9.8"
}
```

```toml
# src-tauri/Cargo.toml 关键片段（已修正）
rust-version = "1.94"                    # 由 sqlx 0.9.0 的 MSRV 决定

sqlx = { version = "0.9", default-features = false, features = [
    "runtime-tokio", "sqlite", "macros", "migrate", "chrono", "uuid", "json",
] }                                       # 注：sqlite = sqlite-bundled，需 C 工具链

keyring = "4"                             # ✅ 默认 v1 已含 Windows 凭据管理器

reqwest = { version = "0.13", features = [ # 去掉 default-features=false
    "json", "rustls", "stream", "gzip", "http2", "system-proxy",
] }                                       # ❌ 删除无效的 "webpki-roots"
```

```gitattributes
*.sql text eol=lf    # 保证 sqlx 迁移哈希跨平台可复现
```
