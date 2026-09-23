//! AiTodo 应用入口与 Tauri 运行时装配。
//!
//! 分层约定（任务书 §2.4「功能分模块开发」）：
//! - `db`       —— 连接池、迁移、事务、备份原语
//! - `error`    —— 统一错误码与前端可读消息
//! - `models`   —— 数据模型与 IPC 输入/输出类型
//! - `commands` —— 业务用例（任务 CRUD、今日概览等）
//! - `lib.rs`   —— 只做装配：插件注册、状态注入、托盘、窗口事件
//!
//! 时区策略（§4.3 / §5）：Rust 侧一律处理 UTC；"今天/本周"等本地日历
//! 归属由前端按用户本地时区计算后，把 UTC 边界传入（见 `today_overview`）。
//! 这样避免了在数据层硬编码某一时区。

pub mod backup;
pub mod commands;
pub mod db;
pub mod error;
pub mod models;
pub mod organize;
pub mod reminders;
pub mod subtasks;

use std::sync::atomic::Ordering;

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
// Emitter 提供 `emit()`，必须显式引入才可用（trait 方法不会自动可见）
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_log::{Target, TargetKind};

use commands::AppState;
use db::Db;

/// 主窗口标签，全应用统一引用，避免各处硬编码字符串不一致。
pub const MAIN_WINDOW: &str = "main";

/// 装配并启动应用。
pub fn run() {
    let mut builder = tauri::Builder::default();

    // ---------------------------------------------------------------------
    // 单实例插件必须最先注册：任务书 §10 要求"单实例或明确多实例的
    // 数据库互斥策略"，这里选择单实例——第二次启动时激活已有窗口，
    // 避免两个进程同时写同一个 SQLite 文件。
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
                    Target::new(TargetKind::LogDir { file_name: Some("aitodo".into()) }),
                ])
                // release 下只记 info 及以上，避免日志膨胀；调试期开 debug
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
        // 系统通知（§4.3）；应用未启动时的通知由前端调度
        .plugin(tauri_plugin_notification::init())
        // 全局快捷键（§4.4 / §8.2：穿透后必须保留可用的恢复入口）
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // 开机自启（§8.6，可选开关）
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 窗口位置尺寸记忆，崩溃/重启后不会跑到屏幕外（§8 多显示器断连找回）
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            // ---------------- 数据目录与数据库 ----------------
            // 刻意使用 app_data_dir（Windows 下 %APPDATA%\com.pla0185.aitodo），
            // 而不是安装目录：这样 NSIS 卸载程序的"删除应用数据"选项才能
            // 统一决定是否清除用户数据（§10 卸载后的数据保留/删除选项）。
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法获取应用数据目录：{e}"))?;

            log::info!("AiTodo 启动，数据目录：{}", data_dir.display());

            let db = tauri::async_runtime::block_on(Db::init(&data_dir))
                .map_err(|e| format!("数据库初始化失败：{e}"))?;

            log::info!("数据库就绪：{}", db.db_path().display());

            app.manage(AppState::new(db));

            // ---------------- 提醒调度（§4.3） ----------------
            // 官方通知插件的 schedule 在桌面端会被忽略（已核实其源码注释），
            // 因此调度由我们自己的轮询循环负责，数据库是唯一事实来源。
            reminders::spawn(app.handle().clone());

            // ---------------- 系统托盘（§8.6） ----------------
            if let Err(e) = setup_tray(app.handle()) {
                // 托盘失败不应阻止应用启动：用户仍可通过主窗口使用
                log::error!("托盘初始化失败（应用继续运行）：{e}");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 任务书 §8.5：「关闭主窗口是退出还是缩到托盘由设置决定」。
                // 阶段 2 接入设置项后改为读配置；当前默认"关闭即隐藏到托盘"，
                // 但只有在托盘确实可用时才阻止关闭，否则会导致窗口无法找回。
                if window.label() == MAIN_WINDOW && window.app_handle().tray_by_id("main-tray").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                    log::info!("主窗口已隐藏到托盘（可从托盘菜单重新打开）");
                }
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
            commands::task_toggle_done,
            commands::task_soft_delete,
            commands::task_restore,
            commands::task_purge,
            commands::task_purge_all_deleted,
            commands::task_bulk,
            commands::today_overview,
            commands::set_reminders_paused,
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
        ])
        .run(tauri::generate_context!())
        .expect("AiTodo 启动失败");
}

