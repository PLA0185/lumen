//! Lumen 应用入口与 Tauri 运行时装配。
//!
//! 分层约定（任务书 §2.4「功能分模块开发」）：
//! - `db`              —— 连接池、迁移、事务、备份原语
//! - `error`           —— 统一错误码与前端可读消息
//! - `models`          —— 数据模型与 IPC 输入/输出类型
//! - `commands`        —— 任务 CRUD、今日概览等基础用例
//! - `organize`        —— 项目 / 分类 / 标签
//! - `subtasks`        —— 子任务与依赖
//! - `reminders`       —— 提醒调度与去重
//! - `recurrence`      —— 重复规则引擎（纯逻辑，可独立测试）
//! - `recurrence_service` —— 系列 / 实例 / 例外 / 分段的持久化与范围语义
//! - `attachments`     —— 附件受控存储
//! - `backup`          —— 备份、导出、恢复
//! - `window_mgr`      —— 窗口能力与安全恢复（§8）
//! - `shortcuts`       —— 全局快捷键
//! - `pdf`             —— PDF 导出（走 WebView2 自身的 PrintToPdf，保证中文可读）
//! - `update`          —— 自动更新检查（§9 分发与升级）
//! - `lib.rs`          —— 只做装配：插件注册、状态注入、托盘、启动恢复
//!
//! 时区策略（§4.3 / §5）：Rust 侧一律处理 UTC；"今天/本周"等本地日历
//! 归属由前端按用户本地时区计算后把 UTC 边界传入。这样避免了在数据层
//! 硬编码某一时区。

pub mod ai;
pub mod ai_features;
pub mod attachments;
pub mod backup;
pub mod commands;
#[cfg(test)]
mod commands_e2e;
pub mod db;
pub mod error;
pub mod focus;
pub mod models;
pub mod organize;
pub mod pdf;
pub mod recurrence;
#[cfg(test)]
mod recurrence_e2e;
pub mod recurrence_service;
#[cfg(test)]
mod remediation2_e2e;
pub mod reminders;
pub mod shortcuts;
pub mod stats;
pub mod subtasks;
pub mod window_mgr;

use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_log::{Target, TargetKind};

use commands::AppState;
use db::Db;
use window_mgr::WindowConfig;

/// 主窗口标签，全应用统一引用
pub const MAIN_WINDOW: &str = window_mgr::MAIN;

/// 托盘图标 ID
const TRAY_ID: &str = "main-tray";

