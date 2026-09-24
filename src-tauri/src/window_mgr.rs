//! 窗口能力管理（任务书 §8）。
//!
//! ## 覆盖的要求
//!
//! | 条款 | 能力 | 实现 |
//! | --- | --- | --- |
//! | §8.1 | 始终置顶（主窗口与悬浮窗分别开关，重启恢复） | `set_always_on_top` |
//! | §8.2 | 鼠标穿透 + **始终可用的退出路径** | `set_ignore_cursor_events` + 托盘/全局快捷键/启动恢复 |
//! | §8.3 | 不透明度实时可调、合理下限、重启保持 | 前端 CSS 变量 + 配置持久化 |
//! | §8.4 | 窗口模式：主窗口 / 悬浮今日 / 极简 | 三个 webview 窗口 |
//! | §8.5 | 任务栏显隐独立开关；关闭是退出还是缩托盘 | `set_skip_taskbar` + 关闭行为设置 |
//! | §8.6 | 托盘显隐独立开关；菜单项完整 | 见 `lib.rs` 的托盘部分 |
//!
//! ## 安全底线（§8 反复强调的一条）
//!
//! > 不得因为穿透、透明度或隐藏任务栏，使用户必须删除配置文件才能找回窗口。
//!
//! 因此这里设置了三道**互相独立**的恢复路径：
//! 1. 托盘菜单「打开 / 隐藏主窗口」；
//! 2. 全局快捷键（默认 `Ctrl+Alt+A`，可关闭但关闭时会警告）；
//! 3. **启动时的安全恢复**：每次启动都把悬浮窗的穿透关闭、并把主窗口
//!    显示出来——这样即使上次退出时状态很"隐蔽"，重启一定能看到界面。
//!
//! 另外，开启穿透前会做前置检查：若托盘未启用且全局快捷键未注册，
//! 则**拒绝开启**并说明原因，而不是先把用户关在门外再想办法。

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

use crate::commands::AppState;

/// 主窗口标签
pub const MAIN: &str = "main";
/// 悬浮今日小窗标签
pub const FLOATING: &str = "floating";
/// 快速添加窗口标签
pub const QUICK_ADD: &str = "quick-add";

/// 悬浮窗默认宽度（紧凑但不拥挤）
const FLOATING_W: f64 = 320.0;
/// 悬浮窗默认高度
const FLOATING_H: f64 = 460.0;

/// 悬浮窗尺寸的取值边界。
///
/// 放宽到"能放下完整编辑表单"是为了让悬浮窗也能改任务详情；
/// 上限则防止用户误拖出一个比屏幕还大的窗口后找不回内容。
const FLOATING_W_MIN: f64 = 260.0;
const FLOATING_W_MAX: f64 = 1400.0;
const FLOATING_H_MIN: f64 = 200.0;
const FLOATING_H_MAX: f64 = 1800.0;

/// 快速添加窗尺寸（够放下一行输入与提示）
const QUICK_W: f64 = 520.0;
const QUICK_H: f64 = 120.0;

/// 不透明度的下限：低于这个值文字将不可读，也失去"找回窗口"的可能
pub const OPACITY_MIN: f64 = 0.25;

