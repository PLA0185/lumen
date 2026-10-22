//! Local-only scene-triggered reminders.

pub mod capture;
pub mod matcher;
pub mod models;
pub mod repository;
pub mod state_machine;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use models::{CaptureFramePayload, SceneNoteItem, SceneRule, SceneTestResult};
use sqlx::Row;
use state_machine::{SceneState, SceneStateMachine, SceneTransition};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    commands::AppState,
    error::{AppError, AppResult},
};

#[derive(Default)]
pub struct SceneRuntime {
    capture_arm: Mutex<Option<CaptureArm>>,
    rules: Mutex<HashMap<String, RuntimeRule>>,
    runtime_hydrated: AtomicBool,
}

struct CaptureArm {
    test_rule: Option<String>,
    expires_at: Instant,
}

const CAPTURE_ARM_DURATION: Duration = Duration::from_secs(60);

struct RuntimeRule {
    rule: SceneRule,
    machine: SceneStateMachine,
}

impl SceneRuntime {
    pub fn capture_armed(&self) -> bool {
        let Ok(mut arm) = self.capture_arm.lock() else {
            return false;
        };
        if arm
            .as_ref()
            .is_some_and(|pending| Instant::now() >= pending.expires_at)
        {
            *arm = None;
        }
        arm.is_some()
    }

    fn arm(&self, test_rule: Option<String>) -> AppResult<()> {
        let mut arm = self
            .capture_arm
            .lock()
            .map_err(|_| AppError::internal("场景采集状态不可用"))?;
        *arm = Some(CaptureArm {
            test_rule,
            expires_at: Instant::now() + CAPTURE_ARM_DURATION,
        });
        Ok(())
    }

    fn cancel_capture(&self) -> AppResult<()> {
        *self
            .capture_arm
            .lock()
            .map_err(|_| AppError::internal("场景采集状态不可用"))? = None;
        Ok(())
    }

    fn take_capture_arm(&self) -> Option<Option<String>> {
        let mut arm = self.capture_arm.lock().ok()?;
        let pending = arm.take()?;
        (Instant::now() < pending.expires_at).then_some(pending.test_rule)
    }

    pub fn reset_rule(&self, app: &AppHandle, id: &str) {
        if let Ok(mut rules) = self.rules.lock() {
            if let Some(entry) = rules.get_mut(id) {
                entry.machine = SceneStateMachine::default();
            }
        }
        refresh_scene_window(app, self);
    }

    pub fn reset_all(&self, app: &AppHandle) {
        if let Ok(mut rules) = self.rules.lock() {
            for entry in rules.values_mut() {
                entry.machine = SceneStateMachine::default();
            }
        }
        crate::window_mgr::hide_scene_note(app);
    }
}

#[tauri::command]
pub async fn scene_capture_arm(app: AppHandle, runtime: State<'_, SceneRuntime>) -> AppResult<()> {
    ensure_capture_supported()?;
    runtime.arm(None)?;
    if let Err(error) = app.emit("scene-capture-armed", ()) {
        runtime.cancel_capture()?;
        return Err(AppError::internal(format!("无法通知采集状态：{error}")));
    }
    Ok(())
}

#[tauri::command]
pub async fn scene_test_arm(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    rule_id: String,
) -> AppResult<()> {
    ensure_capture_supported()?;
    repository::get_rule(&state.db, &rule_id).await?;
    runtime.arm(Some(rule_id))?;
    if let Err(error) = app.emit("scene-capture-armed", ()) {
        runtime.cancel_capture()?;
        return Err(AppError::internal(format!("无法通知采集状态：{error}")));
    }
    Ok(())
}

#[tauri::command]
pub async fn scene_capture_cancel(runtime: State<'_, SceneRuntime>) -> AppResult<()> {
    runtime.cancel_capture()
}

fn ensure_capture_supported() -> AppResult<()> {
    if capture::platform_supported() {
        Ok(())
    } else {
        Err(AppError::validation(
            "智能场景便签只支持 Windows 10 1903（18362）或更高版本；当前未开启任何屏幕监测。",
        ))
    }
}