/// 装配并启动应用。
pub fn run() {
    let mut builder = tauri::Builder::default();

    // ---------------------------------------------------------------------
    // 单实例插件必须最先注册（§10 明确要求单实例或明确多实例的互斥策略）。
    // 第二次启动时激活已有窗口，避免两个进程同时写同一个 SQLite 文件。
    // ---------------------------------------------------------------------
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }));
    }

    builder
        // ---------------- 日志（§9：一键打开日志目录、日志不含密钥） ----------------
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir {
                        file_name: Some("lumen".into()),
                    }),
                ])
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .max_file_size(5 * 1024 * 1024)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // 系统通知（§4.3）
        .plugin(tauri_plugin_notification::init())
        // 全局快捷键（§4.4 / §8.2 恢复入口）。
        // 注意 with_handler 是必需的：没有 handler 时 on_shortcut 注册不生效。
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, _shortcut, _event| {
                    // 实际动作在各自的 on_shortcut 闭包里处理，
                    // 这里保持空实现即可（插件要求提供 handler）。
                })
                .build(),
        )
        // 开机自启（§8.6，可选开关）
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 窗口位置尺寸记忆（§8 多显示器断连找回）
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        // 自动更新（§9「后续版本升级不需要用户手动重装」）。
        // 更新包必须用 minisign 私钥签名，公钥在 tauri.conf.json 的
        // plugins.updater.pubkey 中；签名无法关闭（插件硬性要求）。
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 更新安装完成后需要重启应用
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // ---------------- 数据目录与数据库 ----------------
            // 刻意使用 app_data_dir（Windows 下 %APPDATA%\com.pla0185.lumen），
            // 而不是安装目录：这样 NSIS 卸载程序的"删除应用数据"选项才能
            // 统一决定是否清除用户数据（§10 卸载后的数据保留/删除选项）。
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法获取应用数据目录：{e}"))?;

            log::info!("Lumen 启动，数据目录：{}", data_dir.display());

            let db = tauri::async_runtime::block_on(Db::init(&data_dir))
                .map_err(|e| format!("数据库初始化失败：{e}"))?;

            log::info!("数据库就绪：{}", db.db_path().display());

            app.manage(AppState::new(db));

            let handle = app.handle().clone();

            // ---------------- 启动流程（顺序有意义） ----------------
            tauri::async_runtime::block_on(async move {
                let state = match handle.try_state::<AppState>() {
                    Some(s) => s,
                    None => return,
                };

                // 1) 读窗口配置，并执行**启动时的安全恢复**（§8 安全底线）
                let mut cfg = match window_mgr::load_config(&state).await {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("读取窗口配置失败，使用默认值：{e}");
                        WindowConfig::default()
                    }
                };
                let changed = window_mgr::safe_recovery(&handle, &mut cfg);
                if changed {
                    if let Err(e) = window_mgr::save_config(&state, &cfg).await {
                        log::warn!("保存安全恢复后的窗口配置失败：{e}");
                    }
                }
                // 写入内存缓存：托盘菜单的同步处理器只读内存，不查库
                state.set_cfg(&cfg);

                // 2) 注册全局快捷键（失败要让用户知道，否则恢复路径是假的）
                if let Err(e) = shortcuts::reload(&handle, &cfg) {
                    log::error!("全局快捷键注册失败：{e}");
                    let _ = handle.emit("shortcut-error", e.to_string());
                }

                // 3) 按配置应用主窗口能力
                if let Some(w) = handle.get_webview_window(MAIN_WINDOW) {
                    let _ = w.set_always_on_top(cfg.main_always_on_top);
                    let _ = w.set_skip_taskbar(!cfg.main_show_in_taskbar);
                }

                // 4) 恢复悬浮窗（若用户上次开着）
                if cfg.floating_enabled {
                    if let Err(e) = window_mgr::ensure_floating(&handle, &cfg) {
                        log::error!("恢复悬浮窗失败：{e}");
                    }
                }

                // 5) 托盘
                if cfg.tray_enabled {
                    if let Err(e) = setup_tray(&handle) {
                        log::error!("托盘初始化失败（应用继续运行）：{e}");
                    }
                } else {
                    log::info!("按用户设置未启用托盘图标");
                }

                // 6) 提醒调度（§4.3）
                reminders::spawn(handle.clone());

                // 7) 统计今日未完成数并刷新菜单文字
                refresh_today_count(&handle);
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    if window.label() != MAIN_WINDOW {
                        return;
                    }
                    // §8.5：关闭主窗口是退出还是缩到托盘，由设置决定。
                    let app = window.app_handle();
                    let cfg = app
                        .try_state::<AppState>()
                        .map(|s| {
                            tauri::async_runtime::block_on(window_mgr::load_config(&s))
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();

                    if cfg.close_action == "quit" {
                        log::info!("按设置：关闭主窗口即退出程序");
                        app.exit(0);
                        return;
                    }

                    // 缩到托盘。但必须先确认真的还有恢复路径，
                    // 否则用户会以为自己关掉了应用，实际却再也打不开（§8.5）。
                    let has_tray = app.tray_by_id(TRAY_ID).is_some() || cfg.tray_enabled;
                    if has_tray || cfg.shortcut_enabled {
                        api.prevent_close();
                        let _ = window.hide();
                        log::info!(
                            "主窗口已隐藏到托盘（恢复方式：{}）",
                            if has_tray {
                                "托盘菜单"
                            } else {
                                "全局快捷键"
                            }
                        );
                    } else {
                        // 没有恢复路径时**允许真正关闭**，而不是把用户困住
                        log::warn!("未启用托盘与全局快捷键，关闭主窗口将直接退出程序");
                        app.exit(0);
                    }
                }
                // 记住悬浮窗位置（§8 多显示器断连后找回）。
                // 用 match guard 而不是嵌套 if：正是 clippy 提示的那种可折叠写法。
                WindowEvent::Moved(_) if window.label() == window_mgr::FLOATING => {
                    window_mgr::remember_floating_position(window.app_handle());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::app_info,
            commands::app_data_paths,
            commands::task_create,
            commands::task_update,
            commands::task_get,
            commands::task_list,
            commands::task_count,
            commands::task_toggle_done,
            commands::task_soft_delete,
            commands::task_restore,
            commands::task_purge,
            commands::task_purge_all_deleted,
            commands::task_bulk,
            commands::task_duplicate,
            commands::task_reorder,
            commands::today_overview,
            commands::set_reminders_paused,
            commands::tasks_in_range,
            commands::task_reschedule,
            // ---- 项目 / 分类 / 标签（§4.2）----
            organize::project_list,
            organize::project_create,
            organize::project_update,
            organize::project_set_archived,
            organize::project_delete_impact,
            organize::project_delete,
            organize::project_merge,
            organize::category_list,
            organize::category_create,
            organize::category_update,
            organize::category_delete_impact,
            organize::category_delete,
            organize::category_merge,
            organize::tag_list,
            organize::tag_create,
            organize::tag_update,
            organize::tag_delete,
            organize::tag_merge,
            organize::task_tags_get,
            organize::task_tags_set,
            // ---- 子任务与依赖（§4.1）----
            subtasks::subtask_create,
            subtasks::subtask_list,
            subtasks::subtask_update,
            subtasks::subtask_delete,
            subtasks::subtask_progress,
            subtasks::subtask_progress_batch,
            subtasks::dependency_add,
            subtasks::dependency_list,
            subtasks::dependency_dependents,
            subtasks::dependency_remove,
            subtasks::dependency_is_blocked,
            // ---- 提醒（§4.3）----
            reminders::reminder_create,
            reminders::reminder_list,
            reminders::reminder_set_enabled,
            reminders::reminder_delete,
            reminders::reminder_snooze,
            reminders::reminder_scheduler_status,
            reminders::reminder_set_grace,
            reminders::reminder_check_missed,
            reminders::reminder_list_pending,
            // ---- 备份 / 导出 / 恢复（§9）----
            backup::backup_export,
            backup::backup_preview,
            backup::backup_restore,
            backup::backup_list,
            backup::backup_delete,
            backup::backup_auto,
            backup::export_csv,
            backup::export_markdown,
            // ---- PDF 导出（§6 中文可读）----
            pdf::export_pdf,
            commands::task_report,
            commands::task_report_all,
            // ---- 附件受控存储（§4.1）----
            attachments::attachment_add,
            attachments::attachment_list,
            attachments::attachment_remove,
            attachments::attachment_reveal,
            attachments::attachment_check,
            attachments::attachment_cleanup_orphans,
            // ---- 重复系列（§5）----
            recurrence_service::recurring_create,
            recurrence_service::recurring_preview,
            recurrence_service::recurring_materialize,
            recurrence_service::recurring_get,
            recurrence_service::recurring_occurrences,
            recurrence_service::recurring_stats,
            recurrence_service::recurring_edit_instance,
            recurrence_service::recurring_skip_occurrence,
            recurrence_service::recurring_delete,
            recurrence_service::recurring_scope_info,
            // ---- 窗口能力（§8）----
            window_mgr::window_get_config,
            window_mgr::window_set_config,
            window_mgr::window_apply_action,
            window_mgr::window_reset_safe,
            window_mgr::window_floating_state,
            window_mgr::window_floating_reset_position,
            window_mgr::window_set_floating_opacity,
            window_mgr::window_set_floating_size,
            window_mgr::app_quit,
            // ---- AI 提供商适配（§6）----
            ai::ai_provider_defaults,
            ai::ai_provider_key_status,
            ai::ai_get_config,
            ai::ai_set_config,
            ai::ai_test_connection,
            ai::ai_list_models,
            ai::ai_clear_key,
            ai::ai_status,
            // ---- AI 功能（§6：预览确认后才写库）----
            ai_features::ai_organize,
            ai_features::ai_breakdown,
            ai_features::ai_plan,
            ai_features::ai_review,
            ai_features::ai_apply,
            ai_features::ai_discard,
            ai_features::schedule_conflicts,
            // ---- 统计与成长（§7）----
            stats::stats_period,
            stats::stats_growth,
            stats::growth_get_config,
            stats::growth_set_config,
            stats::goals_list,
            stats::goal_create,
            stats::goal_delete,
            // ---- 专注模式（§4.4）----
            focus::focus_start,
            focus::focus_current,
            focus::focus_pause,
            focus::focus_resume,
            focus::focus_end,
            focus::focus_cancel,
            focus::focus_summary,
            focus::focus_format_seconds,
        ])
        .run(tauri::generate_context!())
        .expect("Lumen 启动失败");
}