/// 窗口配置（持久化到数据库的 settings 表）
///
/// **容器级 `serde(default)` 是必须的**：以后新增配置项时，老用户数据库里
/// 存的 JSON 里没有这个键，若不加 default，整份配置会解析失败并整体回落到
/// 默认值——用户此前调好的置顶、穿透、快捷键会一次性被抹掉。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowConfig {
    // ---------------- 主窗口 ----------------
    /// 主窗口是否始终置顶（§8.1）——与"任务置顶"无关，界面必须区分
    pub main_always_on_top: bool,
    /// 主窗口是否显示在任务栏（§8.5）
    pub main_show_in_taskbar: bool,
    /// 关闭主窗口时的行为：`tray` 缩到托盘 / `quit` 退出（§8.5）
    pub close_action: String,

    // ---------------- 悬浮窗 ----------------
    /// 是否显示悬浮今日小窗
    pub floating_enabled: bool,
    /// 悬浮窗是否始终置顶
    pub floating_always_on_top: bool,
    /// 悬浮窗是否鼠标穿透（§8.2）
    pub floating_click_through: bool,
    /// 悬浮窗不透明度 0.25–1.0（§8.3）
    pub floating_opacity: f64,
    /// 悬浮窗是否显示在任务栏（默认 false，它是桌面组件而非独立应用窗口）
    pub floating_show_in_taskbar: bool,
    /// 悬浮窗位置（物理像素）；None 表示右下角默认位置
    pub floating_x: Option<f64>,
    pub floating_y: Option<f64>,
    /// 悬浮窗尺寸（逻辑像素）：用户拖动右下角把手调节后持久化
    pub floating_width: f64,
    pub floating_height: f64,

    // ---------------- 托盘与快捷键 ----------------
    /// 是否启用托盘图标（§8.6）
    pub tray_enabled: bool,
    /// 是否启用全局快捷键
    pub shortcut_enabled: bool,
    /// 打开/隐藏主窗口的全局快捷键
    pub shortcut_toggle: String,
    /// 打开快速添加的全局快捷键
    pub shortcut_quick_add: String,
    /// 今日概览的全局快捷键
    pub shortcut_today: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            main_always_on_top: false,
            main_show_in_taskbar: true,
            close_action: "tray".to_string(),

            floating_enabled: false,
            floating_always_on_top: true,
            floating_click_through: false,
            floating_opacity: 1.0,
            floating_show_in_taskbar: false,
            floating_x: None,
            floating_y: None,
            floating_width: FLOATING_W,
            floating_height: FLOATING_H,

            tray_enabled: true,
            shortcut_enabled: true,
            shortcut_toggle: "CmdOrCtrl+Alt+A".to_string(),
            shortcut_quick_add: "CmdOrCtrl+Alt+N".to_string(),
            shortcut_today: "CmdOrCtrl+Alt+D".to_string(),
        }
    }
}

impl WindowConfig {
    /// 归一化：把越界值收敛到安全范围，避免手工改数据库导致窗口不可见。
    pub fn normalize(&mut self) {
        // 不透明度必须有下限，否则用户会把窗口调成完全不可见（§8.3）
        if !self.floating_opacity.is_finite() {
            self.floating_opacity = 1.0;
        }
        self.floating_opacity = self.floating_opacity.clamp(OPACITY_MIN, 1.0);

        // 尺寸同样要收敛：0 或负数会让窗口"消失"，超大值会让内容拖出屏幕
        if !self.floating_width.is_finite() {
            self.floating_width = FLOATING_W;
        }
        if !self.floating_height.is_finite() {
            self.floating_height = FLOATING_H;
        }
        self.floating_width = self.floating_width.clamp(FLOATING_W_MIN, FLOATING_W_MAX);
        self.floating_height = self.floating_height.clamp(FLOATING_H_MIN, FLOATING_H_MAX);

        if self.close_action != "tray" && self.close_action != "quit" {
            self.close_action = "tray".to_string();
        }
    }

    /// 当前是否有可用的"找回窗口"路径（§8 的安全前提）
    pub fn has_recovery_path(&self) -> bool {
        self.tray_enabled || self.shortcut_enabled
    }
}

/// 从数据库读取窗口配置；不存在则写入默认值。
pub async fn load_config(state: &AppState) -> crate::error::AppResult<WindowConfig> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value_json FROM settings WHERE key = 'window_config'")
            .fetch_optional(state.db.pool())
            .await?;

    match row {
        Some((json,)) => {
            let mut cfg: WindowConfig = serde_json::from_str(&json).unwrap_or_default();
            cfg.normalize();
            Ok(cfg)
        }
        None => {
            let cfg = WindowConfig::default();
            save_config(state, &cfg).await?;
            Ok(cfg)
        }
    }
}

/// 写入窗口配置
pub async fn save_config(state: &AppState, cfg: &WindowConfig) -> crate::error::AppResult<()> {
    let json = serde_json::to_string(cfg)
        .map_err(|e| crate::error::AppError::internal(format!("序列化窗口配置失败：{e}")))?;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at) VALUES ('window_config', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
    )
    .bind(&json)
    .bind(&now)
    .execute(state.db.pool())
    .await?;
    Ok(())
}

// =============================================================================
// 悬浮窗与快速添加窗的创建
// =============================================================================