/// Global-shortcut callback. The one-shot arm is consumed before starting capture, so even a
/// failed capture cannot turn into continuous recording.
pub fn capture_armed(app: &AppHandle) {
    let Some(runtime) = app.try_state::<SceneRuntime>() else {
        return;
    };
    let Some(test_rule) = runtime.take_capture_arm() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(capture::capture_foreground).await;
        match result {
            Ok(Ok(frame)) => {
                if let Some(rule_id) = test_rule {
                    let Some(state) = app.try_state::<AppState>() else {
                        return;
                    };
                    match test_rule_on_frame(&state.db, &rule_id, &frame).await {
                        Ok(result) => {
                            if let Err(error) = app.emit("scene-test-result", result) {
                                log::warn!("无法显示场景规则测试结果：{error}");
                            }
                        }
                        Err(error) => {
                            emit_capture_error(&app, "SCENE_TEST_FAILED", &error.to_string())
                        }
                    }
                } else {
                    let payload = CaptureFramePayload {
                        capture_id: uuid::Uuid::now_v7().to_string(),
                        png_base64: STANDARD.encode(&frame.png),
                        process_name: frame.process_name,
                        window_title: frame.window_title,
                        width: frame.width,
                        height: frame.height,
                    };
                    if let Err(error) = app.emit("scene-capture-ready", payload) {
                        log::warn!("无法将一次性画面预览发送给主窗口：{error}");
                    }
                }
            }
            Ok(Err(error)) => emit_capture_error(&app, "SCENE_CAPTURE_FAILED", &error.to_string()),
            Err(_) => emit_capture_error(&app, "SCENE_CAPTURE_FAILED", "窗口采集线程未能完成"),
        }
    });
}

fn emit_capture_error(app: &AppHandle, code: &str, message: &str) {
    let payload = serde_json::json!({ "code": code, "message": message });
    if let Err(error) = app.emit("scene-capture-error", payload) {
        log::warn!("场景采集失败事件无法发送：{error}");
    }
}