// =============================================================================
// 托盘（§8.6）
// =============================================================================

/// 菜单项 ID 常量。集中定义避免字符串拼写错误导致事件匹配不上。
mod tray_ids {
    /// 打开/隐藏主窗口
    pub const TOGGLE: &str = "tray-toggle";
    /// 快速添加
    pub const QUICK_ADD: &str = "tray-quick-add";
    /// 今日概览
    pub const TODAY: &str = "tray-today";
    /// 显示/隐藏悬浮窗
    pub const FLOATING: &str = "tray-floating";
    /// 切换主窗口置顶
    pub const MAIN_TOP: &str = "tray-main-top";
    /// 切换悬浮窗置顶
    pub const FLOAT_TOP: &str = "tray-float-top";
    /// 切换悬浮窗穿透
    pub const CLICK_THROUGH: &str = "tray-click-through";
    /// 暂停 / 恢复提醒
    pub const PAUSE_REMINDERS: &str = "tray-pause-reminders";
    /// 窗口安全重置
    pub const RESET_SAFE: &str = "tray-reset-safe";
    /// 设置
    pub const SETTINGS: &str = "tray-settings";
    /// 完全退出
    pub const QUIT: &str = "tray-quit";
}

/// 设置托盘在图标的显隐（§8.6）。返回是否处于启用状态。
pub fn set_tray_visible(app: &AppHandle, visible: bool) {
    if visible {
        if app.tray_by_id(TRAY_ID).is_none() {
            if let Err(e) = setup_tray(app) {
                log::error!("创建托盘图标失败：{e}");
            }
        } else if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_visible(true);
        }
    } else if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
    log::info!("托盘图标已{}", if visible { "显示" } else { "隐藏" });
}