/// 创建悬浮今日小窗。
///
/// 若已存在则直接显示，不重复创建（避免出现两个悬浮窗）。
pub fn ensure_floating(app: &AppHandle, cfg: &WindowConfig) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(FLOATING) {
        let _ = w.show();
        apply_floating(app, cfg);
        return Ok(());
    }

    let floating_w = cfg.floating_width.clamp(FLOATING_W_MIN, FLOATING_W_MAX);
    let floating_h = cfg.floating_height.clamp(FLOATING_H_MIN, FLOATING_H_MAX);
    let mut builder =
        WebviewWindowBuilder::new(app, FLOATING, WebviewUrl::App("index.html".into()))
            .title("Lumen 今日")
            .inner_size(floating_w, floating_h)
            // 最小尺寸与后端校验边界保持一致，避免"拖到很小后内容挤成一团"
            .min_inner_size(FLOATING_W_MIN, FLOATING_H_MIN)
            .resizable(true)
            .decorations(false)
            // 悬浮窗必须是透明的，否则桌面组件会带一块不透明底色（§8.4）
            .transparent(true)
            .skip_taskbar(!cfg.floating_show_in_taskbar)
            .always_on_top(cfg.floating_always_on_top)
            .shadow(false)
            .visible(false); // 先隐藏，等定位与状态应用完再显示，避免位置跳变

    // 默认放右下角；有保存的位置则沿用
    if let (Some(x), Some(y)) = (cfg.floating_x, cfg.floating_y) {
        builder = builder.position(x, y);
    } else if let Ok(Some(mon)) = app.primary_monitor() {
        let size = mon.size();
        let scale = mon.scale_factor();
        let x = (size.width as f64 / scale) - floating_w - 24.0;
        let y = (size.height as f64 / scale) - floating_h - 80.0;
        builder = builder.position(x.max(0.0), y.max(0.0));
    }

    let win = builder.build()?;

    // 关闭时是"隐藏"而不是销毁：用户下次打开能保留位置与状态。
    // 真正的退出必须走托盘或主窗口，避免悬浮窗被关掉后无法恢复。
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window(FLOATING) {
                let _ = w.hide();
            }
        }
    });

    apply_floating(app, cfg);
    let _ = win.show();
    Ok(())
}

/// 把配置应用到悬浮窗（置顶 / 穿透 / 任务栏 / 不透明度）
pub fn apply_floating(app: &AppHandle, cfg: &WindowConfig) {
    let Some(w) = app.get_webview_window(FLOATING) else {
        return;
    };
    let _ = w.set_always_on_top(cfg.floating_always_on_top);
    let _ = w.set_ignore_cursor_events(cfg.floating_click_through);
    let _ = w.set_skip_taskbar(!cfg.floating_show_in_taskbar);
    // 不透明度由前端 CSS 应用（整窗 alpha 需要平台支持，CSS 更可控且能保证文字对比度）
    let _ = w.emit("floating-config", cfg.clone());
}

/// 创建快速添加窗（§3 要求独立的快速添加窗口）
pub fn ensure_quick_add(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(QUICK_ADD) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }

    let mut builder =
        WebviewWindowBuilder::new(app, QUICK_ADD, WebviewUrl::App("index.html".into()))
            .title("快速添加")
            .inner_size(QUICK_W, QUICK_H)
            .resizable(false)
            .decorations(true)
            .always_on_top(true)
            .center()
            .skip_taskbar(true);

    if let Ok(Some(mon)) = app.primary_monitor() {
        let scale = mon.scale_factor();
        let size = mon.size();
        let x = (size.width as f64 / scale) / 2.0 - QUICK_W / 2.0;
        let y = (size.height as f64 / scale) / 3.0;
        builder = builder.position(x.max(0.0), y.max(0.0));
    }

    let win = builder.build()?;

    let handle = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window(QUICK_ADD) {
                let _ = w.hide();
            }
        }
    });

    Ok(())
}