/// 系统托盘图标 ID
const TRAY_ID: &str = "main-tray";

/// 构建托盘菜单与事件处理。
///
/// §8.6 要求托盘菜单至少包含：打开/隐藏、快速添加、今日概览、置顶、穿透、
/// 暂停提醒、设置、完全退出。阶段 1 先落地"打开/隐藏、今日概览、暂停提醒、
/// 完全退出"四项并打通事件链路，其余项在阶段 4（窗口能力）接入——
/// 未实现的功能不放入菜单，避免出现点不动的假按钮（§3）。
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app, None)?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AiTodo —— 智能任务管理")
        // 左键单击切换主窗口显隐，符合 Windows 托盘交互习惯
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_tray_menu(app, event))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    log::info!("系统托盘已就绪");
    Ok(())
}

/// 菜单项 ID 常量。集中定义避免字符串拼写错误导致事件匹配不上。
mod tray_ids {
    /// 打开/隐藏主窗口
    pub const TOGGLE: &str = "tray-toggle";
    /// 今日概览（只读展示，暂以通知形式呈现）
    pub const TODAY: &str = "tray-today";
    /// 暂停 / 恢复提醒
    pub const PAUSE_REMINDERS: &str = "tray-pause-reminders";
    /// 完全退出
    pub const QUIT: &str = "tray-quit";
}

/// 根据当前未完成数构建托盘菜单。
///
/// `open_count` 为 None 时表示尚未统计，菜单显示"今日概览"而不显示数字。
fn build_tray_menu(app: &AppHandle, open_count: Option<i64>) -> tauri::Result<Menu<tauri::Wry>> {
    let paused = app
        .try_state::<AppState>()
        .map(|s| s.reminders_paused.load(Ordering::Relaxed))
        .unwrap_or(false);

    let today_text = match open_count {
        Some(0) => "今日：全部完成 ✓".to_string(),
        Some(n) => format!("今日未完成：{n} 项"),
        None => "今日概览".to_string(),
    };

    let toggle = MenuItem::with_id(app, tray_ids::TOGGLE, "打开 / 隐藏主窗口", true, None::<&str>)?;
    let today = MenuItem::with_id(app, tray_ids::TODAY, today_text, false, None::<&str>)?;
    let pause = MenuItem::with_id(
        app,
        tray_ids::PAUSE_REMINDERS,
        if paused { "恢复提醒" } else { "暂停提醒" },
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, tray_ids::QUIT, "完全退出 AiTodo", true, None::<&str>)?;

    Menu::with_items(app, &[&toggle, &today, &pause, &quit])
}

/// 处理托盘菜单事件。
fn handle_tray_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        tray_ids::TOGGLE => toggle_main_window(app),

        tray_ids::TODAY => {
            if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
                let _ = win.show();
                let _ = win.set_focus();
                // 前端监听该事件切换到"今天"视图
                let _ = win.emit("navigate", "today");
            }
        }

        tray_ids::PAUSE_REMINDERS => {
            if let Some(state) = app.try_state::<AppState>() {
                let now = !state.reminders_paused.load(Ordering::Relaxed);
                state.reminders_paused.store(now, Ordering::Relaxed);
                log::info!("提醒已{}", if now { "暂停" } else { "恢复" });
                // 重建菜单以反映最新状态
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    if let Ok(menu) = build_tray_menu(app, None) {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
                let _ = app.emit("reminders-paused-changed", now);
            }
        }

        tray_ids::QUIT => {
            log::info!("用户从托盘退出应用");
            app.exit(0);
        }

        other => {
            log::warn!("收到未知托盘菜单事件：{other}");
        }
    }
}

/// 切换主窗口显隐。
///
/// §8 要求"不得因为穿透、透明度或隐藏任务栏，使用户必须删除配置文件
/// 才能找回窗口"——因此这里在显示窗口时同时恢复置顶与焦点。
fn toggle_main_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window(MAIN_WINDOW) else {
        log::error!("主窗口不存在，无法切换显隐");
        return;
    };
    match win.is_visible() {
        Ok(true) => {
            let _ = win.hide();
        }
        _ => {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
        }
    }
}