/// 构建托盘菜单与事件处理。
///
/// §8.6 要求菜单至少包含：打开/隐藏、快速添加、今日概览、置顶、穿透、
/// 暂停提醒、设置、完全退出。这里逐项落实，并额外提供「窗口安全重置」
/// 作为兜底手段——它对应 §8 那条"不得让用户必须删配置文件才能找回窗口"。
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let cfg = current_config(app);
    let menu = build_tray_menu(app, &cfg)?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Lumen —— 智能任务管理")
        // 左键单击切换主窗口显隐，符合 Windows 托盘交互习惯
        .show_menu_on_left_click(false)
        // 直接传函数指针，避免多余的闭包包装
        .on_menu_event(handle_tray_menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window_mgr::toggle_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    log::info!("系统托盘已就绪");
    Ok(())
}

/// 同步读取当前配置。
///
/// **绝不能在这里查数据库**：本函数被托盘菜单事件处理器调用，那是同步上下文；
/// 若在其内部 `block_on`，而外层已处于 Tauri 的异步运行时中（setup 阶段），
/// 就会造成 tokio 运行时嵌套并直接 panic（实测踩到过）。
/// 因此配置以 AppState 的内存副本为准，数据库只负责持久化。
fn current_config(app: &AppHandle) -> WindowConfig {
    app.try_state::<AppState>()
        .map(|s| s.cfg())
        .unwrap_or_default()
}

