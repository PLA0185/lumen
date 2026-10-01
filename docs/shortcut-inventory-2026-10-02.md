# Shortcut inventory — 2026-10-02

Read-only code reconnaissance of D:/Codex/Lumen; no runtime UI, production data, builds, tests, or repository changes. IDs below are recommended stable IDs, except existing global configuration field names. Actual Settings component is SettingsView, embedding WindowSettings; no SettingsWindow exists.

## Application commands

| Recommended ID / existing field | Current binding | Scope and registration / source |
| --- | --- | --- |
| window.toggle / shortcutToggle | CmdOrCtrl+Alt+A | OS global, Rust shortcuts::reload, src-tauri/src/shortcuts.rs:54, callback :88; defaults window_mgr.rs:133 |
| window.quickAdd / shortcutQuickAdd | CmdOrCtrl+Alt+N | OS global, same registration :55, callback :89; defaults :134 |
| window.today / shortcutToday | CmdOrCtrl+Alt+D | OS global, same registration :56, callback :107; defaults :135 |
| window.floating / shortcutFloating | Alt+Q | OS global, same registration :57, callback :90; defaults :136 |
| task.quickAdd | Ctrl or Meta+n | App window keydown, src/App.tsx:291-317; new task except memos/organize explanatory toast; some views switch to inbox |
| app.search | Ctrl or Meta+f | Same app handler, src/App.tsx:307; focus .search__input |
| app.refresh | Ctrl or Meta+r | Same app handler, src/App.tsx:310; reload |
| search.submit | Enter | Search input React onKeyDown, src/App.tsx:493; reload except memos |
| canvas.panHold | Space + left pointer drag | FlowCanvas window capture keydown/keyup :110-122, pointerdown/move :151-159; only pointer-over/target-inside canvas, excludes input/inspector/composition |
| canvas.zoomWheel | Alt + wheel | FlowCanvas viewport non-passive wheel listener :104-109/:120; excludes inputs; cursor anchor, clamp 2%-200% |
| quickAdd.submit | Enter without Shift | QuickAdd input onKeyDown, src/components/QuickAdd.tsx:207-214; FloatingToday quick window input :931-941 |
| quickAdd.cancel | Escape | Same two handlers; optional onCancel in QuickAdd, hides quick window in FloatingToday |
| taskEditor.cancel | Escape | TaskEditor window keydown effect, src/components/TaskEditor.tsx:177-184 |
| scopeDialog.cancel | Escape | ScopeDialog window keydown effect, src/components/ScopeDialog.tsx:78-82 |
| cloudHistory.close | Escape | CloudHistory document keydown :19-30; blocked while pending |
| floating.renameStart | F2 OR Enter | FloatingToday title span onKeyDown :682-687 |
| floating.renameSave | Enter | FloatingToday edit input onKeyDown :660-667 |
| floating.renameCancel | Escape | Same input handler :664 |
| organize.create | Enter | OrganizeView create input :515-516 |
| organize.renameSave | Enter | OrganizeView rename inputs :585-587, :684-686, :753-755 (project/category/tag branches) |
| organize.renameCancel | Escape | Same branches |
| subtask.renameSave | Enter | SubtaskList edit input :159-161 |
| subtask.renameCancel | Escape | Same edit input |
| subtask.add | Enter | SubtaskList add input :200-201 |
| stats.createGoal | Enter | StatsView input :484-485 |
| assistant.send | Ctrl+Enter | AiAssistant textarea :171-172; Meta not currently supported |

Canvas buttons zoom in/out/fit, next result, add/delete/reorder step, inspector open/close have no keyboard accelerators today. Native focusable buttons still activate via standard Enter/Space. Sidebar/header also have no explicit app accelerators. Retain them; shortcut settings do not require structural UI removal.

## Editing/accessibility keys are separate

CloudHistory handles Tab/Shift+Tab solely for modal focus trapping (:21-27); retain as accessibility behavior. fresh-paste.ts:76-98 intercepts Ctrl/Meta+V (including Shift), excludes Alt/composition/content-editor fields. ContentEditor.tsx:143-145 intercepts same paste family and forwards consumer onKeyDown first. These implement native clipboard freshness/editor paste, not app navigation commands; leave copy/cut/paste/undo/redo/select-all, text navigation, Tab focus traversal, IME composition and standard button activation as OS/editor conventions. No bespoke copy/cut/undo/redo keyboard registration found in src or Rust.

## Existing persistence and validation