/// 显示或隐藏快速添加窗
pub fn toggle_quick_add(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(QUICK_ADD) {
        match w.is_visible() {
            Ok(true) => {
                let _ = w.hide();
            }
            _ => {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    } else if let Err(e) = ensure_quick_add(app) {
        log::error!("创建快速添加窗口失败：{e}");
    }
}

/// 切换主窗口显隐（托盘与全局快捷键共用）
pub fn toggle_main(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN) else {
        log::error!("主窗口不存在");
        return;
    };
    match w.is_visible() {
        Ok(true) => {
            let _ = w.hide();
        }
        _ => {
            // 显示时顺带取消最小化并抢焦点，确保用户一定看到
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }
}

/// 启动时的安全恢复（§8 的安全底线）。
///
/// 每次启动都执行，效果是：
/// - 悬浮窗的**穿透被强制关闭**（否则用户可能无法点击它）；
/// - 主窗口被显示并置顶到前台一次。
///
/// 这样即使用户上次把界面设置得极其"隐蔽"，重启后一定能看到主窗口。
pub fn safe_recovery(app: &AppHandle, cfg: &mut WindowConfig) -> bool {
    let mut changed = false;

    // 穿透在重启时不自动恢复：它是最容易让用户"失去操作能力"的设置。
    // 用户如需穿透，需在本次会话中重新开启（界面有明确开关）。
    if cfg.floating_click_through {
        cfg.floating_click_through = false;
        changed = true;
        log::info!("安全恢复：悬浮窗的鼠标穿透已在启动时关闭");
    }

    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }

    if let Some(w) = app.get_webview_window(FLOATING) {
        let _ = w.set_ignore_cursor_events(false);
    }

    changed
}

// =============================================================================
// IPC 命令
// =============================================================================

/// 读取当前窗口配置
#[tauri::command]
pub async fn window_get_config(
    state: tauri::State<'_, AppState>,
) -> crate::error::AppResult<serde_json::Value> {
    let cfg = load_config(&state).await?;
    let cfg = cfg.clone();
    Ok(serde_json::json!({
        "config": cfg,
        "opacityMin": OPACITY_MIN,
        "hasRecoveryPath": cfg.has_recovery_path(),
    }))
}

/// 更新窗口配置并立即应用。
///
/// 关键安全逻辑：开启穿透前检查是否至少有一条恢复路径（§8.2）；
/// 同时关闭托盘与快捷键并隐藏任务栏图标时直接拒绝（§8.6 明确禁止）。
#[tauri::command]
pub async fn window_set_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    config: WindowConfig,
) -> crate::error::AppResult<WindowConfig> {
    let old = load_config(&state).await?;
    let mut cfg = config;
    cfg.normalize();

    // ---- 安全校验 1：穿透必须有恢复路径（§8.2）----
    if cfg.floating_click_through && !cfg.has_recovery_path() {
        return Err(crate::error::AppError::conflict(
            "无法开启鼠标穿透：托盘与全局快捷键都被关闭了",
        )
        .with_hint(
            "穿透后悬浮窗无法被点击，必须保留至少一条恢复入口。\
             请先启用托盘图标或全局快捷键。",
        ));
    }

    // ---- 安全校验 2：不允许同时关闭所有可见入口（§8.6）----
    if !cfg.tray_enabled && !cfg.main_show_in_taskbar && cfg.close_action == "tray" {
        return Err(
            crate::error::AppError::conflict("无法同时隐藏任务栏图标并关闭托盘").with_hint(
                "关闭主窗口会缩到托盘，但托盘已被关闭，你将无法重新打开它。\
             请二选一：保留托盘，或把「关闭主窗口」改为「退出程序」。",
            ),
        );
    }

    // ---- 应用主窗口设置 ----
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.set_always_on_top(cfg.main_always_on_top);
        let _ = w.set_skip_taskbar(!cfg.main_show_in_taskbar);
    }

    // ---- 应用悬浮窗设置 ----
    if cfg.floating_enabled {
        if let Err(e) = ensure_floating(&app, &cfg) {
            return Err(crate::error::AppError::internal(format!(
                "创建悬浮窗失败：{e}"
            )));
        }
    } else if let Some(w) = app.get_webview_window(FLOATING) {
        let _ = w.hide();
    }

    apply_floating(&app, &cfg);

    // ---- 托盘显隐（§8.6）----
    if cfg.tray_enabled != old.tray_enabled {
        crate::set_tray_visible(&app, cfg.tray_enabled);
    }

    // ---- 全局快捷键重新注册 ----
    if cfg.shortcut_enabled != old.shortcut_enabled
        || cfg.shortcut_toggle != old.shortcut_toggle
        || cfg.shortcut_quick_add != old.shortcut_quick_add
        || cfg.shortcut_today != old.shortcut_today
    {
        if let Err(e) = crate::shortcuts::reload(&app, &cfg) {
            // 快捷键注册失败不应让整个设置更新失败，但必须让用户知道
            log::warn!("重新注册全局快捷键失败：{e}");
            let _ = app.emit("shortcut-error", e.to_string());
        }
    }

    save_config(&state, &cfg).await?;
    // 同步内存缓存：托盘菜单的同步处理器与窗口事件都读它
    state.set_cfg(&cfg);
    // 广播给所有窗口，让界面同步反映最新状态
    let _ = app.emit("window-config-changed", cfg.clone());
    // 菜单的勾选状态需要重建才能反映出来
    crate::refresh_tray_menu(&app);

    log::info!(
        "窗口配置已更新：置顶(主 {} / 悬浮 {})、穿透 {}、不透明度 {:.2}、任务栏(主 {} / 悬浮 {})",
        cfg.main_always_on_top,
        cfg.floating_always_on_top,
        cfg.floating_click_through,
        cfg.floating_opacity,
        cfg.main_show_in_taskbar,
        cfg.floating_show_in_taskbar
    );

    Ok(cfg)
}