/// 根据当前状态构建托盘菜单（勾选项反映真实状态）
fn build_tray_menu(app: &AppHandle, cfg: &WindowConfig) -> tauri::Result<Menu<tauri::Wry>> {
    let paused = app
        .try_state::<AppState>()
        .map(|s| {
            s.reminders_paused
                .load(std::sync::atomic::Ordering::Relaxed)
        })
        .unwrap_or(false);

    // 今日未完成数：从内存缓存读取。
    // 该值由 `refresh_today_count` 异步更新，菜单构建本身不做任何 I/O——
    // 在同步上下文里查库会导致 tokio 运行时嵌套崩溃。
    let cached = app
        .try_state::<AppState>()
        .map(|s| {
            s.today_open_count
                .load(std::sync::atomic::Ordering::Relaxed)
        })
        .unwrap_or(-1);

    let today_text = match cached {
        -1 => "今日概览".to_string(),
        0 => "今日：全部完成 ✓".to_string(),
        n => format!("今日未完成：{n} 项"),
    };

    let toggle = MenuItem::with_id(
        app,
        tray_ids::TOGGLE,
        "打开 / 隐藏主窗口",
        true,
        None::<&str>,
    )?;
    let quick = MenuItem::with_id(
        app,
        tray_ids::QUICK_ADD,
        "快速添加任务…",
        true,
        None::<&str>,
    )?;
    let today = MenuItem::with_id(app, tray_ids::TODAY, today_text, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    let floating_label = if cfg.floating_enabled {
        "隐藏今日悬浮窗"
    } else {
        "显示今日悬浮窗"
    };
    let floating = MenuItem::with_id(app, tray_ids::FLOATING, floating_label, true, None::<&str>)?;

    // 勾选项用 CheckMenuItem 表达当前状态，用户一眼能看出开关位置。
    // 注意：普通 MenuItem 没有 set_checked，必须用 CheckMenuItem。
    let main_top = CheckMenuItem::with_id(
        app,
        tray_ids::MAIN_TOP,
        "主窗口始终置顶",
        true,
        cfg.main_always_on_top,
        None::<&str>,
    )?;

    let float_top = CheckMenuItem::with_id(
        app,
        tray_ids::FLOAT_TOP,
        "悬浮窗始终置顶",
        true,
        cfg.floating_always_on_top,
        None::<&str>,
    )?;

    let through = CheckMenuItem::with_id(
        app,
        tray_ids::CLICK_THROUGH,
        "悬浮窗鼠标穿透（开启后无法点击悬浮窗）",
        true,
        cfg.floating_click_through,
        None::<&str>,
    )?;

    let sep2 = PredefinedMenuItem::separator(app)?;

    let pause = CheckMenuItem::with_id(
        app,
        tray_ids::PAUSE_REMINDERS,
        if paused {
            "恢复提醒"
        } else {
            "暂停提醒"
        },
        true,
        paused,
        None::<&str>,
    )?;

    let sep3 = PredefinedMenuItem::separator(app)?;

    let settings = MenuItem::with_id(app, tray_ids::SETTINGS, "设置…", true, None::<&str>)?;
    let reset = MenuItem::with_id(
        app,
        tray_ids::RESET_SAFE,
        "窗口找不到了？重置窗口设置",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, tray_ids::QUIT, "完全退出 Lumen", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &toggle, &quick, &today, &sep1, &floating, &main_top, &float_top, &through, &sep2,
            &pause, &sep3, &settings, &reset, &quit,
        ],
    )
}

/// 本地"今天"的 UTC 范围（与前端 `todayRange()` 口径一致）
fn local_today_range() -> (String, String) {
    use chrono::TimeZone;
    let now = chrono::Local::now();
    let start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| chrono::Local.from_local_datetime(&d).single())
        .unwrap_or(now);
    let end = start + chrono::Duration::days(1);
    let fmt =
        |d: chrono::DateTime<chrono::Local>| crate::db::to_db_time(d.with_timezone(&chrono::Utc));
    (fmt(start), fmt(end))
}

