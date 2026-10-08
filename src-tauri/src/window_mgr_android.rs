//! Android 不提供桌面悬浮窗、托盘或全局快捷键。
//! 这些 IPC 仍返回明确错误，避免旧前端误把不可用操作显示成成功。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{
    commands::AppState,
    error::{AppError, AppResult, ErrorCode},
};

pub const MAIN: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowConfig {
    pub main_always_on_top: bool,
    pub main_show_in_taskbar: bool,
    pub close_action: String,
    pub floating_enabled: bool,
    pub floating_always_on_top: bool,
    pub floating_click_through: bool,
    pub floating_opacity: f64,
    pub floating_show_in_taskbar: bool,
    pub floating_x: Option<f64>,
    pub floating_y: Option<f64>,
    pub floating_width: f64,
    pub floating_height: f64,
    pub tray_enabled: bool,
    pub shortcut_enabled: bool,
    pub shortcut_toggle: String,
    pub shortcut_quick_add: String,
    pub shortcut_today: String,
    pub shortcut_floating: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            main_always_on_top: false,
            main_show_in_taskbar: true,
            close_action: "tray".into(),
            floating_enabled: false,
            floating_always_on_top: true,
            floating_click_through: false,
            floating_opacity: 1.0,
            floating_show_in_taskbar: false,
            floating_x: None,
            floating_y: None,
            floating_width: 450.0,
            floating_height: 600.0,
            tray_enabled: true,
            shortcut_enabled: true,
            shortcut_toggle: "CmdOrCtrl+Alt+A".into(),
            shortcut_quick_add: "CmdOrCtrl+Alt+N".into(),
            shortcut_today: "CmdOrCtrl+Alt+D".into(),
            shortcut_floating: "Alt+Q".into(),
        }
    }
}

fn desktop_only<T>() -> AppResult<T> {
    Err(AppError::new(
        ErrorCode::NotConfigured,
        "Android 端不提供悬浮窗、系统托盘和全局快捷键。",
    ))
}

pub async fn load_config(_state: &AppState) -> AppResult<WindowConfig> {
    Ok(WindowConfig::default())
}

pub async fn save_config(_state: &AppState, _config: &WindowConfig) -> AppResult<()> {
    desktop_only()
}

pub fn safe_recovery(_app: &AppHandle, _config: &mut WindowConfig) -> bool {
    false
}

#[tauri::command]
pub async fn window_get_config() -> AppResult<serde_json::Value> {
    desktop_only()
}

#[tauri::command]
pub async fn window_set_config(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _config: WindowConfig,
) -> AppResult<WindowConfig> {
    desktop_only()
}

#[tauri::command]
pub async fn window_apply_action(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _action: String,
) -> AppResult<WindowConfig> {
    desktop_only()
}

#[tauri::command]
pub async fn window_reset_safe(
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> AppResult<WindowConfig> {
    desktop_only()
}

#[tauri::command]
pub async fn window_floating_state(
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    desktop_only()
}

#[tauri::command]
pub async fn window_floating_reset_position(_app: AppHandle) -> AppResult<bool> {
    desktop_only()
}

#[tauri::command]
pub async fn window_set_floating_opacity(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _opacity: f64,
) -> AppResult<f64> {
    desktop_only()
}

#[tauri::command]
pub async fn window_set_floating_size(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _width: f64,
    _height: f64,
) -> AppResult<serde_json::Value> {
    desktop_only()
}

#[tauri::command]
pub async fn app_quit(app: AppHandle) -> AppResult<()> {
    app.exit(0);
    Ok(())
}