/// 立即应用一个"即时操作"，不进设置页也能用（托盘菜单与悬浮窗按钮调用）
#[tauri::command]
pub async fn window_apply_action(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    action: String,
) -> crate::error::AppResult<WindowConfig> {
    let mut cfg = load_config(&state).await?;

    match action.as_str() {
        "toggle_main_top" => cfg.main_always_on_top = !cfg.main_always_on_top,
        "toggle_floating_top" => cfg.floating_always_on_top = !cfg.floating_always_on_top,
        "toggle_floating_click_through" => {
            let want = !cfg.floating_click_through;
            // 与设置页同一套安全检查：穿透必须有恢复路径
            if want && !cfg.has_recovery_path() {
                return Err(crate::error::AppError::conflict(
                    "无法开启鼠标穿透：托盘与全局快捷键都被关闭了",
                )
                .with_hint("请先启用托盘图标或全局快捷键，穿透后才有办法把窗口找回来。"));
            }
            cfg.floating_click_through = want;
        }
        "toggle_tray" => cfg.tray_enabled = !cfg.tray_enabled,
        "show_floating" => cfg.floating_enabled = true,
        "hide_floating" => cfg.floating_enabled = false,
        "show_main" => {
            if let Some(w) = app.get_webview_window(MAIN) {
                let _ = w.show();
                let _ = w.set_focus();
            }
            return Ok(cfg);
        }
        other => {
            return Err(crate::error::AppError::validation(format!(
                "未知的窗口操作：{other}"
            )))
        }
    }

    cfg.normalize();

    // 复用完整设置路径的安全校验
    window_set_config(app, state, cfg).await
}

/// 重置窗口状态到安全的默认值。
///
/// 这是"最后一道保险"：当用户把窗口设置得自己都找不回来时，
/// 可以从托盘调用它一次性恢复（§8「不得使用户必须删除配置文件才能找回窗口」）。
#[tauri::command]
pub async fn window_reset_safe(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> crate::error::AppResult<WindowConfig> {
    let old = load_config(&state).await?;

    // 保留托盘与快捷键的偏好（它们本身是恢复手段），其余回到安全默认
    let cfg = WindowConfig {
        tray_enabled: true, // 强制启用托盘：这是最重要的恢复入口
        shortcut_enabled: old.shortcut_enabled,
        shortcut_toggle: old.shortcut_toggle.clone(),
        shortcut_quick_add: old.shortcut_quick_add.clone(),
        shortcut_today: old.shortcut_today.clone(),
        // 主窗口恢复为可见、非置顶、在任务栏显示
        main_show_in_taskbar: true,
        ..WindowConfig::default()
    };

    log::warn!("执行窗口安全重置：隐藏的窗口会被重新显示，穿透与隐藏设置被清除");

    let applied = window_set_config(app.clone(), state, cfg).await?;

    // 额外保证：把两个窗口都显示出来并居中，确保用户立刻能看到
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.center();
        let _ = w.set_focus();
    }
    if let Some(w) = app.get_webview_window(FLOATING) {
        let _ = w.set_ignore_cursor_events(false);
        let _ = w.hide(); // 悬浮窗默认隐藏，避免再次造成干扰
    }

    Ok(applied)
}