- WindowConfig has container serde(default), camelCase at window_mgr.rs:71. New fields with defaults can preserve old JSON without schema migration. Four strings plus shortcutEnabled are in TS window-ipc.ts:35-40.
- windowGetConfig/windowSetConfig IPC exports are window-ipc.ts:93/:103; commands window_get_config/window_set_config. load_config selects settings key window_config (:172-188); save_config upserts only that key (:193-205), leaving AI/cloud credentials and other rows alone.
- SettingsView window tab embeds WindowSettings (:449). WindowSettings listens window-config-changed and shortcut-error (:62-63); apply merges patch into current config (:71-81). Its four global selects (:533-560) only expose current + SHORTCUT_PRESETS (window-ipc.ts:163); currently no arbitrary capture or local/canvas preferences.
- Rust reload is installed at startup lib.rs:212; plugin builder with_handler at :121-123. window_set_config reloads when global fields change (:468-478), then persists and emits :481-485.
- Global accelerator parsing uses Shortcut::from_str (shortcuts.rs:25). Empty strings skip. Duplicate detection compares raw strings (:67), so equivalent aliases/order/case are not canonicalized. Per-key errors aggregate; reload first unregisters all and permits partial registrations.
- Critical limitation: window_set_config catches reload errors and STILL saves/returns success (:474-481). UI receives error event, but configuration does not reflect effective registration. has_recovery_path (:167) checks shortcutEnabled boolean, not usable registered toggle. Invalid/out-of-process conflict can therefore leave recovery promise untrue. Fix validation/rollback before treating new UI as reliable.
- load_config currently falls back to all defaults on malformed JSON (:180). Additive fields must use defaults rather than invalidating existing payload; do not rewrite credentials/data or ask users to clear config.

## Minimal unified contract recommendation

Keep existing global fields unchanged for compatibility. Add a defaulted nested localShortcuts map/struct (stable IDs, canonical accelerator arrays to preserve F2/Enter aliases) and canvasInput { panHold, wheelZoom: 'direct'|'alt'|'ctrl'|'shift'|'off' }; direct wheel zoom should be selectable and default per request. Wheel policy is a gesture preference rather than pretending wheel is accepted by OS global accelerator parser. Pan should be key hold plus left drag, with explicit empty/disabled policy if desired.

Expose one Settings shortcut section grouping global, app, context and canvas commands, displaying scope and editable bindings; exact settings location can remain in existing layout. Route existing handlers through one small TS matcher with exact modifiers, composition/defaultPrevented/repeat policy, scope precedence and editable-target exceptions. Preserve contextual Enter/Escape actions by matching in their existing component scope, not registering dozens of window listeners. Use Rust authoritative validation for globals before registration; prevalidate complete set, canonical duplicate identity, rollback old registrations on failure, do not persist failed changes. Broadcast local/canvas updates through existing window-config-changed, ensure mounted canvas listeners read latest config rather than stale empty-dependency closure.

No DB schema change is needed: extend existing settings JSON only; merge loaded config and preserve all existing window fields. If a separate shortcut_config key is chosen, it fits existing key/value table but creates another API unnecessarily.

## Conflicts and precision hazards

- App Ctrl/Meta+n/f/r handler lacks editable-target, defaultPrevented, repeat and composition guards and checks only ctrl||meta. Thus Ctrl+Alt+N can also match local new task, Ctrl+Alt+R can refresh, and Ctrl+Shift+lowercase variants can match depending emitted key. Global/local scope collisions must be validated or have deterministic priority.
- Multiple modal window/document Escape handlers can act on the same event (TaskEditor and ScopeDialog do not stop propagation); local scoped matcher should avoid closing parent and child together.
- Search Enter explicitly reloads even though setSearch already debounces/requeries; existing repo caveat describes redundant refresh. Record if left unchanged.
- Space hold currently accepts modifiers too, including Alt+Space. Windows reserves Alt+Space for native menu; canvas_input.rs installs a main-window-only subclass at :13-27, command :31-45 sets active scope, and :88-94 suppresses SC_KEYMENU with character 32 only while active. Preserve scoped protection if modifier+Space still allowed, and do not suppress native menu outside canvas.
- Alt/F10 menu activation, Alt+F4 close, OS combinations (Win+L etc.), IME switches, Ctrl+C/V/X/A/Z/Y and standard text keys should not be silently seized by global/app settings. Reserved conflicts may be blocked or explained; no runtime OS registration check was performed here.
- Canvas Alt+wheel can coexist with Alt+Space hold and trigger native menu; direct-wheel default reduces required modifier mixing. Retain input exclusion so editor/inspector wheel scrolls normally.

## Verification limits

Inventory was verified by code searches for keydown/keyup/onKeyDown/key/code/accelerator/global_shortcut/wheel in src and src-tauri/src, followed by reading handler/persistence ranges. No tests executed, no claim of runtime completeness across WebView/OS defaults, no production config or credentials read. This report is outside repository as requested; controller owns required work-log/commits/CI later.