async fn test_rule_on_frame(
    db: &crate::db::Db,
    rule_id: &str,
    frame: &capture::ForegroundFrame,
) -> AppResult<SceneTestResult> {
    let started = Instant::now();
    let rule = repository::get_rule(db, rule_id).await?;
    let allowed = rule
        .program_allowlist
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&frame.process_name));
    let title_ok = rule.window_title_contains.is_empty()
        || rule.window_title_contains.iter().any(|term| {
            frame
                .window_title
                .to_lowercase()
                .contains(&term.to_lowercase())
        });
    if !allowed || !title_ok {
        return Ok(SceneTestResult {
            rule_id: Some(rule_id.to_owned()),
            passed: false,
            window_allowed: false,
            image_score: None,
            image_threshold: rule.template_config.as_ref().map(|config| config.threshold),
            ocr_conditions_met: None,
            reason_code: "WINDOW_NOT_ALLOWED".into(),
            explanation: "当前前台程序或窗口标题不符合规则白名单；没有运行内容识别。".into(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
    evaluate_rule(db, &rule, frame, started.elapsed().as_millis() as u64).await
}

async fn evaluate_rule(
    db: &crate::db::Db,
    rule: &SceneRule,
    frame: &capture::ForegroundFrame,
    elapsed_before: u64,
) -> AppResult<SceneTestResult> {
    let started = Instant::now();
    let image_score = if rule.match_mode.needs_image() {
        let template_id = rule
            .template_id
            .as_deref()
            .ok_or_else(|| AppError::validation("图像规则没有参考图"))?;
        let template = repository::template_bytes(db, template_id).await?;
        Some(matcher::image_score(
            &frame.png,
            &template,
            rule.template_config
                .as_ref()
                .ok_or_else(|| AppError::validation("图像规则没有选区配置"))?,
        )?)
    } else {
        None
    };
    let ocr_match = if rule.match_mode.needs_ocr() {
        let bytes = frame.png.clone();
        let model_dir = crate::paddle_ocr::bundled_model_dir();
        let recognized =
            tokio::task::spawn_blocking(move || crate::paddle_ocr::recognize(&bytes, &model_dir))
                .await
                .map_err(|_| AppError::internal("OCR 工作线程未能完成"))??;
        Some(matcher::ocr_terms_match(
            &recognized,
            &rule.required_terms,
            &rule.excluded_terms,
        ))
    } else {
        None
    };
    let threshold = rule.template_config.as_ref().map(|config| config.threshold);
    let image_match = image_score
        .zip(threshold)
        .is_none_or(|(score, threshold)| score >= threshold);
    let passed = image_match && ocr_match.unwrap_or(true);
    let elapsed_ms = elapsed_before.saturating_add(started.elapsed().as_millis() as u64);
    Ok(SceneTestResult {
        rule_id: Some(rule.id.clone()),
        passed,
        window_allowed: true,
        image_score,
        image_threshold: threshold,
        ocr_conditions_met: ocr_match,
        reason_code: if passed {
            "MATCH"
        } else if !image_match {
            "IMAGE_BELOW_THRESHOLD"
        } else {
            "OCR_TERMS_NOT_MET"
        }
        .into(),
        explanation: if passed {
            "当前前台窗口符合此规则。"
        } else {
            "当前画面未同时满足规则中的图像与文字条件。"
        }
        .into(),
        elapsed_ms,
    })
}

#[tauri::command]
pub async fn scene_rule_test_arm(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    id: String,
) -> AppResult<()> {
    scene_test_arm(app, state, runtime, id).await
}

#[tauri::command]
pub async fn scene_note_current(runtime: State<'_, SceneRuntime>) -> AppResult<Vec<SceneNoteItem>> {
    let rules = runtime
        .rules
        .lock()
        .map_err(|_| AppError::internal("场景便签状态不可用"))?;
    Ok(rules
        .values()
        .filter(|entry| entry.machine.should_show_note())
        .map(|entry| note_item(&entry.rule))
        .collect())
}

fn note_item(rule: &SceneRule) -> SceneNoteItem {
    SceneNoteItem {
        rule_id: rule.id.clone(),
        headline: rule.headline.clone(),
        body_md: rule.body_md.clone(),
        memo_id: rule.memo_id.clone(),
        step_id: rule.step_id.clone(),
        snoozed_until: None,
    }
}

#[tauri::command]
pub async fn scene_note_ack(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    id: String,
) -> AppResult<()> {
    {
        let mut rules = runtime
            .rules
            .lock()
            .map_err(|_| AppError::internal("场景便签状态不可用"))?;
        let entry = rules
            .get_mut(&id)
            .ok_or_else(|| AppError::not_found("场景规则", &id))?;
        entry.machine.acknowledge();
    }
    let now = crate::db::now_stamp();
    sqlx::query(
        "UPDATE scene_runtime SET state='acked',last_ack_at=?,updated_at=? WHERE rule_id=?",
    )
    .bind(&now)
    .bind(&now)
    .bind(&id)
    .execute(state.db.pool())
    .await?;
    refresh_scene_window(&app, &runtime);
    Ok(())
}

#[tauri::command]
pub async fn scene_note_snooze(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    id: String,
    minutes: Option<i64>,
) -> AppResult<()> {
    let duration = {
        let mut rules = runtime
            .rules
            .lock()
            .map_err(|_| AppError::internal("场景便签状态不可用"))?;
        let entry = rules
            .get_mut(&id)
            .ok_or_else(|| AppError::not_found("场景规则", &id))?;
        let duration = minutes
            .unwrap_or(entry.rule.default_snooze_minutes)
            .clamp(1, 1440);
        entry
            .machine
            .snooze(Instant::now(), Duration::from_secs(duration as u64 * 60));
        duration
    };
    let until = crate::db::to_db_time(crate::db::utc_now() + chrono::Duration::minutes(duration));
    sqlx::query(
        "UPDATE scene_runtime SET state='snoozed',snooze_until=?,updated_at=? WHERE rule_id=?",
    )
    .bind(&until)
    .bind(crate::db::now_stamp())
    .bind(&id)
    .execute(state.db.pool())
    .await?;
    refresh_scene_window(&app, &runtime);
    Ok(())
}

#[tauri::command]
pub async fn scene_note_open_flow(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    let rule = repository::get_rule(&state.db, &id).await?;
    let memo_id = rule
        .memo_id
        .ok_or_else(|| AppError::validation("此场景规则没有关联备忘或流程"))?;
    let window = app
        .get_webview_window(crate::window_mgr::MAIN)
        .ok_or_else(|| AppError::internal("Lumen 主窗口不可用"))?;
    window
        .show()
        .map_err(|error| AppError::internal(format!("打开 Lumen 主窗口失败：{error}")))?;
    window
        .unminimize()
        .map_err(|error| AppError::internal(format!("恢复 Lumen 主窗口失败：{error}")))?;
    window
        .set_focus()
        .map_err(|error| AppError::internal(format!("聚焦 Lumen 主窗口失败：{error}")))?;
    window
        .emit(
            "scene-open-flow",
            serde_json::json!({ "memoId": memo_id, "stepId": rule.step_id }),
        )
        .map_err(|error| AppError::internal(format!("打开关联流程失败：{error}")))?;
    Ok(())
}

fn refresh_scene_window(app: &AppHandle, runtime: &SceneRuntime) {
    let visible = runtime
        .rules
        .lock()
        .ok()
        .is_some_and(|rules| rules.values().any(|entry| entry.machine.should_show_note()));
    if visible {
        crate::window_mgr::show_scene_note(app);
    } else {
        crate::window_mgr::hide_scene_note(app);
    }
}

pub fn spawn_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if let Err(error) = monitor_once(&app).await {
                log::warn!("场景监测循环失败（不记录画面、标题或识别文本）：{error}");
            }
        }
    });
}