/// 读取悬浮窗当前是否可交互（穿透状态），供悬浮窗自己显示提示
#[tauri::command]
pub async fn window_floating_state(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> crate::error::AppResult<serde_json::Value> {
    let cfg = load_config(&state).await?;
    let visible = app
        .get_webview_window(FLOATING)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    Ok(serde_json::json!({
        "enabled": cfg.floating_enabled,
        "visible": visible,
        "clickThrough": cfg.floating_click_through,
        "alwaysOnTop": cfg.floating_always_on_top,
        "opacity": cfg.floating_opacity,
        "width": cfg.floating_width,
        "height": cfg.floating_height,
    }))
}

/// 设置悬浮窗不透明度（悬浮窗内的滑块用，拖动时高频调用）。
///
/// 只写配置并广播事件，不重建窗口——重建会让用户看到闪烁。
#[tauri::command]
pub async fn window_set_floating_opacity(
    app: AppHandle,
    state: State<'_, AppState>,
    opacity: f64,
) -> crate::error::AppResult<f64> {
    let mut cfg = load_config(&state).await?;
    cfg.floating_opacity = if opacity.is_finite() { opacity } else { 1.0 };
    cfg.normalize();
    save_config(&state, &cfg).await?;
    apply_floating(&app, &cfg);
    Ok(cfg.floating_opacity)
}

/// 设置悬浮窗尺寸（拖动右下角把手结束后调用）。
#[tauri::command]
pub async fn window_set_floating_size(
    app: AppHandle,
    state: State<'_, AppState>,
    width: f64,
    height: f64,
) -> crate::error::AppResult<serde_json::Value> {
    let mut cfg = load_config(&state).await?;
    cfg.floating_width = if width.is_finite() { width } else { FLOATING_W };
    cfg.floating_height = if height.is_finite() {
        height
    } else {
        FLOATING_H
    };
    cfg.normalize();
    save_config(&state, &cfg).await?;

    // 立刻把规范化后的尺寸应用到真实窗口：
    // 用户可能拖到超出上限（或小于下限），窗口必须跟配置一致，
    // 否则会出现"界面显示的尺寸"与"下次启动的尺寸"不一致的怪现象。
    if let Some(w) = app.get_webview_window(FLOATING) {
        let _ = w.set_size(LogicalSize::new(cfg.floating_width, cfg.floating_height));
    }
    Ok(serde_json::json!({
        "width": cfg.floating_width,
        "height": cfg.floating_height,
    }))
}

/// 把悬浮窗移动到屏幕右下角（用户拖丢后的一键找回）
#[tauri::command]
pub async fn window_floating_reset_position(app: AppHandle) -> crate::error::AppResult<bool> {
    let Some(w) = app.get_webview_window(FLOATING) else {
        return Err(crate::error::AppError::not_found("悬浮窗", FLOATING));
    };
    if let Ok(Some(mon)) = app.primary_monitor() {
        let size = mon.size();
        let scale = mon.scale_factor();
        // 用配置里的尺寸算落点，否则调大窗口后会有一部分跑到屏幕外
        let (w_now, h_now) = match app.try_state::<AppState>() {
            Some(state) => match load_config(&state).await {
                Ok(cfg) => (cfg.floating_width, cfg.floating_height),
                Err(_) => (FLOATING_W, FLOATING_H),
            },
            None => (FLOATING_W, FLOATING_H),
        };
        let x = (size.width as f64 / scale) - w_now - 24.0;
        let y = (size.height as f64 / scale) - h_now - 80.0;
        w.set_position(LogicalPosition::new(x.max(0.0), y.max(0.0)))
            .map_err(|e| crate::error::AppError::internal(format!("移动悬浮窗失败：{e}")))?;
        w.set_size(LogicalSize::new(w_now, h_now))
            .map_err(|e| crate::error::AppError::internal(format!("调整悬浮窗尺寸失败：{e}")))?;
        let _ = w.show();
        let _ = w.set_ignore_cursor_events(false);
        return Ok(true);
    }
    Ok(false)
}

/// 退出应用（托盘「完全退出」与设置页使用）
#[tauri::command]
pub async fn app_quit(app: AppHandle) -> crate::error::AppResult<()> {
    log::info!("用户请求退出应用");
    app.exit(0);
    Ok(())
}

/// 记录悬浮窗位置，供下次启动恢复。
///
/// 实现注意：`State<'_, _>` 是从 AppHandle 借出来的 guard，不能跨 await
/// 逃逸到异步任务里。因此这里先把位置读出来，再克隆 AppHandle 进异步块，
/// 在块内重新取 state。
pub fn remember_floating_position(app: &AppHandle) {
    let Some(w) = app.get_webview_window(FLOATING) else {
        return;
    };
    let Ok(pos) = w.outer_position() else { return };
    let Ok(scale) = w.scale_factor() else { return };
    if scale <= 0.0 {
        return;
    }

    let x = pos.x as f64 / scale;
    let y = pos.y as f64 / scale;

    // 位置写入是高频事件（拖动过程中连续触发），因此：
    // 1) 只在数值变化超过 1px 时才落库，避免浮点噪声写爆数据库；
    // 2) 失败只记 debug 日志——位置记忆失败不该影响用户操作。
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let Ok(mut cfg) = load_config(&state).await else {
            return;
        };

        if (cfg.floating_x.unwrap_or(f64::MIN) - x).abs() < 1.0
            && (cfg.floating_y.unwrap_or(f64::MIN) - y).abs() < 1.0
        {
            return;
        }

        cfg.floating_x = Some(x);
        cfg.floating_y = Some(y);
        if let Err(e) = save_config(&state, &cfg).await {
            log::debug!("保存悬浮窗位置失败：{e}");
        }
    });
}

