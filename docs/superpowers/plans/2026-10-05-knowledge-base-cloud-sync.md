# 知识库云同步实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将导入的知识库资料原件和解析结果安全纳入现有 WebDAV 同步。

**Architecture:** 在业务事件与批次格式上复用现有冲突/加密机制，但按事件类型拆分知识库批次及父链，并发布单独的加密知识库 head，确保旧版严格校验的同步头仍兼容。文件按内容哈希去重，接收后由本机重建 FTS5 索引。

**Tech Stack:** Rust, SQLite/sqlx migrations, AES-GCM WebDAV sync, React/TypeScript, Vitest, Cargo tests.

**Spec:** `docs/superpowers/specs/2026-10-05-knowledge-base-cloud-sync.md`

## Global Constraints

- 只有 `all` 范围同步知识库资料；`tasks` 与 `memos` 的既有范围不扩大。
- 原件上限 20 MiB、解析正文上限 100,000 字符、云端密文资源上限 40 MiB。
- 旧版 `head.json` schema 不变；知识库必须使用独立 head 与包链。
- 接收端不得重跑 AI/OCR；只在本机重建派生索引。
- 同步、解密、摘要校验、数据库写入失败必须向上返回错误，不能伪成功。
- 不要求清库；若加正式迁移，迁移前必须通过现有一致性快照机制。

## Review Focus

- 同一文件被两台设备各自导入：稳定身份与资产映射不会违反唯一约束。
- 损坏/缺失/与来源哈希不符的云端原件：不会覆盖本地或标记同步成功。
- 独立知识库包链：普通业务 head 和旧版客户端永远看不到知识库包。
- 只选任务或备忘/流程范围：知识库内容既不打包上传也不拉取。
- 知识来源删除：FTS 触发器及流程引用保护均有效。

---

### Task 1: 稳定知识身份及本机索引重建

**Files:**
- Modify: `src-tauri/src/cloud_sync/business.rs`
- Modify: `src-tauri/src/knowledge_base.rs`
- Test: Rust module tests in those files

**Interfaces:**
- `business::capture_scope(db, include_knowledge)` and `business::apply_scope(db, include_knowledge)` select knowledge records only for the all-data scope.
- `knowledge_base::rebuild_synced_source(conn, source_id)` rebuilds chunks and FTS rows in the caller's transaction.

- [x] Write tests for stable SHA identity, local alias mapping for independently imported docs, remote source indexing, and safe removal of flow-referenced assets.
- [x] Run the end-to-end import test before implementation and verify it failed because the remote source row was missing.
- [x] Add `knowledge_sources` to the business table allowlist; generate stable source/asset aliases by SHA-256; normalize device-local timestamps; validate size/hash/status constraints.
- [x] Rebuild local chunks/FTS after knowledge source upsert and preserve referenced assets after tombstone deletion.
- [x] Run targeted Rust tests and verify they pass.

### Task 2: Isolated encrypted knowledge packet stream

**Files:**
- Modify: `src-tauri/src/cloud_sync/business.rs`
- Modify: `src-tauri/src/cloud_sync.rs`
- Test: `src-tauri/src/cloud_sync/tests.rs`

**Interfaces:**
- `published_heads(db)` returns ordinary business heads; `published_knowledge_heads(db)` returns only knowledge packet heads.
- `fetch(..., root, knowledge_only)` validates packet homogeneity and imports a source's original asset before applying its event.
- `KnowledgeHead` is encrypted at `devices/<device>/knowledge-head.json`; `head.json` remains unchanged.

- [x] Write tests proving packet parents and published heads are separated by event class and that the original head format contains no KB roots.
- [x] Run the end-to-end All-scope transfer test before implementation and verify it failed because the receiving source row was absent.
- [x] Split packet generation, packet-parent selection, upload selection, and fetch validation by event class.
- [x] Publish/read the separate encrypted knowledge head only for All scope; upload and download source originals with size and SHA validation.
- [x] Run cloud sync integration tests for two independent DBs, All/Tasks/Memos scopes, deleted sources, duplicate imports, and missing originals.

### Task 3: User-visible sync scope and device compatibility

**Files:**
- Modify: `src-tauri/src/cloud_sync.rs`
- Modify: `src/components/CloudSettings.tsx`
- Modify: `src/components/KnowledgeBase.tsx`
- Tests: cloud settings / knowledge-base component tests

**Compatibility:** Older clients ignore the separate knowledge head and continue syncing the original business head. Each computer that should receive knowledge documents must run a compatible version.

- [x] Test the three sync-scope explanations and user-visible KB sync privacy/scope copy.
- [x] Keep KB out of non-All upload, download, and head publication; preserve legacy head schema for older clients.
- [x] Update knowledge-base and cloud settings copy to explain All-only document sync, flow scope, encryption, local indexing, and the need to upgrade each computer.
- [x] Run frontend and Rust targeted tests.

### Task 4: Documentation, version, and release gates

**Files:**
- Modify: `README.md`
- Modify: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, Tauri version config, release notes
- Modify: `docs/work-log.md`

- [x] Add the design and plan to README's document index; update version to 0.4.28 and release notes.
- [x] Run `pnpm install --frozen-lockfile`, `pnpm typecheck`, `pnpm test`, `pnpm build`, `pnpm lint`, `cargo fmt --check`, `cargo test --lib`, and strict Clippy after build.
- [x] Run the three repository secret/artifact checks from `AGENTS.md`; inspect expected documentation-only grep matches.
- [x] Append the work-log round with done/not done/verification/documents.
- [x] Commit each logical change in Chinese and push the requested GitHub branch; verify GitHub Actions.