async fn monitor_once(app: &AppHandle) -> AppResult<()> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::internal("应用数据库状态不可用"))?;
    let runtime = app
        .try_state::<SceneRuntime>()
        .ok_or_else(|| AppError::internal("场景运行状态不可用"))?;
    let status = repository::monitor_status(&state.db, runtime.capture_armed()).await?;
    if !status.platform_supported
        || !status.enabled
        || status.paused
        || status.allowed_rule_count == 0
    {
        runtime.reset_all(app);
        return Ok(());
    }
    let rules = repository::list_rules(&state.db, false)
        .await?
        .into_iter()
        .filter(|rule| rule.enabled)
        .collect::<Vec<_>>();
    hydrate_runtime(&state.db, &runtime, &rules).await?;
    let foreground_process = match capture::foreground_process_name() {
        Ok(process_name) => process_name,
        Err(_) => {
            apply_observation(app, &state.db, &runtime, &rules, None, None).await?;
            return Ok(());
        }
    };
    let process_candidates = rules
        .iter()
        .filter(|rule| {
            rule.program_allowlist
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&foreground_process))
        })
        .collect::<Vec<_>>();
    if process_candidates.is_empty() {
        apply_observation(app, &state.db, &runtime, &rules, None, None).await?;
        return Ok(());
    }
    let foreground = match capture::foreground_info_if_process(&foreground_process) {
        Ok(info) => info,
        Err(_) => {
            apply_observation(app, &state.db, &runtime, &rules, None, None).await?;
            return Ok(());
        }
    };
    let candidates = process_candidates
        .into_iter()
        .filter(|rule| {
            rule.window_title_contains.is_empty()
                || rule.window_title_contains.iter().any(|term| {
                    foreground
                        .window_title
                        .to_lowercase()
                        .contains(&term.to_lowercase())
                })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        apply_observation(app, &state.db, &runtime, &rules, None, None).await?;
        return Ok(());
    }
    let frame = match tokio::task::spawn_blocking(capture::capture_foreground).await {
        Ok(Ok(frame))
            if frame.process_name == foreground.process_name
                && frame.window_title == foreground.window_title =>
        {
            frame
        }
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => return Ok(()),
    };
    let has_ocr = candidates.iter().any(|rule| rule.match_mode.needs_ocr());
    let recognized = if has_ocr {
        let bytes = frame.png.clone();
        let model_dir = crate::paddle_ocr::bundled_model_dir();
        tokio::task::spawn_blocking(move || crate::paddle_ocr::recognize(&bytes, &model_dir))
            .await
            .ok()
            .and_then(Result::ok)
    } else {
        None
    };
    let identity_hash = window_identity_hash(&foreground.process_name, &foreground.window_title);
    let mut matched = HashMap::new();
    let mut scores = HashMap::new();
    for rule in &rules {
        if !candidates.iter().any(|candidate| candidate.id == rule.id) {
            matched.insert(rule.id.clone(), false);
            continue;
        }
        let image_ok = if rule.match_mode.needs_image() {
            let Some(template_id) = rule.template_id.as_deref() else {
                matched.insert(rule.id.clone(), false);
                continue;
            };
            let template = repository::template_bytes(&state.db, template_id).await?;
            let config = rule
                .template_config
                .as_ref()
                .ok_or_else(|| AppError::validation("图像规则缺少选区配置"))?;
            let score = matcher::image_score(&frame.png, &template, config)?;
            scores.insert(rule.id.clone(), f64::from(score));
            score >= config.threshold
        } else {
            true
        };
        let text_ok = if rule.match_mode.needs_ocr() {
            recognized.as_ref().is_some_and(|text| {
                matcher::ocr_terms_match(text, &rule.required_terms, &rule.excluded_terms)
            })
        } else {
            true
        };
        matched.insert(rule.id.clone(), image_ok && text_ok);
    }
    apply_observation(
        app,
        &state.db,
        &runtime,
        &rules,
        Some((&matched, &identity_hash)),
        Some(&scores),
    )
    .await
}