/// 供托盘使用的暂停提醒切换
pub async fn toggle_reminders_paused(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let now = !state.reminders_paused.load(Ordering::Relaxed);
    state.reminders_paused.store(now, Ordering::Relaxed);
    let _ = app.emit("reminders-paused-changed", now);
    now
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_safe() {
        let c = WindowConfig::default();
        // 默认必须能找回窗口
        assert!(c.has_recovery_path(), "默认必须有恢复路径");
        assert!(c.tray_enabled, "默认应启用托盘");
        assert!(c.shortcut_enabled, "默认应启用全局快捷键");
        assert!(c.main_show_in_taskbar, "默认主窗口应显示在任务栏");
        assert_eq!(c.close_action, "tray");
        // 穿透默认关闭：它是最容易让用户失去操作能力的设置
        assert!(!c.floating_click_through, "默认不应开启穿透");
        assert_eq!(c.floating_opacity, 1.0, "默认不透明度为 100%");
    }

    /// §8.3：不透明度必须有合理下限，"防止完全不可见"
    #[test]
    fn opacity_is_clamped_to_readable_range() {
        let mut c = WindowConfig::default();

        c.floating_opacity = 0.0;
        c.normalize();
        assert_eq!(c.floating_opacity, OPACITY_MIN, "低于下限应被抬到下限");

        c.floating_opacity = -5.0;
        c.normalize();
        assert_eq!(c.floating_opacity, OPACITY_MIN);

        c.floating_opacity = 2.0;
        c.normalize();
        assert_eq!(c.floating_opacity, 1.0, "高于 1 应被收敛到 1");

        c.floating_opacity = f64::NAN;
        c.normalize();
        assert_eq!(c.floating_opacity, 1.0, "NaN 不应导致窗口消失");
    }

    #[test]
    fn opacity_min_is_readable() {
        // 下限不能低到让文字不可读
        assert!(OPACITY_MIN >= 0.2, "下限过低会让文字不可读");
        assert!(OPACITY_MIN <= 0.5, "下限过高会失去半透明的意义");
    }

    #[test]
    fn unknown_close_action_falls_back_to_tray() {
        let mut c = WindowConfig::default();
        c.close_action = "explode".into();
        c.normalize();
        assert_eq!(c.close_action, "tray", "未知关闭行为应回落到最安全的值");
    }

    #[test]
    fn recovery_path_requires_tray_or_shortcut() {
        let mut c = WindowConfig::default();
        assert!(c.has_recovery_path());

        c.tray_enabled = false;
        assert!(c.has_recovery_path(), "还有快捷键，仍可恢复");

        c.shortcut_enabled = false;
        assert!(!c.has_recovery_path(), "两者都关就没有恢复路径了");
    }

    // =====================================================================
    // 整改任务书 §16：窗口 / 托盘的"找不回程序"防线
    // =====================================================================
    //
    // 最坏组合是：悬浮窗穿透 + 隐藏主窗口任务栏图标 + 关掉托盘 + 关掉快捷键。
    // 这时用户没有任何入口，只能删配置文件。下面的用例逐条锁住防线：
    // 1. 默认配置必须安全；
    // 2. 参数归一化把越界值收敛（手工改库也救得回来）；
    // 3. `has_recovery_path` 是"是否允许开启穿透"的判定依据。

    /// 默认配置必须是"能看见、能找回"的
    #[test]
    fn defaults_never_lock_the_user_out() {
        let c = WindowConfig::default();
        assert!(c.tray_enabled, "默认启用托盘");
        assert!(c.shortcut_enabled, "默认启用全局快捷键");
        assert!(c.main_show_in_taskbar, "默认主窗口在任务栏可见");
        assert!(!c.floating_click_through, "默认不开启穿透");
        assert!(c.has_recovery_path());
    }

    /// 四种显隐组合里，"托盘关 + 快捷键关"必须被判定为无恢复路径，
    /// 不论任务栏与穿透怎么设置——因为这两项用户都可能看不到。
    #[test]
    fn no_recovery_path_when_both_entries_disabled() {
        for taskbar in [true, false] {
            for through in [true, false] {
                let mut c = WindowConfig {
                    tray_enabled: false,
                    shortcut_enabled: false,
                    main_show_in_taskbar: taskbar,
                    floating_click_through: through,
                    ..WindowConfig::default()
                };
                assert!(
                    !c.has_recovery_path(),
                    "托盘与快捷键都关时必须判定为无恢复路径（任务栏={taskbar} 穿透={through}）"
                );
                // 归一化不应把它"修好"——这是用户的显式选择，
                // 只是不允许在这种状态下再开启穿透（由 window_set_config 拦截）
                c.normalize();
                assert!(!c.has_recovery_path());
            }
        }
    }

    /// 尺寸与不透明度的越界值都要被收敛，避免手工改库把窗口弄没
    #[test]
    fn size_and_opacity_are_always_within_bounds() {
        let mut c = WindowConfig::default();

        for bad in [0.0, -100.0, f64::NAN, f64::INFINITY] {
            c.floating_width = bad;
            c.floating_height = bad;
            c.normalize();
            assert!(
                c.floating_width >= 260.0 && c.floating_width <= 1400.0,
                "宽度应被收敛，实际 {}",
                c.floating_width
            );
            assert!(
                c.floating_height >= 200.0 && c.floating_height <= 1800.0,
                "高度应被收敛，实际 {}",
                c.floating_height
            );
        }

        for bad in [0.0, -1.0, 5.0, f64::NAN] {
            c.floating_opacity = bad;
            c.normalize();
            assert!(
                (0.25..=1.0).contains(&c.floating_opacity),
                "不透明度应被收敛，实际 {}",
                c.floating_opacity
            );
        }
    }

    /// 新增配置项时，老配置不能整体回落到默认值（容器级 serde(default)）
    #[test]
    fn old_config_json_without_new_fields_keeps_user_settings() {
        // 模拟 0.2.0 时代的配置：没有 floatingWidth/Height
        let old = r#"{
            "mainAlwaysOnTop": true,
            "mainShowInTaskbar": false,
            "closeAction": "quit",
            "floatingEnabled": true,
            "floatingAlwaysOnTop": false,
            "floatingClickThrough": false,
            "floatingOpacity": 0.7,
            "floatingShowInTaskbar": true,
            "floatingX": 12.0,
            "floatingY": 34.0,
            "trayEnabled": true,
            "shortcutEnabled": true,
            "shortcutToggle": "CmdOrCtrl+Alt+A",
            "shortcutQuickAdd": "CmdOrCtrl+Alt+N",
            "shortcutToday": "CmdOrCtrl+Alt+D"
        }"#;
        let c: WindowConfig = serde_json::from_str(old).expect("老配置必须能解析");
        assert!(c.main_always_on_top, "老配置里的置顶不能被重置");
        assert_eq!(c.close_action, "quit", "关闭行为要保留");
        assert!((c.floating_opacity - 0.7).abs() < 1e-9);
        assert_eq!(c.floating_x, Some(12.0));
        // 新字段取默认值，而不是让整份配置失效
        assert!(c.floating_width > 0.0);
        assert!(c.floating_height > 0.0);
    }

    /// 默认快捷键不能互相冲突，否则注册时会有一个失败
    #[test]
    fn default_shortcuts_are_distinct() {
        let c = WindowConfig::default();
        assert_ne!(c.shortcut_toggle, c.shortcut_quick_add);
        assert_ne!(c.shortcut_toggle, c.shortcut_today);
        assert_ne!(c.shortcut_quick_add, c.shortcut_today);
        // 且都应是非空的加速键描述
        for s in [&c.shortcut_toggle, &c.shortcut_quick_add, &c.shortcut_today] {
            assert!(s.contains('+'), "快捷键应形如 Mod+Key：{s}");
        }
    }

    #[test]
    fn config_roundtrips_through_json() {
        let mut c = WindowConfig::default();
        c.floating_enabled = true;
        c.floating_opacity = 0.8;
        c.floating_x = Some(100.0);
        c.floating_y = Some(200.0);
        c.main_always_on_top = true;

        let s = serde_json::to_string(&c).unwrap();
        let back: WindowConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.floating_enabled, true);
        assert!((back.floating_opacity - 0.8).abs() < 1e-9);
        assert_eq!(back.floating_x, Some(100.0));
        assert!(back.main_always_on_top);
    }

    /// 数据库里可能是旧版本或被手工改坏的 JSON，此时必须回落到默认而不是崩溃
    #[test]
    fn malformed_json_falls_back_to_default() {
        let bad = "{ this is not json";
        let cfg: WindowConfig = serde_json::from_str(bad).unwrap_or_default();
        assert!(cfg.has_recovery_path(), "解析失败时应回落到安全的默认配置");
    }
}