/// 异步刷新"今日未完成数"缓存，并顺带刷新托盘菜单。
///
/// 之所以要缓存而不是让菜单现查：菜单构建在同步上下文里，查库需要 async，
/// 二者混用会导致 tokio 运行时嵌套（实测崩溃）。因此把 I/O 放到这里。
pub fn refresh_today_count(app: &AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app2.try_state::<AppState>() else {
            return;
        };
        let (start, end) = local_today_range();
        let row = sqlx::query(
            "SELECT COUNT(*) AS n FROM tasks
             WHERE deleted_at IS NULL
               AND status NOT IN ('done', 'archived')
               AND planned_at IS NOT NULL
               AND planned_at >= ?1 AND planned_at < ?2",
        )
        .bind(&start)
        .bind(&end)
        .fetch_one(state.db.pool())
        .await;

        use sqlx::Row;
        match row {
            Ok(r) => match r.try_get::<i64, _>("n") {
                Ok(n) => state
                    .today_open_count
                    .store(n, std::sync::atomic::Ordering::Relaxed),
                Err(e) => log::debug!("读取今日未完成数失败：{e}"),
            },
            Err(e) => log::debug!("统计今日未完成数失败：{e}"),
        }

        refresh_tray_menu(&app2);
    });
}

/// 处理托盘菜单事件。
fn handle_tray_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        tray_ids::TOGGLE => window_mgr::toggle_main(app),

        tray_ids::QUICK_ADD => window_mgr::toggle_quick_add(app),

        tray_ids::TODAY => {
            if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
                let _ = win.emit("navigate", "today");
            }
            refresh_tray_menu(app);
        }

        tray_ids::FLOATING => {
            let cfg = current_config(app);
            let action = if cfg.floating_enabled {
                "hide_floating"
            } else {
                "show_floating"
            };
            apply_action_from_tray(app, action);
        }

        tray_ids::MAIN_TOP => apply_action_from_tray(app, "toggle_main_top"),
        tray_ids::FLOAT_TOP => apply_action_from_tray(app, "toggle_floating_top"),

        tray_ids::CLICK_THROUGH => {
            // 开启穿透前的安全检查在 window_mgr 里；这里把失败原因告诉用户
            apply_action_from_tray(app, "toggle_floating_click_through");
        }

        tray_ids::PAUSE_REMINDERS => {
            let now = tauri::async_runtime::block_on(window_mgr::toggle_reminders_paused(app));
            log::info!(
                "提醒已{}（来自托盘菜单）",
                if now { "暂停" } else { "恢复" }
            );
            refresh_tray_menu(app);
        }

        tray_ids::SETTINGS => {
            if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
                let _ = win.show();
                let _ = win.set_focus();
                let _ = win.emit("navigate", "settings");
            }
        }

        tray_ids::RESET_SAFE => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app2.state::<AppState>();
                match window_mgr::window_reset_safe(app2.clone(), state).await {
                    Ok(_) => {
                        log::info!("窗口设置已重置为安全默认值");
                        let _ = app2.emit("window-reset-done", ());
                        refresh_tray_menu(&app2);
                    }
                    Err(e) => log::error!("窗口重置失败：{e}"),
                }
            });
        }

        tray_ids::QUIT => {
            log::info!("用户从托盘退出应用");
            app.exit(0);
        }

        other => log::warn!("收到未知托盘菜单事件：{other}"),
    }
}

/// 通过托盘触发一个窗口动作，并把结果（含失败原因）反馈给用户
fn apply_action_from_tray(app: &AppHandle, action: &str) {
    let app2 = app.clone();
    let action = action.to_string();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        match window_mgr::window_apply_action(app2.clone(), state, action).await {
            Ok(_) => refresh_tray_menu(&app2),
            Err(e) => {
                // 失败必须让用户看见，否则他只会觉得"点了没用"
                log::warn!("托盘操作失败：{e}");
                let _ = app2.emit("action-error", e.to_string());
                if let Some(w) = app2.get_webview_window(MAIN_WINDOW) {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.emit("action-error", e.to_string());
                }
            }
        }
    });
}

/// 重建托盘菜单，让勾选状态与最新配置一致。
///
/// 菜单在 Windows 上是静态资源，状态变化后必须重建才能反映出来。
pub fn refresh_tray_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let cfg = current_config(app);
    match build_tray_menu(app, &cfg) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                log::warn!("刷新托盘菜单失败：{e}");
            }
        }
        Err(e) => log::warn!("构建托盘菜单失败：{e}"),
    }
}