async fn hydrate_runtime(
    db: &crate::db::Db,
    runtime: &SceneRuntime,
    rules: &[SceneRule],
) -> AppResult<()> {
    if runtime.runtime_hydrated.load(Ordering::Acquire) {
        return Ok(());
    }
    let rows = sqlx::query("SELECT rule_id,state,snooze_until FROM scene_runtime")
        .fetch_all(db.pool())
        .await?;
    let now = crate::db::utc_now();
    let persisted = rows
        .iter()
        .map(|row| {
            let id: String = row.try_get("rule_id")?;
            let state: String = row.try_get("state")?;
            let snooze_until: Option<String> = row.try_get("snooze_until")?;
            let remaining = snooze_until.and_then(|value| {
                chrono::DateTime::parse_from_rfc3339(&value)
                    .ok()
                    .and_then(|until| {
                        until
                            .with_timezone(&chrono::Utc)
                            .signed_duration_since(now)
                            .to_std()
                            .ok()
                    })
            });
            let state = match state.as_str() {
                "visible" => SceneState::Visible,
                "acked" => SceneState::Acked,
                "snoozed" => SceneState::Snoozed,
                "entering" => SceneState::Entering,
                "exiting" => SceneState::Exiting,
                _ => SceneState::Outside,
            };
            Ok::<_, sqlx::Error>((id, (state, remaining)))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    let mut in_memory = runtime
        .rules
        .lock()
        .map_err(|_| AppError::internal("场景运行状态不可用"))?;
    for rule in rules {
        let (state, remaining) = persisted
            .get(&rule.id)
            .copied()
            .unwrap_or((SceneState::Outside, None));
        in_memory
            .entry(rule.id.clone())
            .or_insert_with(|| RuntimeRule {
                rule: rule.clone(),
                machine: SceneStateMachine::restore(state, remaining),
            });
    }
    runtime.runtime_hydrated.store(true, Ordering::Release);
    Ok(())
}

fn window_identity_hash(process_name: &str, title: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(
        format!("{}\0{}", process_name.to_lowercase(), title).as_bytes(),
    ))
}

async fn apply_observation(
    app: &AppHandle,
    db: &crate::db::Db,
    runtime: &SceneRuntime,
    rules: &[SceneRule],
    observation: Option<(&HashMap<String, bool>, &str)>,
    scores: Option<&HashMap<String, f64>>,
) -> AppResult<()> {
    let now = Instant::now();
    let mut transitions = Vec::new();
    {
        let mut state = runtime
            .rules
            .lock()
            .map_err(|_| AppError::internal("场景运行状态不可用"))?;
        state.retain(|id, _| rules.iter().any(|rule| rule.id == *id));
        for rule in rules {
            let entry = state.entry(rule.id.clone()).or_insert_with(|| RuntimeRule {
                rule: rule.clone(),
                machine: SceneStateMachine::default(),
            });
            entry.rule = rule.clone();
            let matched = observation
                .and_then(|(matches, _)| matches.get(&rule.id))
                .copied()
                .unwrap_or(false);
            let transition = entry.machine.observe(
                matched,
                now,
                rule.stable_hits as u8,
                Duration::from_millis(rule.stable_leave_ms as u64),
            );
            if transition != SceneTransition::None {
                transitions.push((
                    rule.id.clone(),
                    transition,
                    entry.machine.state(),
                    observation.map(|(_, hash)| hash.to_owned()),
                    scores.and_then(|scores| scores.get(&rule.id).copied()),
                ));
            }
        }
    }
    let stamp = crate::db::now_stamp();
    for (id, transition, next_state, identity_hash, score) in transitions {
        let state_name = match next_state {
            SceneState::Outside => "outside",
            SceneState::Entering => "entering",
            SceneState::Visible => "visible",
            SceneState::Acked => "acked",
            SceneState::Snoozed => "snoozed",
            SceneState::Exiting => "exiting",
        };
        sqlx::query("UPDATE scene_runtime SET state=?,window_identity_hash=?,last_seen_at=?,last_fired_at=CASE WHEN ?='visible' THEN ? ELSE last_fired_at END,updated_at=? WHERE rule_id=?")
            .bind(state_name).bind(identity_hash).bind(&stamp).bind(state_name).bind(&stamp).bind(&stamp).bind(&id).execute(db.pool()).await?;
        repository::record_event(
            db,
            Some(&id),
            if transition == SceneTransition::Show {
                "trigger"
            } else {
                "leave"
            },
            if transition == SceneTransition::Show {
                "MATCH_STABLE"
            } else {
                "LEFT_STABLE"
            },
            score,
            0,
        )
        .await?;
    }
    let visible = runtime
        .rules
        .lock()
        .map_err(|_| AppError::internal("场景便签状态不可用"))?
        .values()
        .any(|entry| entry.machine.should_show_note());
    if visible {
        crate::window_mgr::show_scene_note(app);
    } else {
        crate::window_mgr::hide_scene_note(app);
    }
    Ok(())
}

#[cfg(test)]
mod capture_arm_tests {
    use super::*;

    #[test]
    fn one_shot_capture_can_be_cancelled_and_expires() {
        let runtime = SceneRuntime::default();
        runtime.arm(Some("rule-id".into())).unwrap();
        assert!(runtime.capture_armed());
        assert_eq!(runtime.take_capture_arm(), Some(Some("rule-id".into())));
        assert!(!runtime.capture_armed());

        runtime.arm(None).unwrap();
        runtime.cancel_capture().unwrap();
        assert!(!runtime.capture_armed());

        runtime.arm(None).unwrap();
        runtime
            .capture_arm
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .expires_at = Instant::now() - Duration::from_secs(1);
        assert!(!runtime.capture_armed());
        assert_eq!(runtime.take_capture_arm(), None);
    }
}
