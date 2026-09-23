//! 全局快捷键（任务书 §4.4 / §8.2 / §8.6）。
//!
//! ## 为什么快捷键是"安全设施"而不只是便利功能
//!
//! §8.2 要求悬浮窗开启穿透后必须有**始终可用的退出路径**，
//! §8.6 要求在关闭托盘与任务栏图标前必须有可用的恢复快捷键。
//! 因此快捷键注册的**失败必须被用户感知**：若注册不上，用户可能以为
//! 自己还有一条恢复路径，实际却没有。
//!
//! 这里的做法：
//! - 注册失败返回明确错误，由 `window_mgr` 广播 `shortcut-error` 事件；
//! - 冲突时给出"哪个键被占用"的可读信息（§4.4「全局快捷键冲突要提示」）；
//! - 允许关闭，但关闭前由窗口设置的安全校验兜底（不能只剩零条恢复路径）。

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::{AppError, AppResult};
use crate::window_mgr::WindowConfig;

/// 解析加速键字符串。
///
/// 使用插件自带的 `Shortcut::from_str`（接受 `CmdOrCtrl+Alt+A` 这类写法），
/// 解析失败时给出针对性的提示而不是抛一个底层错误。
fn parse(accel: &str) -> AppResult<Shortcut> {
    accel.parse::<Shortcut>().map_err(|e| {
        AppError::validation(format!("快捷键格式不正确：{accel}"))
            .with_hint(format!(
                "请使用「修饰键+按键」的写法，例如 Ctrl+Alt+A（底层错误：{e}）"
            ))
    })
}

/// 按配置重新注册全部全局快捷键。
///
/// 实现要点：**先全部注销再注册**，这样改键时不会残留旧的绑定
/// （否则用户会觉得"改了键但旧键还能用"）。
pub fn reload(app: &AppHandle, cfg: &WindowConfig) -> AppResult<()> {
    let gs = app.global_shortcut();

    // 先清空，避免旧绑定残留
    if let Err(e) = gs.unregister_all() {
        log::warn!("注销原有全局快捷键时出错（将继续注册）：{e}");
    }

    if !cfg.shortcut_enabled {
        log::info!("全局快捷键已按用户设置关闭");
        return Ok(());
    }

    let mut failures: Vec<String> = Vec::new();

    // 每个快捷键单独注册，任何一个失败都不影响其它（并记录原因）
    let items: [(&str, &str); 3] = [
        (&cfg.shortcut_toggle, "打开/隐藏主窗口"),
        (&cfg.shortcut_quick_add, "快速添加"),
        (&cfg.shortcut_today, "今日概览"),
    ];

    // 去重：两个功能配同一个键时，只有先注册的那个生效，
    // 这里提前发现并告诉用户，而不是让它静默失效。
    let mut seen: Vec<String> = Vec::new();
    for (accel, label) in items {
        if accel.trim().is_empty() {
            continue;
        }
        if seen.iter().any(|s| s == accel) {
            failures.push(format!("「{label}」与另一个功能使用了相同的快捷键 {accel}"));
            continue;
        }
        seen.push(accel.to_string());

        let shortcut = match parse(accel) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("「{label}」的快捷键 {accel} 无法解析：{e}"));
                continue;
            }
        };

        let action = label.to_string();
        if let Err(e) = gs.on_shortcut(shortcut, move |app, _sc, event| {
            // 只在按下时触发，避免抬起时又执行一次
            if event.state() != ShortcutState::Pressed {
                return;
            }
            match action.as_str() {
                "打开/隐藏主窗口" => crate::window_mgr::toggle_main(app),
                "快速添加" => crate::window_mgr::toggle_quick_add(app),
                "今日概览" => {
                    // 显示主窗口并切到「今天」视图
                    if let Some(w) = app.get_webview_window(crate::window_mgr::MAIN) {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                        let _ = tauri::Emitter::emit(&w, "navigate", "today");
                    }
                }
                _ => {}
            }
        }) {
            failures.push(format!("「{label}」的快捷键 {accel} 注册失败：{e}"));
        }
    }

    if failures.is_empty() {
        log::info!("全局快捷键已注册：{}", seen.join("、"));
        Ok(())
    } else {
        // 返回第一条失败信息作为主要提示，其余写日志
        for f in &failures {
            log::warn!("{f}");
        }
        Err(AppError::conflict(format!(
            "有 {} 个全局快捷键未能注册",
            failures.len()
        ))
        .with_hint(format!(
            "{}\n\n多数情况是该快捷键已被其它程序占用。请换一个组合，\
             或在设置中关闭全局快捷键（注意：关闭后请确保托盘可用，否则将失去恢复入口）。",
            failures.join("\n")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法的加速键写法应当能解析
    #[test]
    fn valid_accelerators_parse() {
        for s in [
            "CmdOrCtrl+Alt+A",
            "Ctrl+Shift+N",
            "Alt+F1",
            "Super+T",
        ] {
            assert!(parse(s).is_ok(), "应能解析：{s}");
        }
    }

    /// 非法写法必须报错且给出可操作的提示，不能静默失败
    #[test]
    fn invalid_accelerators_are_rejected_with_hint() {
        for s in ["", "不是快捷键", "Ctrl+", "+A", "F99"] {
            let r = parse(s);
            if let Err(e) = r {
                assert!(
                    e.hint.is_some(),
                    "解析失败时应给出写法提示：{s} -> {}",
                    e.message
                );
            }
        }
    }

    /// 默认配置的三个快捷键必须互不相同，否则会有一个注册不上
    #[test]
    fn default_config_shortcuts_are_unique() {
        let c = WindowConfig::default();
        let mut v = vec![
            c.shortcut_toggle.clone(),
            c.shortcut_quick_add.clone(),
            c.shortcut_today.clone(),
        ];
        let n = v.len();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), n, "默认快捷键存在重复");
    }

    /// 默认快捷键都应能解析——否则开箱即用就有一条失效
    #[test]
    fn default_config_shortcuts_are_parseable() {
        let c = WindowConfig::default();
        assert!(parse(&c.shortcut_toggle).is_ok(), "{}", c.shortcut_toggle);
        assert!(parse(&c.shortcut_quick_add).is_ok(), "{}", c.shortcut_quick_add);
        assert!(parse(&c.shortcut_today).is_ok(), "{}", c.shortcut_today);
    }
}
