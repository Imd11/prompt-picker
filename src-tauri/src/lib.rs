use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Emitter, Listener, Manager, WindowEvent,
};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub(crate) const PRODUCT_NAME: &str = "Sleepy Cat";
const LEGACY_PRODUCT_NAME: &str = "Prompt Drawer";
const PROMPT_PICK_CAPTURE_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(900);

#[cfg(debug_assertions)]
mod calico_probe;
mod platform;
pub use platform::{
    accessibility_status, frontmost_app, request_accessibility_permission, AccessibilityStatus,
    AutosendOutcome, CandidateInput, FrontmostApp, FrontmostAppWithPid,
};
mod overlay_position;
pub use overlay_position::{prompt_button_position, OverlayPoint};
mod visual_liveness;
mod visual_liveness_log;
mod windows;
pub use windows::{
    acknowledge_prompt_popover_mode, hide_prompt_button, hide_prompt_popover,
    move_prompt_button_to, prompt_button_position_cmd, show_prompt_button,
    show_prompt_button_controls_from_button, show_prompt_popover, show_prompt_popover_from_button,
    toggle_prompt_popover_from_button,
};
mod macos_panels;
pub use macos_panels::{activate_main_window, configure_non_activating_panel};
mod prompt_files;
pub use prompt_files::{
    prompt_library_file_metadata, read_prompt_library_file, write_prompt_library_file,
};

#[tauri::command]
fn accessibility_status_cmd() -> AccessibilityStatus {
    accessibility_status()
}

#[tauri::command]
fn request_accessibility_permission_cmd() -> AccessibilityStatus {
    request_accessibility_permission()
}

#[derive(Clone, Debug, serde::Serialize)]
struct PromptInteractionPermissionStatus {
    required: bool,
    trusted: bool,
    native_prompt_requested: bool,
    language: String,
}

fn prompt_interaction_permission_status_from_parts(
    required: bool,
    trusted: bool,
    native_prompt_requested: bool,
    language: String,
) -> PromptInteractionPermissionStatus {
    PromptInteractionPermissionStatus {
        required,
        trusted: if required { trusted } else { true },
        native_prompt_requested,
        language,
    }
}

#[tauri::command]
fn prompt_interaction_permission_status(
    settings_state: tauri::State<SettingsFileState>,
) -> PromptInteractionPermissionStatus {
    let settings = settings_state.read_value();
    prompt_interaction_permission_status_from_parts(
        cfg!(target_os = "macos"),
        accessibility_status().trusted,
        accessibility_prompt_requested(&settings),
        settings_language(&settings).to_string(),
    )
}

#[tauri::command]
fn request_prompt_interaction_permission(
    settings_state: tauri::State<SettingsFileState>,
) -> AccessibilityStatus {
    let _ = set_accessibility_prompt_requested(settings_state.inner(), true);
    request_accessibility_permission()
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    platform::macos::open_accessibility_settings()
}

#[tauri::command]
fn frontmost_app_cmd() -> Option<FrontmostApp> {
    frontmost_app()
}

#[tauri::command]
fn current_input_target(
    state: tauri::State<LastInputTargetState>,
) -> Option<platform::InputTarget> {
    if let Some(target) = platform::macos::current_input_target() {
        record_last_input_target_if_valid(state.inner(), &target);
        return Some(target);
    }

    if let Some(app) = platform::frontmost_app_with_pid() {
        record_last_app_if_valid(state.inner(), app);
    }

    None
}

#[tauri::command]
async fn begin_prompt_pick_session(
    session_id: u64,
    session_state: tauri::State<'_, PromptPickSessionState>,
    recent_state: tauri::State<'_, LastInputTargetState>,
) -> Result<Option<FrontmostApp>, String> {
    let session_state = session_state.inner().clone();
    let recent_state = recent_state.inner().clone();
    let captured_app = tauri::async_runtime::spawn_blocking(move || {
        capture_prompt_pick_session_target(&session_state, &recent_state, session_id)
    })
    .await
    .map_err(|error| format!("Prompt pick session task failed: {}", error))?;

    Ok(captured_app)
}

pub(crate) fn capture_prompt_pick_session_target(
    session_state: &PromptPickSessionState,
    recent_state: &LastInputTargetState,
    session_id: u64,
) -> Option<FrontmostApp> {
    session_state.begin_if_new(session_id);
    let frontmost = current_prompt_pick_frontmost_candidate();

    if let Some(input_target) = platform::macos::current_input_target() {
        record_last_input_target_if_valid(recent_state, &input_target);
    }

    let recent_identity = recent_state.captured_identity();
    let target = prompt_pick_session_target(frontmost, recent_state.get())?;
    let identity = captured_identity_for_target(&target, recent_identity.as_ref())?;
    let target_pid = target.pid;
    let target_bundle_id = target.app.bundle_id.clone();
    let captured_app =
        record_prompt_pick_session_target_if_valid(session_state, target, identity, session_id);
    if captured_app.is_some() {
        if let Some(pid) = target_pid {
            if let Some(page_url) = platform::macos::active_browser_page_url(pid, &target_bundle_id)
            {
                session_state.set_page_url_if_current(session_id, &target_bundle_id, page_url);
            }
        }
    }
    captured_app
}

pub(crate) fn prepare_prompt_pick_session_target(
    session_state: &PromptPickSessionState,
    recent_state: &LastInputTargetState,
    session_id: u64,
) -> Option<FrontmostApp> {
    session_state.begin(session_id);
    let captured_app = freeze_prompt_pick_session_target(
        session_state,
        current_prompt_pick_frontmost_candidate(),
        recent_state.get(),
        session_id,
    );
    session_state.mark_capture_pending(session_id);
    captured_app
}

fn current_prompt_pick_frontmost_candidate() -> Option<FrontmostAppWithPid> {
    let frontmost = platform::frontmost_app_with_pid();
    let needs_underlying_app = frontmost.as_ref().is_none_or(|frontmost| {
        frontmost.pid == Some(std::process::id()) || is_prompt_picker_app(&frontmost.app)
    });
    let underlying_app = needs_underlying_app
        .then(|| platform::macos::topmost_external_app_with_pid(std::process::id()))
        .flatten();
    prompt_pick_frontmost_candidate(frontmost, underlying_app)
}

#[tauri::command]
fn paste_prompt(body: String, app: tauri::AppHandle) -> Result<(), String> {
    platform::macos::paste_prompt_with_copier(&body, |text| copy_text_to_clipboard(&app, text))
}

#[tauri::command]
async fn paste_prompt_and_submit_to_last_target(
    body: String,
    submit_key: Option<String>,
    session_state: tauri::State<'_, PromptPickSessionState>,
    recent_state: tauri::State<'_, LastInputTargetState>,
    settings_state: tauri::State<'_, SettingsFileState>,
    app: tauri::AppHandle,
) -> Result<AutosendOutcome, String> {
    native_submit_key_from_arg(submit_key)?;
    let submit_key = authoritative_submit_key(settings_state.read_text());
    let session_state = session_state.inner().clone();
    let recent_state = recent_state.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        paste_prompt_and_submit_to_last_target_impl(
            &body,
            &session_state,
            &recent_state,
            &app,
            submit_key,
        )
    })
    .await
    .map_err(|error| format!("Autosend task failed: {}", error))?
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct AutosendSequenceOutcome {
    pub copied: bool,
    pub sent: bool,
    pub sent_count: usize,
    pub processed_count: usize,
    pub completion: Option<platform::AutosendCompletion>,
    pub failed_index: Option<usize>,
    pub error: Option<String>,
    pub reason: Option<platform::macos::AutosendFailureReason>,
}

impl AutosendSequenceOutcome {
    fn sent_all(count: usize) -> Self {
        Self {
            copied: true,
            sent: true,
            sent_count: count,
            processed_count: count,
            completion: Some(platform::AutosendCompletion::Submitted),
            failed_index: None,
            error: None,
            reason: None,
        }
    }

    fn from_failure(outcome: AutosendOutcome, sent_count: usize, failed_index: usize) -> Self {
        Self {
            copied: outcome.copied,
            sent: false,
            sent_count,
            processed_count: sent_count,
            completion: outcome.completion,
            failed_index: Some(failed_index),
            error: outcome.error,
            reason: outcome.reason,
        }
    }

    fn from_paste_only(outcome: AutosendOutcome, processed_count: usize) -> Self {
        Self {
            copied: outcome.copied,
            sent: false,
            sent_count: 0,
            processed_count,
            completion: outcome.completion,
            failed_index: None,
            error: outcome.error,
            reason: outcome.reason,
        }
    }
}

#[tauri::command]
async fn paste_prompt_sequence_and_submit_to_last_target(
    bodies: Vec<String>,
    interval_ms: u64,
    submit_key: Option<String>,
    session_state: tauri::State<'_, PromptPickSessionState>,
    recent_state: tauri::State<'_, LastInputTargetState>,
    settings_state: tauri::State<'_, SettingsFileState>,
    app: tauri::AppHandle,
) -> Result<AutosendSequenceOutcome, String> {
    native_submit_key_from_arg(submit_key)?;
    let submit_key = authoritative_submit_key(settings_state.read_text());
    let session_state = session_state.inner().clone();
    let recent_state = recent_state.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if submit_key == platform::macos::NativeSubmitKey::None {
            let body = bodies.join("\n\n");
            let outcome = paste_prompt_and_submit_to_last_target_impl(
                &body,
                &session_state,
                &recent_state,
                &app,
                submit_key,
            )?;
            return Ok(
                if outcome.completion == Some(platform::AutosendCompletion::PastedOnly) {
                    AutosendSequenceOutcome::from_paste_only(outcome, bodies.len())
                } else if outcome.sent {
                    AutosendSequenceOutcome::sent_all(bodies.len())
                } else {
                    AutosendSequenceOutcome::from_failure(outcome, 0, 0)
                },
            );
        }
        paste_prompt_sequence_and_submit_to_last_target_impl(
            &bodies,
            interval_ms,
            &session_state,
            &recent_state,
            &app,
            submit_key,
        )
    })
    .await
    .map_err(|error| format!("Autosend sequence task failed: {}", error))?
}

fn paste_prompt_sequence_and_submit_to_last_target_impl(
    bodies: &[String],
    interval_ms: u64,
    state: &PromptPickSessionState,
    recent_state: &LastInputTargetState,
    app: &tauri::AppHandle,
    submit_key: platform::macos::NativeSubmitKey,
) -> Result<AutosendSequenceOutcome, String> {
    state.wait_for_capture(PROMPT_PICK_CAPTURE_WAIT_TIMEOUT);
    let Some(captured_identity) = state
        .captured_identity()
        .or_else(|| recent_state.captured_identity())
    else {
        return Ok(AutosendSequenceOutcome::from_failure(
            stale_target_outcome(),
            0,
            0,
        ));
    };
    let target_pid = captured_identity.application.main_pid;
    let target_launch_identity = captured_identity.application.launch_identity;
    let captured_window = captured_identity.window;
    let captured_page_url = captured_identity.page_url;
    let submit_key = platform::macos::effective_submit_key_for_target(
        &captured_identity.application.bundle_id,
        target_pid,
        captured_page_url.as_deref(),
        submit_key,
    );
    paste_prompt_sequence_and_submit_to_session_target_with_senders(
        bodies,
        interval_ms,
        state,
        None,
        submit_key,
        |body, bundle_id, click_point, submit_key| {
            platform::macos::paste_prompt_and_submit_to_app_clipboard_with_copier(
                body,
                bundle_id,
                target_pid,
                target_launch_identity,
                click_point.map(|point| (point.x, point.y)),
                captured_window.as_ref(),
                captured_page_url.as_deref(),
                submit_key,
                |pid| activate_target_process(app, pid),
                |text| copy_text_to_clipboard(app, text),
            )
        },
        |text| copy_text_to_clipboard(app, text),
        |delay_ms| std::thread::sleep(std::time::Duration::from_millis(delay_ms)),
    )
}

fn paste_prompt_and_submit_to_last_target_impl(
    body: &str,
    state: &PromptPickSessionState,
    recent_state: &LastInputTargetState,
    app: &tauri::AppHandle,
    submit_key: platform::macos::NativeSubmitKey,
) -> Result<AutosendOutcome, String> {
    state.wait_for_capture(PROMPT_PICK_CAPTURE_WAIT_TIMEOUT);
    let Some(captured_identity) = state
        .captured_identity()
        .or_else(|| recent_state.captured_identity())
    else {
        return Ok(stale_target_outcome());
    };
    let target_pid = captured_identity.application.main_pid;
    let target_launch_identity = captured_identity.application.launch_identity;
    let captured_window = captured_identity.window;
    let captured_page_url = captured_identity.page_url;
    paste_prompt_and_submit_to_session_target_with_senders(
        body,
        state,
        None,
        submit_key,
        |body, bundle_id, click_point, submit_key| {
            platform::macos::paste_prompt_and_submit_to_app_clipboard_with_copier(
                body,
                bundle_id,
                target_pid,
                target_launch_identity,
                click_point.map(|point| (point.x, point.y)),
                captured_window.as_ref(),
                captured_page_url.as_deref(),
                submit_key,
                |pid| activate_target_process(app, pid),
                |text| copy_text_to_clipboard(app, text),
            )
        },
        |text| copy_text_to_clipboard(app, text),
    )
}

#[cfg(target_os = "macos")]
fn activate_target_process(app: &tauri::AppHandle, pid: u32) -> Result<(), String> {
    crate::macos_panels::activate_running_application(app, pid)
}

#[cfg(not(target_os = "macos"))]
fn activate_target_process(_app: &tauri::AppHandle, _pid: u32) -> Result<(), String> {
    Err("Target activation is only implemented on macOS.".to_string())
}

fn copy_text_to_clipboard(app: &tauri::AppHandle, body: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(body)
        .map_err(|error| format!("Clipboard write failed: {}", error))
}

const MIN_SEQUENCE_INTERVAL_MS: u64 = 200;
const MAX_SEQUENCE_INTERVAL_MS: u64 = 3_000;

fn clamp_sequence_interval_ms(interval_ms: u64) -> u64 {
    interval_ms.clamp(MIN_SEQUENCE_INTERVAL_MS, MAX_SEQUENCE_INTERVAL_MS)
}

const FOCUS_PRESERVING_PASTE_SETTLE_MS: u64 = 180;

fn native_submit_key_from_arg(
    submit_key: Option<String>,
) -> Result<platform::macos::NativeSubmitKey, String> {
    match submit_key.as_deref().unwrap_or("enter") {
        "none" => Ok(platform::macos::NativeSubmitKey::None),
        "enter" => Ok(platform::macos::NativeSubmitKey::Enter),
        "command_enter" => Ok(platform::macos::NativeSubmitKey::CommandEnter),
        value => Err(format!("Invalid submit key: {}", value)),
    }
}

fn authoritative_submit_key(
    settings_text: Result<Option<String>, String>,
) -> platform::macos::NativeSubmitKey {
    settings_text
        .ok()
        .flatten()
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .and_then(|settings| {
            settings
                .pointer("/promptInsertion/mode")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .map(|mode| match mode.as_str() {
            "paste_only" => platform::macos::NativeSubmitKey::None,
            "paste_enter" | "paste_and_submit" | "paste_command_enter" => {
                platform::macos::NativeSubmitKey::Enter
            }
            _ => platform::macos::NativeSubmitKey::None,
        })
        .unwrap_or(platform::macos::NativeSubmitKey::None)
}

fn prompt_pick_target_or_recent(
    session_state: &PromptPickSessionState,
    recent_state: Option<&LastInputTargetState>,
) -> Option<CapturedPromptTarget> {
    if let Some((target, identity)) = session_state.take_captured() {
        return Some(CapturedPromptTarget { target, identity });
    }

    let recent_state = recent_state?;
    let target = recent_state.get()?;
    if !is_recent_prompt_target(&target) || !is_usable_autosend_app(&target.app) {
        return None;
    }
    Some(CapturedPromptTarget {
        target: PromptPickSessionTarget {
            app: target.app,
            pid: target.pid,
            observed_at_ms: now_ms(),
            click_point: target.click_point,
        },
        identity: recent_state.captured_identity(),
    })
}

struct CapturedPromptTarget {
    target: PromptPickSessionTarget,
    identity: Option<CapturedTargetIdentity>,
}

impl std::ops::Deref for CapturedPromptTarget {
    type Target = PromptPickSessionTarget;

    fn deref(&self) -> &Self::Target {
        &self.target
    }
}

#[allow(dead_code)]
fn focus_preserving_prompt_to_last_target_impl<C, F, R, P, S, W>(
    body: &str,
    state: &PromptPickSessionState,
    recent_state: Option<&LastInputTargetState>,
    submit_key: platform::macos::NativeSubmitKey,
    copy_sender: C,
    frontmost_reader: F,
    recover_target: R,
    paste_sender: P,
    submit_sender: S,
    sleeper: W,
) -> Result<AutosendOutcome, String>
where
    C: FnOnce(&str) -> Result<(), String>,
    F: FnMut() -> Option<FrontmostAppWithPid>,
    R: FnMut(&PromptPickSessionTarget) -> Result<(), String>,
    P: FnOnce() -> Result<(), String>,
    S: FnOnce(platform::macos::NativeSubmitKey) -> Result<(), String>,
    W: FnOnce(u64),
{
    let Some(target) = prompt_pick_target_or_recent(state, recent_state) else {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "No prompt pick target app was recorded for autosend.",
        ));
    };

    if target
        .identity
        .as_ref()
        .is_some_and(|identity| !captured_target_identity_is_current(identity))
    {
        return Ok(stale_target_outcome());
    }

    if is_unsafe_autosend_target(&target.app) {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "Target app is not safe for autosend.",
        ));
    }

    if !allows_app_only_autosend(&target.app) {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "Target app is not safe for app-only autosend.",
        ));
    }

    if !platform::macos::accessibility_status().trusted {
        return Ok(AutosendOutcome::missing_accessibility_permission());
    }

    Ok(guarded_focus_preserving_autosend_with_senders(
        body,
        &target,
        submit_key,
        copy_sender,
        frontmost_reader,
        recover_target,
        paste_sender,
        submit_sender,
        sleeper,
    ))
}

#[allow(dead_code)]
fn focus_preserving_prompt_sequence_to_last_target_impl<C, F, R, P, S, W>(
    bodies: &[String],
    interval_ms: u64,
    state: &PromptPickSessionState,
    recent_state: Option<&LastInputTargetState>,
    submit_key: platform::macos::NativeSubmitKey,
    copy_sender: C,
    frontmost_reader: F,
    recover_target: R,
    paste_sender: P,
    submit_sender: S,
    sleeper: W,
) -> Result<AutosendSequenceOutcome, String>
where
    C: FnMut(&str) -> Result<(), String>,
    F: FnMut() -> Option<FrontmostAppWithPid>,
    R: FnMut(&PromptPickSessionTarget) -> Result<(), String>,
    P: FnMut() -> Result<(), String>,
    S: FnMut(platform::macos::NativeSubmitKey) -> Result<(), String>,
    W: FnMut(u64),
{
    let clean_bodies: Vec<String> = bodies
        .iter()
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .collect();
    let Some(first_body) = clean_bodies.first() else {
        return Ok(AutosendSequenceOutcome::from_failure(
            AutosendOutcome::copy_failed("Prompt group is empty.".to_string()),
            0,
            1,
        ));
    };

    let Some(target) = prompt_pick_target_or_recent(state, recent_state) else {
        let outcome = copy_without_sending(
            first_body,
            copy_sender,
            "No prompt pick target app was recorded for autosend.",
        );
        return Ok(AutosendSequenceOutcome::from_failure(outcome, 0, 1));
    };

    if target
        .identity
        .as_ref()
        .is_some_and(|identity| !captured_target_identity_is_current(identity))
    {
        return Ok(AutosendSequenceOutcome::from_failure(
            stale_target_outcome(),
            0,
            0,
        ));
    }

    if is_unsafe_autosend_target(&target.app) || !allows_app_only_autosend(&target.app) {
        let outcome = copy_without_sending(
            first_body,
            copy_sender,
            "Target app is not safe for app-only autosend.",
        );
        return Ok(AutosendSequenceOutcome::from_failure(outcome, 0, 1));
    }

    if !platform::macos::accessibility_status().trusted {
        return Ok(AutosendSequenceOutcome::from_failure(
            AutosendOutcome::missing_accessibility_permission(),
            0,
            1,
        ));
    }

    Ok(focus_preserving_prompt_sequence_for_target_with_senders(
        &clean_bodies,
        interval_ms,
        &target,
        submit_key,
        copy_sender,
        frontmost_reader,
        recover_target,
        paste_sender,
        submit_sender,
        sleeper,
    ))
}

fn focus_preserving_prompt_sequence_for_target_with_senders<C, F, R, P, S, W>(
    clean_bodies: &[String],
    interval_ms: u64,
    target: &PromptPickSessionTarget,
    submit_key: platform::macos::NativeSubmitKey,
    mut copy_sender: C,
    mut frontmost_reader: F,
    mut recover_target: R,
    mut paste_sender: P,
    mut submit_sender: S,
    mut sleeper: W,
) -> AutosendSequenceOutcome
where
    C: FnMut(&str) -> Result<(), String>,
    F: FnMut() -> Option<FrontmostAppWithPid>,
    R: FnMut(&PromptPickSessionTarget) -> Result<(), String>,
    P: FnMut() -> Result<(), String>,
    S: FnMut(platform::macos::NativeSubmitKey) -> Result<(), String>,
    W: FnMut(u64),
{
    let delay_ms = clamp_sequence_interval_ms(interval_ms);
    for (index, body) in clean_bodies.iter().enumerate() {
        let outcome = guarded_focus_preserving_autosend_with_senders(
            body,
            target,
            submit_key,
            |text| copy_sender(text),
            || frontmost_reader(),
            |target| recover_target(target),
            || paste_sender(),
            |key| submit_sender(key),
            |delay_ms| sleeper(delay_ms),
        );
        if !outcome.sent {
            return AutosendSequenceOutcome::from_failure(outcome, index, index + 1);
        }
        if index + 1 < clean_bodies.len() {
            sleeper(delay_ms);
        }
    }

    AutosendSequenceOutcome::sent_all(clean_bodies.len())
}

fn guarded_focus_preserving_autosend_with_senders<C, F, R, P, S, W>(
    body: &str,
    target: &PromptPickSessionTarget,
    submit_key: platform::macos::NativeSubmitKey,
    copy_sender: C,
    mut frontmost_reader: F,
    mut recover_target: R,
    paste_sender: P,
    submit_sender: S,
    sleeper: W,
) -> AutosendOutcome
where
    C: FnOnce(&str) -> Result<(), String>,
    F: FnMut() -> Option<FrontmostAppWithPid>,
    R: FnMut(&PromptPickSessionTarget) -> Result<(), String>,
    P: FnOnce() -> Result<(), String>,
    S: FnOnce(platform::macos::NativeSubmitKey) -> Result<(), String>,
    W: FnOnce(u64),
{
    if let Err(error) = copy_sender(body) {
        return AutosendOutcome::copy_failed(error);
    }

    let before_paste = frontmost_reader();
    let classification = classify_target_frontmost(target, before_paste.as_ref());
    emit_autosend_diagnostic(
        "before-paste",
        target,
        before_paste.as_ref(),
        Some(classification),
    );
    match classification {
        TargetFrontmostStatus::Target => {}
        TargetFrontmostStatus::PromptPicker => {
            if recover_target(target).is_err() {
                emit_autosend_diagnostic(
                    "recovery-failed",
                    target,
                    before_paste.as_ref(),
                    Some(classification),
                );
                return AutosendOutcome::copied_without_send(
                    "Target app changed before paste; prompt was copied instead.".to_string(),
                );
            }
            let after_recovery = frontmost_reader();
            if !captured_target_matches_frontmost(target, after_recovery.as_ref()) {
                emit_autosend_diagnostic(
                    "post-recovery-mismatch",
                    target,
                    after_recovery.as_ref(),
                    Some(classification),
                );
                return AutosendOutcome::copied_without_send(
                    "Target app changed before paste; prompt was copied instead.".to_string(),
                );
            }
            emit_autosend_diagnostic(
                "post-recovery-target",
                target,
                after_recovery.as_ref(),
                Some(TargetFrontmostStatus::Target),
            );
        }
        TargetFrontmostStatus::OtherOrUnknown => {
            emit_autosend_diagnostic(
                "other-or-unknown",
                target,
                before_paste.as_ref(),
                Some(classification),
            );
            return AutosendOutcome::copied_without_send(
                "Target app changed before paste; prompt was copied instead.".to_string(),
            );
        }
    }

    if let Err(error) = paste_sender() {
        return AutosendOutcome::paste_event_failed(format!(
            "Focus-preserving paste failed: {}",
            error
        ));
    }

    sleeper(FOCUS_PRESERVING_PASTE_SETTLE_MS);

    if submit_key == platform::macos::NativeSubmitKey::None {
        emit_autosend_diagnostic("sent-without-submit", target, None, None);
        return AutosendOutcome::pasted_only();
    }

    let before_submit = frontmost_reader();
    if !captured_target_matches_frontmost(target, before_submit.as_ref()) {
        emit_autosend_diagnostic(
            "before-submit-mismatch",
            target,
            before_submit.as_ref(),
            None,
        );
        return AutosendOutcome::return_event_failed(
            "Target app changed after paste; submit was skipped.".to_string(),
        );
    }

    if let Err(error) = submit_sender(submit_key) {
        return AutosendOutcome::return_event_failed(format!(
            "Focus-preserving submit failed: {}",
            error
        ));
    }

    emit_autosend_diagnostic("sent", target, before_submit.as_ref(), None);
    AutosendOutcome::sent()
}

fn paste_prompt_and_submit_to_session_target_with_senders<A, C>(
    body: &str,
    state: &PromptPickSessionState,
    recent_state: Option<&LastInputTargetState>,
    submit_key: platform::macos::NativeSubmitKey,
    app_sender: A,
    copy_sender: C,
) -> Result<AutosendOutcome, String>
where
    A: FnOnce(
        &str,
        &str,
        Option<TargetClickPoint>,
        platform::macos::NativeSubmitKey,
    ) -> AutosendOutcome,
    C: FnOnce(&str) -> Result<(), String>,
{
    let Some(target) = prompt_pick_target_or_recent(state, recent_state) else {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "No prompt pick target app was recorded for autosend.",
        ));
    };

    if target
        .identity
        .as_ref()
        .is_some_and(|identity| !captured_target_identity_is_current(identity))
    {
        return Ok(stale_target_outcome());
    }

    if is_unsafe_autosend_target(&target.app) {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "Target app is not safe for autosend.",
        ));
    }

    if !allows_app_only_autosend(&target.app) {
        return Ok(copy_without_sending(
            body,
            copy_sender,
            "Target app is not safe for app-only autosend.",
        ));
    }

    Ok(app_sender(
        body,
        &target.app.bundle_id,
        target.click_point,
        submit_key,
    ))
}

fn paste_prompt_sequence_and_submit_to_session_target_with_senders<A, C, S>(
    bodies: &[String],
    interval_ms: u64,
    state: &PromptPickSessionState,
    recent_state: Option<&LastInputTargetState>,
    submit_key: platform::macos::NativeSubmitKey,
    mut app_sender: A,
    copy_sender: C,
    mut sleeper: S,
) -> Result<AutosendSequenceOutcome, String>
where
    A: FnMut(
        &str,
        &str,
        Option<TargetClickPoint>,
        platform::macos::NativeSubmitKey,
    ) -> AutosendOutcome,
    C: FnOnce(&str) -> Result<(), String>,
    S: FnMut(u64),
{
    let clean_bodies: Vec<String> = bodies
        .iter()
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .collect();
    let Some(first_body) = clean_bodies.first() else {
        return Ok(AutosendSequenceOutcome::from_failure(
            AutosendOutcome::copy_failed("Prompt group is empty.".to_string()),
            0,
            1,
        ));
    };

    let Some(target) = prompt_pick_target_or_recent(state, recent_state) else {
        let outcome = copy_without_sending(
            first_body,
            copy_sender,
            "No prompt pick target app was recorded for autosend.",
        );
        return Ok(AutosendSequenceOutcome::from_failure(outcome, 0, 1));
    };

    if target
        .identity
        .as_ref()
        .is_some_and(|identity| !captured_target_identity_is_current(identity))
    {
        return Ok(AutosendSequenceOutcome::from_failure(
            stale_target_outcome(),
            0,
            0,
        ));
    }

    if is_unsafe_autosend_target(&target.app) || !allows_app_only_autosend(&target.app) {
        let outcome = copy_without_sending(
            first_body,
            copy_sender,
            "Target app is not safe for app-only autosend.",
        );
        return Ok(AutosendSequenceOutcome::from_failure(outcome, 0, 1));
    }

    if submit_key == platform::macos::NativeSubmitKey::None {
        let joined = clean_bodies.join("\n\n");
        let outcome = app_sender(
            &joined,
            &target.app.bundle_id,
            target.click_point,
            submit_key,
        );
        if outcome.completion == Some(platform::AutosendCompletion::PastedOnly) {
            return Ok(AutosendSequenceOutcome {
                copied: true,
                sent: false,
                sent_count: 0,
                processed_count: clean_bodies.len(),
                completion: outcome.completion,
                failed_index: None,
                error: None,
                reason: None,
            });
        }
        return Ok(AutosendSequenceOutcome::from_failure(outcome, 0, 1));
    }

    let delay_ms = clamp_sequence_interval_ms(interval_ms);
    for (index, body) in clean_bodies.iter().enumerate() {
        let outcome = app_sender(body, &target.app.bundle_id, target.click_point, submit_key);
        if !outcome.sent {
            return Ok(AutosendSequenceOutcome::from_failure(
                outcome,
                index,
                index + 1,
            ));
        }
        if index + 1 < clean_bodies.len() {
            sleeper(delay_ms);
        }
    }

    Ok(AutosendSequenceOutcome::sent_all(clean_bodies.len()))
}

fn copy_without_sending<C>(body: &str, copy_sender: C, message: &str) -> AutosendOutcome
where
    C: FnOnce(&str) -> Result<(), String>,
{
    match copy_sender(body) {
        Ok(()) => AutosendOutcome::copied_without_send(message.to_string()),
        Err(error) => AutosendOutcome::copy_failed(error),
    }
}

#[cfg(test)]
fn last_target_bundle_id(state: &LastInputTargetState) -> Result<String, String> {
    let Some(target) = state.get() else {
        return Err("Click into a text field first, then choose a prompt.".to_string());
    };
    Ok(target.app.bundle_id)
}

#[tauri::command]
fn open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        activate_main_window(&window)?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    app.emit("open-manager-window", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        activate_main_window(&window)?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    app.emit("open-settings-window", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit_prompt_picker(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_menu_language(app: tauri::AppHandle, language: String) -> Result<(), String> {
    let menu = build_menu_bar_menu(&app, &language)?;
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Err(format!("{PRODUCT_NAME} tray icon is not available."));
    };
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LastInputTarget {
    pub app: FrontmostApp,
    pub pid: Option<u32>,
    pub observed_at_ms: u128,
    pub click_point: Option<TargetClickPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct TargetClickPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct TargetApplicationIdentity {
    bundle_id: String,
    main_pid: u32,
    launch_identity: platform::ProcessLaunchIdentity,
}

type TargetWindowIdentity = platform::TargetWindowIdentity;

#[derive(Clone, Debug, PartialEq)]
struct CapturedTargetIdentity {
    application: TargetApplicationIdentity,
    window: Option<TargetWindowIdentity>,
    page_url: Option<String>,
}

#[derive(Default)]
struct LastInputTargetInner {
    target: Option<LastInputTarget>,
    identity: Option<CapturedTargetIdentity>,
}

#[derive(Clone, Default)]
pub struct LastInputTargetState(std::sync::Arc<std::sync::Mutex<LastInputTargetInner>>);

impl LastInputTargetState {
    pub fn set(&self, target: LastInputTarget) {
        let mut state = self.0.lock().expect("last input target lock poisoned");
        state.target = Some(target);
        state.identity = None;
    }

    fn set_captured(&self, target: LastInputTarget, identity: CapturedTargetIdentity) {
        let mut state = self.0.lock().expect("last input target lock poisoned");
        state.target = Some(target);
        state.identity = Some(identity);
    }

    pub fn clear(&self) {
        let mut state = self.0.lock().expect("last input target lock poisoned");
        state.target = None;
        state.identity = None;
    }

    pub fn get(&self) -> Option<LastInputTarget> {
        self.0
            .lock()
            .expect("last input target lock poisoned")
            .target
            .clone()
    }

    fn captured_identity(&self) -> Option<CapturedTargetIdentity> {
        self.0
            .lock()
            .expect("last input target lock poisoned")
            .identity
            .clone()
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PromptPickSessionTarget {
    pub app: FrontmostApp,
    pub pid: Option<u32>,
    pub observed_at_ms: u128,
    pub click_point: Option<TargetClickPoint>,
}

#[derive(Default)]
struct PromptPickSessionInner {
    active_session_id: u64,
    target: Option<PromptPickSessionTarget>,
    identity: Option<CapturedTargetIdentity>,
    capture_pending: bool,
    consumed: bool,
}

#[derive(Default)]
struct PromptPickSessionShared {
    inner: std::sync::Mutex<PromptPickSessionInner>,
    capture_finished: std::sync::Condvar,
}

#[derive(Clone, Default)]
pub struct PromptPickSessionState(std::sync::Arc<PromptPickSessionShared>);

impl PromptPickSessionState {
    pub fn begin(&self, session_id: u64) {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        state.active_session_id = session_id;
        state.target = None;
        state.identity = None;
        state.capture_pending = false;
        state.consumed = false;
        drop(state);
        self.0.capture_finished.notify_all();
    }

    pub fn begin_if_new(&self, session_id: u64) {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id == session_id {
            return;
        }
        state.active_session_id = session_id;
        state.target = None;
        state.identity = None;
        state.capture_pending = false;
        state.consumed = false;
        drop(state);
        self.0.capture_finished.notify_all();
    }

    pub fn set(&self, target: PromptPickSessionTarget) {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.consumed {
            return;
        }
        state.target = Some(target);
        state.identity = None;
    }

    pub fn set_if_current(&self, session_id: u64, target: PromptPickSessionTarget) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        state.target = Some(target);
        state.identity = None;
        true
    }

    fn set_captured_if_current(
        &self,
        session_id: u64,
        target: PromptPickSessionTarget,
        identity: CapturedTargetIdentity,
    ) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        if let Some(frozen) = state.target.as_mut() {
            if frozen.app.bundle_id != target.app.bundle_id
                || matches!((frozen.pid, target.pid), (Some(left), Some(right)) if left != right)
            {
                return false;
            }
            if frozen.pid.is_none() {
                frozen.pid = target.pid;
            }
            if target.click_point.is_some() {
                frozen.click_point = target.click_point;
            }
            frozen.observed_at_ms = target.observed_at_ms;
        }
        if let Some(frozen_identity) = state.identity.as_mut() {
            if frozen_identity.application != identity.application {
                return false;
            }
            if frozen_identity.window.is_none() {
                frozen_identity.window = identity.window;
            }
            return true;
        }
        state.target = Some(target);
        state.identity = Some(identity);
        true
    }

    pub fn set_if_current_and_empty(
        &self,
        session_id: u64,
        target: PromptPickSessionTarget,
    ) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed || state.target.is_some() {
            return false;
        }
        state.target = Some(target);
        true
    }

    pub fn enrich_if_current_matching(
        &self,
        session_id: u64,
        candidate: PromptPickSessionTarget,
    ) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        let Some(target) = state.target.as_mut() else {
            return false;
        };
        if target.app.bundle_id != candidate.app.bundle_id {
            return false;
        }
        if matches!((target.pid, candidate.pid), (Some(left), Some(right)) if left != right) {
            return false;
        }

        if target.pid.is_none() {
            target.pid = candidate.pid;
        }
        if candidate.click_point.is_some() {
            target.click_point = candidate.click_point;
        }
        target.observed_at_ms = candidate.observed_at_ms;
        true
    }

    pub fn set_page_url_if_current(
        &self,
        session_id: u64,
        bundle_id: &str,
        page_url: String,
    ) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        let Some(identity) = state.identity.as_mut() else {
            return false;
        };
        if identity.application.bundle_id != bundle_id {
            return false;
        }
        identity.page_url = Some(page_url);
        true
    }

    /// Fill a missing window identity for the current session. Parallel to
    /// `set_page_url_if_current`. Does NOT overwrite an already-captured window.
    pub fn set_window_if_current(&self, session_id: u64, window: TargetWindowIdentity) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        let Some(identity) = state.identity.as_mut() else {
            return false;
        };
        if identity.window.is_none() {
            identity.window = Some(window);
        }
        true
    }

    /// True when `session_id` is no longer the active, unconsumed session.
    /// Used by the background refresh loop to stop as soon as the user picks.
    pub fn is_consumed(&self, session_id: u64) -> bool {
        let state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        state.active_session_id != session_id || state.consumed
    }

    /// True when the captured identity has everything needed to send accurately:
    /// window present, and (page_url present OR the target is not a browser).
    /// Returns `true` for a stale/consumed session so the loop exits promptly.
    pub fn target_capture_complete(&self, session_id: u64) -> bool {
        let state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return true;
        }
        let Some(identity) = &state.identity else {
            return false;
        };
        let window_ready = identity.window.is_some();
        let page_url_ready = identity.page_url.is_some()
            || !platform::macos::target_bundle_is_supported_browser(
                &identity.application.bundle_id,
            );
        window_ready && page_url_ready
    }

    pub fn clear(&self) {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        state.target = None;
        state.identity = None;
        state.capture_pending = false;
        state.consumed = true;
        drop(state);
        self.0.capture_finished.notify_all();
    }

    pub fn clear_if_current(&self, session_id: u64) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id {
            return false;
        }
        state.target = None;
        state.identity = None;
        state.capture_pending = false;
        state.consumed = true;
        drop(state);
        self.0.capture_finished.notify_all();
        true
    }

    pub fn mark_capture_pending(&self, session_id: u64) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        state.capture_pending = true;
        true
    }

    pub fn finish_capture(&self, session_id: u64) -> bool {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        if state.active_session_id != session_id || state.consumed {
            return false;
        }
        state.capture_pending = false;
        drop(state);
        self.0.capture_finished.notify_all();
        true
    }

    pub fn wait_for_capture(&self, timeout: std::time::Duration) -> bool {
        let state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        let session_id = state.active_session_id;
        let (state, _) = self
            .0
            .capture_finished
            .wait_timeout_while(state, timeout, |state| {
                state.active_session_id == session_id && !state.consumed && state.capture_pending
            })
            .expect("prompt pick session lock poisoned while waiting for capture");
        state.active_session_id != session_id || state.consumed || !state.capture_pending
    }

    pub fn get(&self) -> Option<PromptPickSessionTarget> {
        self.0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned")
            .target
            .clone()
    }

    pub fn take(&self) -> Option<PromptPickSessionTarget> {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        state.identity = None;
        state.capture_pending = false;
        state.consumed = true;
        self.0.capture_finished.notify_all();
        state.target.take()
    }

    fn take_captured(&self) -> Option<(PromptPickSessionTarget, Option<CapturedTargetIdentity>)> {
        let mut state = self
            .0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned");
        let target = state.target.take()?;
        state.capture_pending = false;
        state.consumed = true;
        self.0.capture_finished.notify_all();
        Some((target, state.identity.take()))
    }

    fn captured_identity(&self) -> Option<CapturedTargetIdentity> {
        self.0
            .inner
            .lock()
            .expect("prompt pick session lock poisoned")
            .identity
            .clone()
    }
}

pub(crate) struct PromptButtonVisibilityState {
    desired_visible: std::sync::atomic::AtomicBool,
    generation: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PromptButtonRendererAction {
    Ignore,
    HideCurrent,
    ShowCurrent,
}

impl PromptButtonRendererAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::HideCurrent => "hideCurrent",
            Self::ShowCurrent => "showCurrent",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PromptButtonRendererSnapshot {
    instance_id: u64,
    transition_id: u64,
    ready: bool,
    visibility_generation: u64,
    action: PromptButtonRendererAction,
}

#[derive(Default)]
struct PromptButtonRendererInner {
    next_instance_id: u64,
    current_instance_id: u64,
    last_transition_id: u64,
    ready: bool,
    resume_requested: bool,
}

#[derive(Default)]
pub(crate) struct PromptButtonRendererState {
    inner: std::sync::Mutex<PromptButtonRendererInner>,
}

#[derive(Default)]
pub(crate) struct PromptButtonRecoveryUrlState {
    url: std::sync::Mutex<Option<tauri::Url>>,
}

impl PromptButtonRecoveryUrlState {
    pub(crate) fn store(&self, url: tauri::Url) {
        *self.url.lock().expect("recovery URL state lock poisoned") = Some(url);
    }

    fn get(&self) -> Option<tauri::Url> {
        self.url
            .lock()
            .expect("recovery URL state lock poisoned")
            .clone()
    }
}

impl PromptButtonRendererState {
    pub(crate) fn allocate_instance(&self) -> u64 {
        let mut inner = self.inner.lock().expect("renderer state lock poisoned");
        inner.next_instance_id = inner.next_instance_id.wrapping_add(1).max(1);
        inner.current_instance_id = inner.next_instance_id;
        inner.last_transition_id = 0;
        inner.ready = false;
        inner.resume_requested = false;
        inner.current_instance_id
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.inner
            .lock()
            .expect("renderer state lock poisoned")
            .ready
    }

    pub(crate) fn request_resume_once(&self) -> bool {
        let mut inner = self.inner.lock().expect("renderer state lock poisoned");
        if inner.ready || inner.resume_requested || inner.current_instance_id == 0 {
            return false;
        }
        inner.resume_requested = true;
        true
    }

    fn current(&self) -> (u64, u64, bool) {
        let inner = self.inner.lock().expect("renderer state lock poisoned");
        (
            inner.current_instance_id,
            inner.last_transition_id,
            inner.ready,
        )
    }

    fn instance_is_current_unready(&self, instance_id: u64) -> bool {
        let inner = self.inner.lock().expect("renderer state lock poisoned");
        inner.current_instance_id == instance_id && !inner.ready
    }

    fn accept(
        &self,
        instance_id: u64,
        transition_id: u64,
        ready: bool,
        visibility: &PromptButtonVisibilityState,
    ) -> Option<PromptButtonRendererSnapshot> {
        let mut inner = self.inner.lock().expect("renderer state lock poisoned");
        if instance_id == 0
            || instance_id != inner.current_instance_id
            || transition_id <= inner.last_transition_id
        {
            return None;
        }
        inner.last_transition_id = transition_id;
        inner.ready = ready;
        inner.resume_requested = false;
        let visibility_generation = visibility.generation();
        let action = if !ready {
            PromptButtonRendererAction::HideCurrent
        } else if visibility.desired_visible() {
            PromptButtonRendererAction::ShowCurrent
        } else {
            PromptButtonRendererAction::Ignore
        };
        Some(PromptButtonRendererSnapshot {
            instance_id,
            transition_id,
            ready,
            visibility_generation,
            action,
        })
    }

    fn action_is_current(
        &self,
        snapshot: PromptButtonRendererSnapshot,
        visibility: &PromptButtonVisibilityState,
    ) -> bool {
        let inner = self.inner.lock().expect("renderer state lock poisoned");
        let transition_is_current = inner.current_instance_id == snapshot.instance_id
            && inner.last_transition_id == snapshot.transition_id
            && inner.ready == snapshot.ready;
        if !transition_is_current {
            return false;
        }
        match snapshot.action {
            PromptButtonRendererAction::Ignore => false,
            PromptButtonRendererAction::HideCurrent => !inner.ready,
            PromptButtonRendererAction::ShowCurrent => {
                inner.ready && visibility.may_show(snapshot.visibility_generation)
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptButtonRendererReadyOutcome {
    accepted: bool,
    current_instance_id: u64,
    transition_id: u64,
    ready: bool,
    action: String,
    applied: bool,
}

#[tauri::command]
fn set_prompt_button_renderer_ready(
    renderer_instance_id: u64,
    transition_id: u64,
    ready: bool,
    app: tauri::AppHandle,
    renderer_state: tauri::State<PromptButtonRendererState>,
    visibility_state: tauri::State<PromptButtonVisibilityState>,
    visual_health: tauri::State<visual_liveness::PromptButtonVisualHealthState>,
) -> PromptButtonRendererReadyOutcome {
    let Some(snapshot) = renderer_state.accept(
        renderer_instance_id,
        transition_id,
        ready,
        visibility_state.inner(),
    ) else {
        let (current_instance_id, current_transition_id, current_ready) = renderer_state.current();
        return PromptButtonRendererReadyOutcome {
            accepted: false,
            current_instance_id,
            transition_id: current_transition_id,
            ready: current_ready,
            action: "ignore".to_string(),
            applied: false,
        };
    };

    visual_health.note_renderer_ready(renderer_instance_id, ready);
    let applied = renderer_state.action_is_current(snapshot, visibility_state.inner())
        && match snapshot.action {
            PromptButtonRendererAction::Ignore => false,
            PromptButtonRendererAction::HideCurrent => hide_prompt_button(app.clone()).is_ok(),
            PromptButtonRendererAction::ShowCurrent => {
                visual_health.may_present(renderer_instance_id)
                    && crate::windows::show_ready_prompt_button_window(&app).is_ok()
            }
        };

    PromptButtonRendererReadyOutcome {
        accepted: true,
        current_instance_id: snapshot.instance_id,
        transition_id: snapshot.transition_id,
        ready: snapshot.ready,
        action: snapshot.action.as_str().to_string(),
        applied,
    }
}

#[tauri::command]
fn ack_prompt_button_visual_probe(
    renderer_instance_id: u64,
    recovery_generation: u64,
    nonce: u64,
    visible_alpha_pixels: u64,
    app: tauri::AppHandle,
    visual_health: tauri::State<visual_liveness::PromptButtonVisualHealthState>,
) -> visual_liveness::VisualProbeAck {
    let acknowledgement = visual_health.accept_probe_ack(
        renderer_instance_id,
        recovery_generation,
        nonce,
        visible_alpha_pixels,
    );
    record_visual_liveness_event(
        &app,
        if acknowledgement.visually_alive {
            "probe_alive"
        } else {
            "probe_blank"
        },
    );
    if acknowledgement.accepted && acknowledgement.visually_alive {
        #[cfg(target_os = "macos")]
        {
            if native_visual_snapshot_spike_enabled() {
                if let Some(window) = app.get_webview_window(crate::windows::BUTTON_WINDOW_LABEL) {
                    let snapshot_app = app.clone();
                    let request = crate::macos_panels::request_calico_webview_snapshot(
                        &window,
                        move |result| {
                            let pixels = result.unwrap_or(0);
                            let accepted = snapshot_app
                                .state::<visual_liveness::PromptButtonVisualHealthState>()
                                .accept_snapshot_result(
                                    renderer_instance_id,
                                    recovery_generation,
                                    nonce,
                                    pixels,
                                );
                            if accepted {
                                record_visual_liveness_event(
                                    &snapshot_app,
                                    if pixels >= visual_liveness::MIN_VISIBLE_ALPHA_PIXELS {
                                        "snapshot_alive"
                                    } else {
                                        "snapshot_blank"
                                    },
                                );
                            }
                        },
                    );
                    if request.is_err() {
                        record_visual_liveness_event(&app, "snapshot_request_failed");
                    }
                }
            } else {
                visual_health.accept_snapshot_result(
                    renderer_instance_id,
                    recovery_generation,
                    nonce,
                    visible_alpha_pixels,
                );
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            visual_health.accept_snapshot_result(
                renderer_instance_id,
                recovery_generation,
                nonce,
                visible_alpha_pixels,
            );
        }
    }
    acknowledgement
}

#[cfg(target_os = "macos")]
fn native_visual_snapshot_spike_enabled() -> bool {
    std::env::var("SLEEPY_CAT_VISUAL_SNAPSHOT_SPIKE").as_deref() == Ok("1")
}

fn record_visual_liveness_event(app: &tauri::AppHandle, event: &'static str) {
    let Some(health) = app.try_state::<visual_liveness::PromptButtonVisualHealthState>() else {
        return;
    };
    let (instance, generation) = health.diagnostic_identity();
    if let Some(logger) = app.try_state::<visual_liveness_log::VisualLivenessLogger>() {
        logger.record(event, health.stage().as_str(), instance, generation);
    }
}

#[tauri::command]
fn set_prompt_button_interaction(
    active: bool,
    drag: bool,
    visual_health: tauri::State<visual_liveness::PromptButtonVisualHealthState>,
) {
    visual_health.set_interaction_active(active, drag);
}

fn is_prompt_button_webview(label: &str) -> bool {
    label == crate::windows::BUTTON_WINDOW_LABEL
}

fn prompt_button_recovery_url(
    current_url: Result<tauri::Url, String>,
    stored_url: Option<tauri::Url>,
    instance_id: u64,
) -> Result<tauri::Url, String> {
    let mut url = current_url.or_else(|_| {
        stored_url.ok_or_else(|| "Prompt button recovery URL is missing.".to_string())
    })?;
    url.set_path("/overlay.html");
    url.set_query(Some(&format!("rendererInstanceId={instance_id}")));
    Ok(url)
}

fn perform_prompt_button_webview_recovery<Hide, CurrentUrl, Navigate>(
    stored_url: Option<tauri::Url>,
    instance_id: u64,
    hide: Hide,
    current_url: CurrentUrl,
    navigate: Navigate,
) -> Result<tauri::Url, String>
where
    Hide: FnOnce() -> Result<(), String>,
    CurrentUrl: FnOnce() -> Result<tauri::Url, String>,
    Navigate: FnOnce(tauri::Url) -> Result<(), String>,
{
    hide().map_err(|error| format!("Failed to hide terminated prompt button WebView: {error}"))?;
    let recovery_url = prompt_button_recovery_url(current_url(), stored_url, instance_id)?;
    navigate(recovery_url.clone())
        .map_err(|error| format!("Failed to navigate terminated prompt button WebView: {error}"))?;
    Ok(recovery_url)
}

fn recover_prompt_button_webview_once<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instance_id: u64,
) -> Result<(), String> {
    let renderer_state = app.state::<PromptButtonRendererState>();
    if !renderer_state.instance_is_current_unready(instance_id) {
        return Err(format!(
            "Prompt button renderer instance {instance_id} is no longer current."
        ));
    }
    let window = app
        .get_webview_window(crate::windows::BUTTON_WINDOW_LABEL)
        .ok_or_else(|| "Prompt button window is missing during WebContent recovery.".to_string())?;
    let stored_url = app.state::<PromptButtonRecoveryUrlState>().get();
    let recovery_url = perform_prompt_button_webview_recovery(
        stored_url,
        instance_id,
        || crate::windows::hide_prompt_button_pair(app),
        || window.url().map_err(|error| error.to_string()),
        |url| window.navigate(url).map_err(|error| error.to_string()),
    )?;
    app.state::<PromptButtonRecoveryUrlState>()
        .store(recovery_url);
    Ok(())
}

fn execute_prompt_button_visual_action(
    app: &tauri::AppHandle,
    action: visual_liveness::VisualMonitorAction,
) {
    match action {
        visual_liveness::VisualMonitorAction::None => {}
        visual_liveness::VisualMonitorAction::Probe(probe) => {
            record_visual_liveness_event(app, "probe_requested");
            if app
                .emit_to(
                    crate::windows::BUTTON_WINDOW_LABEL,
                    "prompt-button-visual-probe",
                    probe,
                )
                .is_err()
            {
                app.state::<visual_liveness::PromptButtonVisualHealthState>()
                    .mark_unresponsive();
            }
        }
        visual_liveness::VisualMonitorAction::Reload => {
            record_visual_liveness_event(app, "reload_requested");
            let renderer_state = app.state::<PromptButtonRendererState>();
            let instance_id = renderer_state.allocate_instance();
            app.state::<visual_liveness::PromptButtonVisualHealthState>()
                .begin_reload(instance_id);
            let recovery_app = app.clone();
            let queue_result = app.run_on_main_thread(move || {
                if let Err(error) = recover_prompt_button_webview_once(&recovery_app, instance_id) {
                    eprintln!("Prompt button WebContent reload failed: {error}");
                    recovery_app
                        .state::<visual_liveness::PromptButtonVisualHealthState>()
                        .record_recovery_failure();
                }
            });
            if let Err(error) = queue_result {
                eprintln!("Failed to queue prompt button WebContent reload: {error}");
                app.state::<visual_liveness::PromptButtonVisualHealthState>()
                    .record_recovery_failure();
            }
        }
        visual_liveness::VisualMonitorAction::Rebuild => {
            record_visual_liveness_event(app, "rebuild_requested");
            let visual_health = app.state::<visual_liveness::PromptButtonVisualHealthState>();
            let generation = visual_health.begin_rebuild_attempt();
            let requested_visibility_generation =
                app.state::<PromptButtonVisibilityState>().generation();
            let position = visual_health.latest_position().unwrap_or_else(|| {
                let settings = app.state::<SettingsFileState>();
                startup_prompt_button_position(settings.inner())
            });
            let rebuild_app = app.clone();
            let queue_result = app.run_on_main_thread(move || {
                let visibility = rebuild_app.state::<PromptButtonVisibilityState>();
                let health = rebuild_app.state::<visual_liveness::PromptButtonVisualHealthState>();
                if !visibility.may_show(requested_visibility_generation)
                    || !health.generation_is_current(generation)
                {
                    return;
                }
                if let Err(error) = crate::windows::destroy_prompt_button_pair(&rebuild_app) {
                    eprintln!("Failed to destroy prompt button pair: {error}");
                    health.record_recovery_failure();
                    return;
                }
                if !crate::windows::prompt_button_pair_is_absent(&rebuild_app) {
                    health.record_recovery_failure();
                    return;
                }
                if !visibility.may_show(requested_visibility_generation)
                    || !health.generation_is_current(generation)
                {
                    return;
                }
                if let Err(error) =
                    crate::windows::build_prompt_button_window(&rebuild_app, position.0, position.1)
                {
                    eprintln!("Failed to rebuild prompt button pair: {error}");
                }
            });
            if let Err(error) = queue_result {
                eprintln!("Failed to queue prompt button pair rebuild: {error}");
                app.state::<visual_liveness::PromptButtonVisualHealthState>()
                    .record_recovery_failure();
            }
        }
    }
}

fn run_prompt_button_visual_monitor_once(app: &tauri::AppHandle) {
    let visibility = app.state::<PromptButtonVisibilityState>();
    let renderer_ready = app.state::<PromptButtonRendererState>().is_ready();
    let action = app
        .state::<visual_liveness::PromptButtonVisualHealthState>()
        .plan_monitor(
            visibility.desired_visible(),
            renderer_ready,
            app.get_webview_window(crate::windows::BUTTON_WINDOW_LABEL)
                .is_some(),
            app.get_webview_window(crate::windows::BUTTON_INPUT_WINDOW_LABEL)
                .is_some(),
        );
    execute_prompt_button_visual_action(app, action);
}

fn handle_prompt_button_webcontent_termination(app: &tauri::AppHandle, label: &str) -> bool {
    if !is_prompt_button_webview(label) {
        return false;
    }
    let visual_health = app.state::<visual_liveness::PromptButtonVisualHealthState>();
    if !visual_health.accepts_termination_event() {
        return true;
    }
    visual_health.mark_unresponsive();
    record_visual_liveness_event(app, "webcontent_terminated");
    let _ = crate::windows::hide_prompt_button_pair(app);
    run_prompt_button_visual_monitor_once(app);
    true
}

#[cfg(debug_assertions)]
#[tauri::command]
fn simulate_prompt_button_webcontent_termination(app: tauri::AppHandle) -> bool {
    handle_prompt_button_webcontent_termination(&app, crate::windows::BUTTON_WINDOW_LABEL)
}

impl PromptButtonVisibilityState {
    fn new(visible: bool) -> Self {
        Self {
            desired_visible: std::sync::atomic::AtomicBool::new(visible),
            generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn from_settings(settings: &serde_json::Value) -> Self {
        let visible = settings
            .pointer("/floatingButton/visible")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        Self::new(visible)
    }

    pub(crate) fn desired_visible(&self) -> bool {
        self.desired_visible
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    fn set(&self, visible: bool) -> u64 {
        self.desired_visible
            .store(visible, std::sync::atomic::Ordering::Release);
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1
    }

    fn may_show(&self, requested_generation: u64) -> bool {
        self.desired_visible() && self.generation() == requested_generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptButtonEnsureAction {
    None,
    ShowExisting,
    BuildMissing,
}

fn prompt_button_ensure_action(
    expected_visible: bool,
    window_present: bool,
    window_visible: bool,
    renderer_ready: bool,
) -> PromptButtonEnsureAction {
    if !expected_visible || (window_present && (!renderer_ready || window_visible)) {
        PromptButtonEnsureAction::None
    } else if window_present {
        PromptButtonEnsureAction::ShowExisting
    } else {
        PromptButtonEnsureAction::BuildMissing
    }
}

struct SettingsFileState {
    path: std::path::PathBuf,
    io_lock: std::sync::Mutex<()>,
}

impl SettingsFileState {
    fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            io_lock: std::sync::Mutex::new(()),
        }
    }

    fn read_text(&self) -> Result<Option<String>, String> {
        let _guard = self.io_lock.lock().map_err(|e| e.to_string())?;
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn read_value(&self) -> serde_json::Value {
        let Ok(_guard) = self.io_lock.lock() else {
            return default_settings_value();
        };
        self.read_value_unlocked()
    }

    fn read_value_unlocked(&self) -> serde_json::Value {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_else(default_settings_value)
    }

    fn write_value_unlocked(&self, settings: &serde_json::Value) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let contents = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, contents).map_err(|e| e.to_string())
    }

    fn write_frontend_text(
        &self,
        value: &str,
        visibility_state: &PromptButtonVisibilityState,
    ) -> Result<(), String> {
        let mut settings: serde_json::Value =
            serde_json::from_str(value).map_err(|e| format!("Invalid settings JSON: {e}"))?;
        if !settings.is_object() {
            return Err("Settings JSON must be an object.".to_string());
        }
        if !settings
            .get("floatingButton")
            .is_some_and(serde_json::Value::is_object)
        {
            settings["floatingButton"] = serde_json::json!({});
        }
        let _guard = self.io_lock.lock().map_err(|e| e.to_string())?;
        settings["floatingButton"]["visible"] =
            serde_json::Value::Bool(visibility_state.desired_visible());
        self.write_value_unlocked(&settings)
    }

    fn patch_bool(&self, pointer: &[&str], value: bool) -> Result<(), String> {
        let _guard = self.io_lock.lock().map_err(|e| e.to_string())?;
        let mut settings = self.read_value_unlocked();
        if !settings.is_object() {
            settings = default_settings_value();
        }
        let (section, key) = (pointer[0], pointer[1]);
        if !settings
            .get(section)
            .is_some_and(serde_json::Value::is_object)
        {
            settings[section] = serde_json::json!({});
        }
        settings[section][key] = serde_json::Value::Bool(value);
        self.write_value_unlocked(&settings)
    }
}

fn record_prompt_pick_session_target_if_valid(
    state: &PromptPickSessionState,
    target: PromptPickSessionTarget,
    identity: CapturedTargetIdentity,
    session_id: u64,
) -> Option<FrontmostApp> {
    if !is_usable_autosend_app(&target.app) {
        return None;
    }

    let app = target.app.clone();
    state
        .set_captured_if_current(session_id, target, identity)
        .then_some(app)
}

#[cfg(test)]
fn enrich_prompt_pick_session_target_if_matching(
    state: &PromptPickSessionState,
    target: PromptPickSessionTarget,
    session_id: u64,
) -> Option<FrontmostApp> {
    if !is_usable_autosend_app(&target.app) {
        return None;
    }
    let app = target.app.clone();
    state
        .enrich_if_current_matching(session_id, target)
        .then_some(app)
}

fn captured_identity_for_target(
    target: &PromptPickSessionTarget,
    recent_identity: Option<&CapturedTargetIdentity>,
) -> Option<CapturedTargetIdentity> {
    let pid = target.pid?;
    if let Some(identity) = recent_identity.filter(|identity| {
        identity.application.bundle_id == target.app.bundle_id
            && identity.application.main_pid == pid
            && captured_target_identity_is_current(identity)
    }) {
        return Some(identity.clone());
    }

    Some(fresh_captured_target_identity(
        &target.app.bundle_id,
        pid,
        platform::macos::process_launch_identity(pid)?,
        platform::macos::current_target_window_identity(pid),
    ))
}

fn fresh_captured_target_identity(
    bundle_id: &str,
    pid: u32,
    launch_identity: platform::ProcessLaunchIdentity,
    window: Option<TargetWindowIdentity>,
) -> CapturedTargetIdentity {
    CapturedTargetIdentity {
        application: TargetApplicationIdentity {
            bundle_id: bundle_id.to_string(),
            main_pid: pid,
            launch_identity,
        },
        window,
        page_url: None,
    }
}

fn target_application_identity_is_current(identity: &TargetApplicationIdentity) -> bool {
    target_application_identity_matches(
        identity,
        platform::macos::process_launch_identity(identity.main_pid),
    )
}

fn target_application_identity_matches(
    identity: &TargetApplicationIdentity,
    current: Option<platform::ProcessLaunchIdentity>,
) -> bool {
    match current {
        Some(current) => current == identity.launch_identity,
        None => !cfg!(target_os = "macos"),
    }
}

fn bundle_requires_exact_window(bundle_id: &str) -> bool {
    matches!(
        bundle_id,
        "com.anthropic.claudefordesktop" | "com.tencent.xinWeChat"
    )
}

fn captured_target_identity_is_current(identity: &CapturedTargetIdentity) -> bool {
    target_application_identity_is_current(&identity.application)
}

fn stale_target_outcome() -> AutosendOutcome {
    AutosendOutcome {
        copied: false,
        sent: false,
        completion: None,
        error: Some("The captured target app or window is no longer available.".to_string()),
        reason: Some(platform::macos::AutosendFailureReason::NoSafeTarget),
    }
}

pub(crate) fn freeze_prompt_pick_session_target(
    state: &PromptPickSessionState,
    frontmost: Option<FrontmostAppWithPid>,
    recent_target: Option<LastInputTarget>,
    session_id: u64,
) -> Option<FrontmostApp> {
    let frontmost = frontmost?;
    if frontmost.pid == Some(std::process::id()) || !is_usable_autosend_app(&frontmost.app) {
        return None;
    }

    let click_point = recent_target
        .filter(is_recent_prompt_target)
        .filter(|target| target.app.bundle_id == frontmost.app.bundle_id)
        .and_then(|target| target.click_point);
    let app = frontmost.app.clone();
    let session_target = PromptPickSessionTarget {
        app: frontmost.app,
        pid: frontmost.pid,
        observed_at_ms: now_ms(),
        click_point,
    };
    let captured_identity = session_target.pid.and_then(|pid| {
        Some(fresh_captured_target_identity(
            &session_target.app.bundle_id,
            pid,
            platform::macos::process_launch_identity(pid)?,
            None,
        ))
    });
    let stored = captured_identity
        .map(|identity| state.set_captured_if_current(session_id, session_target.clone(), identity))
        .unwrap_or_else(|| state.set_if_current_and_empty(session_id, session_target));

    stored.then_some(app)
}

fn prompt_pick_session_target(
    frontmost: Option<FrontmostAppWithPid>,
    recent_target: Option<LastInputTarget>,
) -> Option<PromptPickSessionTarget> {
    let frontmost = frontmost?;
    let is_prompt_picker =
        frontmost.pid == Some(std::process::id()) || is_prompt_picker_app(&frontmost.app);

    if !is_prompt_picker && is_usable_autosend_app(&frontmost.app) {
        let click_point = recent_target
            .as_ref()
            .filter(|target| target.app.bundle_id == frontmost.app.bundle_id)
            .and_then(|target| target.click_point);
        return Some(PromptPickSessionTarget {
            app: frontmost.app,
            pid: frontmost.pid,
            observed_at_ms: now_ms(),
            click_point,
        });
    }

    if !is_prompt_picker {
        return None;
    }

    recent_target
        .filter(is_recent_prompt_target)
        .filter(|target| is_usable_autosend_app(&target.app))
        .map(|target| PromptPickSessionTarget {
            app: target.app,
            pid: target.pid,
            observed_at_ms: now_ms(),
            click_point: target.click_point,
        })
}

fn prompt_pick_frontmost_candidate(
    frontmost: Option<FrontmostAppWithPid>,
    underlying_app: Option<FrontmostAppWithPid>,
) -> Option<FrontmostAppWithPid> {
    match frontmost {
        Some(frontmost)
            if frontmost.pid == Some(std::process::id())
                || is_prompt_picker_app(&frontmost.app) =>
        {
            underlying_app.or(Some(frontmost))
        }
        Some(frontmost) => Some(frontmost),
        None => underlying_app,
    }
}

fn record_last_input_target_if_valid(state: &LastInputTargetState, target: &platform::InputTarget) {
    let Some(app) = target.app.clone() else {
        return;
    };
    if is_prompt_picker_app(&app) {
        return;
    }
    if is_unsafe_autosend_target(&app) {
        state.clear();
        return;
    }
    if !allows_app_only_autosend(&app) {
        state.clear();
        return;
    };

    let click_point = if has_focused_input_frame(&target.frame) {
        Some(target_click_point_from_tuple(target.click_point))
    } else {
        state
            .get()
            .filter(|recent| recent.app.bundle_id == app.bundle_id)
            .filter(|recent| recent.pid == Some(target.pid))
            .filter(is_recent_prompt_target)
            .and_then(|recent| recent.click_point)
    };

    let Some(launch_identity) = platform::macos::process_launch_identity(target.pid) else {
        state.clear();
        return;
    };
    let window = platform::macos::current_target_window_identity(target.pid);
    if bundle_requires_exact_window(&app.bundle_id) && window.is_none() {
        state.clear();
        return;
    }
    let identity = fresh_captured_target_identity(
        &app.bundle_id,
        target.pid,
        launch_identity,
        window.or_else(|| {
            Some(TargetWindowIdentity {
                owner_pid: target.pid,
                frame: target.window_frame.clone(),
                role: Some("AXWindow".to_string()),
                title_hash: None,
                cg_window_id: None,
            })
        }),
    );

    state.set_captured(
        LastInputTarget {
            app,
            pid: Some(target.pid),
            observed_at_ms: now_ms(),
            click_point,
        },
        identity,
    );
}

fn record_last_app_if_valid(state: &LastInputTargetState, target: FrontmostAppWithPid) {
    if target.pid == Some(std::process::id()) || is_prompt_picker_app(&target.app) {
        return;
    }
    if is_unsafe_autosend_target(&target.app) {
        state.clear();
        return;
    }
    if !allows_app_only_autosend(&target.app) {
        state.clear();
        return;
    }
    let Some(pid) = target.pid else {
        state.clear();
        return;
    };
    let Some(launch_identity) = platform::macos::process_launch_identity(pid) else {
        state.clear();
        return;
    };
    let identity = fresh_captured_target_identity(
        &target.app.bundle_id,
        pid,
        launch_identity,
        platform::macos::current_target_window_identity(pid),
    );
    state.set_captured(
        LastInputTarget {
            app: target.app,
            pid: Some(pid),
            observed_at_ms: now_ms(),
            click_point: None,
        },
        identity,
    );
}

fn captured_target_matches_frontmost(
    target: &PromptPickSessionTarget,
    frontmost: Option<&FrontmostAppWithPid>,
) -> bool {
    let Some(frontmost) = frontmost else {
        return false;
    };
    if frontmost.app.bundle_id != target.app.bundle_id {
        return false;
    }
    match (target.pid, frontmost.pid) {
        (Some(target_pid), Some(frontmost_pid)) => target_pid == frontmost_pid,
        _ => true,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetFrontmostStatus {
    Target,
    PromptPicker,
    OtherOrUnknown,
}

fn classify_target_frontmost(
    target: &PromptPickSessionTarget,
    frontmost: Option<&FrontmostAppWithPid>,
) -> TargetFrontmostStatus {
    let Some(frontmost) = frontmost else {
        return TargetFrontmostStatus::OtherOrUnknown;
    };
    if captured_target_matches_frontmost(target, Some(frontmost)) {
        return TargetFrontmostStatus::Target;
    }
    if is_prompt_picker_app(&frontmost.app) {
        return TargetFrontmostStatus::PromptPicker;
    }
    TargetFrontmostStatus::OtherOrUnknown
}

fn autosend_diagnostic_line(
    stage: &str,
    target_bundle_id: Option<&str>,
    has_click_point: bool,
    frontmost_bundle_id: Option<&str>,
    classification: Option<TargetFrontmostStatus>,
) -> String {
    format!(
        "prompt-picker-autosend stage={} target={} has_click_point={} frontmost={} classification={}",
        stage,
        target_bundle_id.unwrap_or("unknown"),
        has_click_point,
        frontmost_bundle_id.unwrap_or("unknown"),
        classification
            .map(|status| format!("{:?}", status))
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn autosend_diagnostics_enabled() -> bool {
    std::env::var("PROMPT_PICKER_FOCUS_DIAGNOSTICS").is_ok()
}

fn emit_autosend_diagnostic(
    stage: &str,
    target: &PromptPickSessionTarget,
    frontmost: Option<&FrontmostAppWithPid>,
    classification: Option<TargetFrontmostStatus>,
) {
    if !autosend_diagnostics_enabled() {
        return;
    }
    eprintln!(
        "{}",
        autosend_diagnostic_line(
            stage,
            Some(&target.app.bundle_id),
            target.click_point.is_some(),
            frontmost.map(|app| app.app.bundle_id.as_str()),
            classification,
        )
    );
}

fn is_prompt_picker_app(app: &FrontmostApp) -> bool {
    app.bundle_id == "local.promptpicker.dev"
        || app.name == PRODUCT_NAME
        || app.name == LEGACY_PRODUCT_NAME
}

fn is_usable_autosend_app(app: &FrontmostApp) -> bool {
    !is_prompt_picker_app(app) && allows_app_only_autosend(app)
}

const PROMPT_PICK_RECENT_TARGET_MAX_AGE_MS: u128 = 5_000;

fn is_recent_prompt_target(target: &LastInputTarget) -> bool {
    now_ms().saturating_sub(target.observed_at_ms) <= PROMPT_PICK_RECENT_TARGET_MAX_AGE_MS
}

fn has_focused_input_frame(frame: &CandidateInput) -> bool {
    frame.width > 1.0 && frame.height > 1.0
}

fn target_click_point_from_tuple(point: (f64, f64)) -> TargetClickPoint {
    TargetClickPoint {
        x: point.0,
        y: point.1,
    }
}

fn point_inside_candidate(point: TargetClickPoint, frame: &CandidateInput) -> bool {
    point.x >= frame.x
        && point.x <= frame.x + frame.width
        && point.y >= frame.y
        && point.y <= frame.y + frame.height
}

fn choose_recovery_click_point(
    recorded_click_point: Option<TargetClickPoint>,
    pointer_click_point: Option<TargetClickPoint>,
    target_window_frame: Option<&CandidateInput>,
    fallback_click_point: Option<TargetClickPoint>,
) -> Option<TargetClickPoint> {
    recorded_click_point
        .or_else(|| {
            let point = pointer_click_point?;
            let frame = target_window_frame?;
            point_inside_candidate(point, frame).then_some(point)
        })
        .or(fallback_click_point)
}

fn allows_app_only_autosend(app: &FrontmostApp) -> bool {
    !is_unsafe_autosend_target(app)
}

fn is_unsafe_autosend_target(app: &FrontmostApp) -> bool {
    matches!(
        app.bundle_id.as_str(),
        "com.apple.finder"
            | "com.apple.systempreferences"
            | "com.apple.SystemSettings"
            | "com.apple.dock"
            | "com.apple.controlcenter"
            | "com.apple.notificationcenterui"
    ) || matches!(
        app.name.as_str(),
        "Finder" | "Desktop" | "System Settings" | "System Preferences" | "Dock" | "Control Center"
    )
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

const TRAY_ID: &str = "prompt-picker-tray";
const TRAY_OPEN_MAIN_ID: &str = "open-main-window";
const TRAY_SHOW_BUTTON_ID: &str = "show-floating-button";
const TRAY_HIDE_BUTTON_ID: &str = "hide-floating-button";
const TRAY_OPEN_ACCESSIBILITY_ID: &str = "open-accessibility-settings";
const TRAY_OPEN_SETTINGS_ID: &str = "open-settings-window";
const TRAY_QUIT_ID: &str = "quit";
const MENUBAR_TEMPLATE_ICON: &[u8] = include_bytes!("../icons/menubar-template.rgba");
const MENUBAR_TEMPLATE_ICON_SIZE: u32 = 22;

#[derive(Debug, PartialEq, Eq)]
struct MenuLabels {
    open_main: &'static str,
    open_settings: &'static str,
    show_button: &'static str,
    hide_button: &'static str,
    open_accessibility: &'static str,
    quit: &'static str,
}

fn menu_labels_for_language(language: &str) -> MenuLabels {
    match language {
        "zh-CN" => MenuLabels {
            open_main: "管理提示词...",
            open_settings: "设置...",
            show_button: "显示 Sleepy Cat",
            hide_button: "隐藏 Sleepy Cat",
            open_accessibility: "打开辅助功能设置",
            quit: "退出 Sleepy Cat",
        },
        _ => MenuLabels {
            open_main: "Manage Prompts...",
            open_settings: "Settings...",
            show_button: "Show Sleepy Cat",
            hide_button: "Hide Sleepy Cat",
            open_accessibility: "Open Accessibility Settings",
            quit: "Quit Sleepy Cat",
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TrayMenuAction {
    OpenMainWindow,
    ShowFloatingButton,
    HideFloatingButton,
    OpenAccessibilitySettings,
    OpenSettingsWindow,
    Quit,
    Unknown,
}

fn tray_menu_action(id: &str) -> TrayMenuAction {
    match id {
        TRAY_OPEN_MAIN_ID => TrayMenuAction::OpenMainWindow,
        TRAY_SHOW_BUTTON_ID => TrayMenuAction::ShowFloatingButton,
        TRAY_HIDE_BUTTON_ID => TrayMenuAction::HideFloatingButton,
        TRAY_OPEN_ACCESSIBILITY_ID => TrayMenuAction::OpenAccessibilitySettings,
        TRAY_OPEN_SETTINGS_ID => TrayMenuAction::OpenSettingsWindow,
        TRAY_QUIT_ID => TrayMenuAction::Quit,
        _ => TrayMenuAction::Unknown,
    }
}

fn menubar_template_icon() -> Image<'static> {
    Image::new(
        MENUBAR_TEMPLATE_ICON,
        MENUBAR_TEMPLATE_ICON_SIZE,
        MENUBAR_TEMPLATE_ICON_SIZE,
    )
}

fn parse_saved_button_position(contents: &str) -> Option<(f64, f64)> {
    let settings: serde_json::Value = serde_json::from_str(contents).ok()?;
    let position = settings.pointer("/overlayPlacement/buttonPosition")?;
    let x = position.get("x")?.as_f64()?;
    let y = position.get("y")?.as_f64()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some((x, y))
}

fn default_settings_value() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "blacklistedApps": [],
        "overlayPlacement": {
            "buttonOffset": null,
            "buttonPosition": null
        },
        "floatingButton": {
            "visible": true
        },
        "promptInsertion": {
            "mode": "paste_and_submit"
        },
        "permissions": {
            "accessibilityPromptRequested": false
        },
        "language": "zh-CN"
    })
}

fn settings_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

fn settings_language(settings: &serde_json::Value) -> &str {
    match settings.get("language").and_then(serde_json::Value::as_str) {
        Some("en-US") => "en-US",
        _ => "zh-CN",
    }
}

fn accessibility_prompt_requested(settings: &serde_json::Value) -> bool {
    settings
        .pointer("/permissions/accessibilityPromptRequested")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn set_accessibility_prompt_requested(
    settings_state: &SettingsFileState,
    requested: bool,
) -> Result<(), String> {
    settings_state.patch_bool(&["permissions", "accessibilityPromptRequested"], requested)
}

#[tauri::command]
fn read_settings_text(
    settings_state: tauri::State<SettingsFileState>,
) -> Result<Option<String>, String> {
    settings_state.read_text()
}

#[tauri::command]
fn write_settings_text(
    value: String,
    settings_state: tauri::State<SettingsFileState>,
    visibility_state: tauri::State<PromptButtonVisibilityState>,
) -> Result<(), String> {
    settings_state.write_frontend_text(&value, visibility_state.inner())
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptButtonVisibilityOutcome {
    visible: bool,
    applied: bool,
    persisted: bool,
    error: Option<String>,
}

fn apply_prompt_button_visibility(
    visible: bool,
    app: &tauri::AppHandle,
    settings_state: &SettingsFileState,
    visibility_state: &PromptButtonVisibilityState,
) -> PromptButtonVisibilityOutcome {
    visibility_state.set(visible);

    if visible {
        app.state::<visual_liveness::PromptButtonVisualHealthState>()
            .interrupt_backoff();
    }

    let runtime_result = if visible {
        let position = prompt_button_position_cmd(app.clone()).ok().flatten();
        let (x, y) = position
            .map(|point| (point.x, point.y))
            .unwrap_or_else(|| startup_prompt_button_position(settings_state));
        show_prompt_button(x, y, app.clone())
    } else {
        hide_prompt_popover(app.clone()).and_then(|_| hide_prompt_button(app.clone()))
    };
    let persistence_result = settings_state.patch_bool(&["floatingButton", "visible"], visible);
    let _ = app.emit("prompt-button-visibility-changed", visible);
    if visible {
        run_prompt_button_visual_monitor_once(app);
    }

    let error = match (&runtime_result, &persistence_result) {
        (Err(runtime), Err(persist)) => Some(format!("{runtime}; {persist}")),
        (Err(runtime), Ok(())) => Some(runtime.clone()),
        (Ok(()), Err(persist)) => Some(persist.clone()),
        (Ok(()), Ok(())) => None,
    };

    PromptButtonVisibilityOutcome {
        visible,
        applied: runtime_result.is_ok(),
        persisted: persistence_result.is_ok(),
        error,
    }
}

#[tauri::command]
fn set_prompt_button_visibility(
    visible: bool,
    app: tauri::AppHandle,
    settings_state: tauri::State<SettingsFileState>,
    visibility_state: tauri::State<PromptButtonVisibilityState>,
) -> PromptButtonVisibilityOutcome {
    apply_prompt_button_visibility(
        visible,
        &app,
        settings_state.inner(),
        visibility_state.inner(),
    )
}

fn build_menu_bar_menu(
    app_handle: &tauri::AppHandle,
    language: &str,
) -> Result<Menu<tauri::Wry>, String> {
    let labels = menu_labels_for_language(language);
    let open_main = MenuItem::with_id(
        app_handle,
        TRAY_OPEN_MAIN_ID,
        labels.open_main,
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let open_settings = MenuItem::with_id(
        app_handle,
        TRAY_OPEN_SETTINGS_ID,
        labels.open_settings,
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let show_button = MenuItem::with_id(
        app_handle,
        TRAY_SHOW_BUTTON_ID,
        labels.show_button,
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let hide_button = MenuItem::with_id(
        app_handle,
        TRAY_HIDE_BUTTON_ID,
        labels.hide_button,
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let open_accessibility = MenuItem::with_id(
        app_handle,
        TRAY_OPEN_ACCESSIBILITY_ID,
        labels.open_accessibility,
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app_handle, TRAY_QUIT_ID, labels.quit, true, None::<&str>)
        .map_err(|e| e.to_string())?;

    Menu::with_items(
        app_handle,
        &[
            &open_main,
            &open_settings,
            &show_button,
            &hide_button,
            &open_accessibility,
            &separator,
            &quit,
        ],
    )
    .map_err(|e| e.to_string())
}

fn startup_prompt_button_position(settings_state: &SettingsFileState) -> (f64, f64) {
    let fallback = (960.0, 700.0);
    let Ok(Some(contents)) = settings_state.read_text() else {
        return fallback;
    };
    parse_saved_button_position(&contents).unwrap_or(fallback)
}

fn setup_menu_bar_app(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let settings_state = app_handle.state::<SettingsFileState>();
    let menu = build_menu_bar_menu(app_handle, settings_language(&settings_state.read_value()))?;

    let tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(PRODUCT_NAME)
        .show_menu_on_left_click(true)
        .icon_as_template(true)
        .icon(menubar_template_icon())
        .on_menu_event(|app, event| match tray_menu_action(event.id().as_ref()) {
            TrayMenuAction::OpenMainWindow => {
                let _ = open_main_window(app.clone());
            }
            TrayMenuAction::OpenSettingsWindow => {
                let _ = open_settings_window(app.clone());
            }
            TrayMenuAction::ShowFloatingButton => {
                let settings_state = app.state::<SettingsFileState>();
                let visibility_state = app.state::<PromptButtonVisibilityState>();
                let _ = apply_prompt_button_visibility(
                    true,
                    app,
                    settings_state.inner(),
                    visibility_state.inner(),
                );
            }
            TrayMenuAction::HideFloatingButton => {
                let settings_state = app.state::<SettingsFileState>();
                let visibility_state = app.state::<PromptButtonVisibilityState>();
                let _ = apply_prompt_button_visibility(
                    false,
                    app,
                    settings_state.inner(),
                    visibility_state.inner(),
                );
            }
            TrayMenuAction::OpenAccessibilitySettings => {
                let _ = platform::macos::open_accessibility_settings();
            }
            TrayMenuAction::Quit => app.exit(0),
            TrayMenuAction::Unknown => {}
        });

    tray_builder.build(app_handle).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .manage(LastInputTargetState::default())
        .manage(PromptPickSessionState::default())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .on_page_load(|webview, payload| {
            if is_prompt_button_webview(webview.label())
                && payload.event() == tauri::webview::PageLoadEvent::Started
            {
                let _ = crate::windows::hide_prompt_button_pair(webview.app_handle());
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        handle_prompt_button_webcontent_termination(webview.app_handle(), webview.label());
    });

    builder
        .invoke_handler(tauri::generate_handler![
            accessibility_status_cmd,
            request_accessibility_permission_cmd,
            prompt_interaction_permission_status,
            request_prompt_interaction_permission,
            open_accessibility_settings,
            frontmost_app_cmd,
            current_input_target,
            begin_prompt_pick_session,
            paste_prompt,
            paste_prompt_and_submit_to_last_target,
            paste_prompt_sequence_and_submit_to_last_target,
            show_prompt_button,
            hide_prompt_button,
            show_prompt_popover,
            hide_prompt_popover,
            show_prompt_popover_from_button,
            toggle_prompt_popover_from_button,
            show_prompt_button_controls_from_button,
            acknowledge_prompt_popover_mode,
            prompt_button_position_cmd,
            move_prompt_button_to,
            open_main_window,
            open_settings_window,
            set_menu_language,
            quit_prompt_picker,
            read_prompt_library_file,
            write_prompt_library_file,
            prompt_library_file_metadata,
            read_settings_text,
            write_settings_text,
            set_prompt_button_visibility,
            set_prompt_button_renderer_ready,
            ack_prompt_button_visual_probe,
            set_prompt_button_interaction,
            #[cfg(debug_assertions)]
            calico_probe::record_calico_surface_probe,
            #[cfg(debug_assertions)]
            simulate_prompt_button_webcontent_termination
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            if calico_probe::enabled() {
                return calico_probe::setup(app.handle()).map_err(Into::into);
            }

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let path = settings_path(app.handle())
                .ok_or_else(|| "Could not resolve settings path.".to_string())?;
            let settings_state = SettingsFileState::new(path);
            let initial_settings = settings_state.read_value();
            let visibility_state = PromptButtonVisibilityState::from_settings(&initial_settings);
            app.manage(settings_state);
            app.manage(visibility_state);
            app.manage(PromptButtonRendererState::default());
            app.manage(visual_liveness::PromptButtonVisualHealthState::default());
            let visual_logger = app
                .path()
                .app_log_dir()
                .ok()
                .and_then(|path| visual_liveness_log::VisualLivenessLogger::start(path).ok())
                .unwrap_or_default();
            app.manage(visual_logger);
            app.manage(PromptButtonRecoveryUrlState::default());
            app.manage(crate::windows::PopoverModeRequestState::default());

            let autosend_health_app = app.handle().clone();
            app.listen("prompt-autosend-activity", move |event| {
                #[derive(serde::Deserialize)]
                struct ActivityPayload {
                    active: bool,
                }
                if let Ok(payload) = serde_json::from_str::<ActivityPayload>(event.payload()) {
                    autosend_health_app
                        .state::<visual_liveness::PromptButtonVisualHealthState>()
                        .set_autosend_active(payload.active);
                }
            });

            setup_menu_bar_app(app.handle())?;
            if let Err(error) = crate::windows::prewarm_prompt_popover(app.handle()) {
                eprintln!("Prompt popover prewarm failed: {error}");
            }

            let window = app.get_webview_window("main").unwrap();
            window.set_title(PRODUCT_NAME).unwrap();
            let main_window = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = main_window.hide();
                }
            });
            let visibility_state = app.state::<PromptButtonVisibilityState>();
            if visibility_state.desired_visible() {
                let settings_state = app.state::<SettingsFileState>();
                let (x, y) = startup_prompt_button_position(settings_state.inner());
                let _ = show_prompt_button(x, y, app.handle().clone());
            }

            let monitor_app = app.handle().clone();
            std::thread::spawn(move || loop {
                let delay = monitor_app
                    .state::<visual_liveness::PromptButtonVisualHealthState>()
                    .next_monitor_delay();
                std::thread::sleep(delay);
                run_prompt_button_visual_monitor_once(&monitor_app);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod prompt_button_renderer_tests {
    use super::*;

    #[test]
    fn allocates_nonzero_instances_and_resets_readiness() {
        let state = PromptButtonRendererState::default();
        let first = state.allocate_instance();
        let visibility = PromptButtonVisibilityState::new(true);
        assert!(state.accept(first, 1, true, &visibility).is_some());

        let second = state.allocate_instance();

        assert_ne!(first, second);
        assert_ne!(second, 0);
        assert!(!state.is_ready());
        assert_eq!(state.current(), (second, 0, false));
    }

    #[test]
    fn rejects_stale_instances_and_out_of_order_transitions() {
        let state = PromptButtonRendererState::default();
        let visibility = PromptButtonVisibilityState::new(true);
        let old = state.allocate_instance();
        let current = state.allocate_instance();

        assert!(state.accept(old, 1, true, &visibility).is_none());
        assert!(state.accept(current, 2, true, &visibility).is_some());
        assert!(state.accept(current, 1, false, &visibility).is_none());
        assert_eq!(state.current(), (current, 2, true));
    }

    #[test]
    fn newer_transition_invalidates_an_older_lock_free_action() {
        let state = PromptButtonRendererState::default();
        let visibility = PromptButtonVisibilityState::new(true);
        let instance = state.allocate_instance();
        let old_hide = state.accept(instance, 1, false, &visibility).unwrap();
        let new_show = state.accept(instance, 2, true, &visibility).unwrap();

        assert!(!state.action_is_current(old_hide, &visibility));
        assert!(state.action_is_current(new_show, &visibility));
    }

    #[test]
    fn close_racing_with_ready_true_prevents_show() {
        let state = PromptButtonRendererState::default();
        let visibility = PromptButtonVisibilityState::new(true);
        let instance = state.allocate_instance();
        let show = state.accept(instance, 1, true, &visibility).unwrap();

        visibility.set(false);

        assert!(!state.action_is_current(show, &visibility));
    }

    #[test]
    fn only_prompt_button_termination_is_owned_by_this_recovery() {
        assert!(is_prompt_button_webview(
            crate::windows::BUTTON_WINDOW_LABEL
        ));
        assert!(!is_prompt_button_webview(
            crate::windows::POPOVER_WINDOW_LABEL
        ));
        assert!(!is_prompt_button_webview("main"));
    }

    #[test]
    fn termination_recovery_hides_and_navigates_the_existing_window() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(PromptButtonRendererState::default());
        app.manage(PromptButtonRecoveryUrlState::default());
        let window = tauri::WebviewWindowBuilder::new(
            &app,
            crate::windows::BUTTON_WINDOW_LABEL,
            tauri::WebviewUrl::App("overlay.html?rendererInstanceId=1".into()),
        )
        .visible(true)
        .build()
        .unwrap();
        app.state::<PromptButtonRecoveryUrlState>()
            .store(window.url().unwrap());
        let instance_id = app.state::<PromptButtonRendererState>().allocate_instance();

        recover_prompt_button_webview_once(app.handle(), instance_id).unwrap();

        let recovered = app
            .get_webview_window(crate::windows::BUTTON_WINDOW_LABEL)
            .unwrap();
        let url = recovered.url().unwrap();
        assert_eq!(url.path(), "/overlay.html");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "rendererInstanceId")
                .map(|(_, value)| value.into_owned()),
            Some(instance_id.to_string())
        );
    }

    #[test]
    fn termination_recovery_url_falls_back_to_the_stored_creation_url() {
        let hidden = std::cell::Cell::new(false);
        let navigated = std::cell::RefCell::new(None);
        let stored =
            tauri::Url::parse("tauri://localhost/overlay.html?rendererInstanceId=1&source=stored")
                .unwrap();

        let recovered = perform_prompt_button_webview_recovery(
            Some(stored),
            7,
            || {
                hidden.set(true);
                Ok(())
            },
            || Err("terminated webview URL unavailable".to_string()),
            |url| {
                *navigated.borrow_mut() = Some(url);
                Ok(())
            },
        )
        .unwrap();

        assert!(hidden.get());
        assert_eq!(navigated.borrow().as_ref(), Some(&recovered));
        assert_eq!(recovered.path(), "/overlay.html");
        assert_eq!(recovered.query(), Some("rendererInstanceId=7"));
        assert!(prompt_button_recovery_url(Err("unavailable".into()), None, 7).is_err());
    }

    #[test]
    fn termination_recovery_propagates_hide_and_navigation_failures() {
        let stored = tauri::Url::parse("tauri://localhost/overlay.html").unwrap();
        let hide_error = perform_prompt_button_webview_recovery(
            Some(stored.clone()),
            2,
            || Err("hide failed".to_string()),
            || Ok(stored.clone()),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(hide_error.contains("hide failed"));

        let navigate_error = perform_prompt_button_webview_recovery(
            Some(stored.clone()),
            2,
            || Ok(()),
            || Ok(stored),
            |_| Err("navigate failed".to_string()),
        )
        .unwrap_err();
        assert!(navigate_error.contains("navigate failed"));
    }
}

#[cfg(test)]
mod last_input_target_tests {
    use super::*;
    use crate::platform::macos::AutosendFailureReason;
    use std::{cell::RefCell, collections::VecDeque};

    fn frontmost_target(name: &str, bundle_id: &str, pid: Option<u32>) -> FrontmostAppWithPid {
        FrontmostAppWithPid {
            app: FrontmostApp {
                name: name.to_string(),
                bundle_id: bundle_id.to_string(),
            },
            pid,
        }
    }

    fn prompt_target(name: &str, bundle_id: &str, pid: Option<u32>) -> PromptPickSessionTarget {
        PromptPickSessionTarget {
            app: FrontmostApp {
                name: name.to_string(),
                bundle_id: bundle_id.to_string(),
            },
            pid,
            observed_at_ms: now_ms(),
            click_point: None,
        }
    }

    fn captured_identity(bundle_id: &str, pid: u32) -> CapturedTargetIdentity {
        CapturedTargetIdentity {
            application: TargetApplicationIdentity {
                bundle_id: bundle_id.to_string(),
                main_pid: pid,
                launch_identity: platform::macos::process_launch_identity(pid)
                    .expect("test process should have launch metadata"),
            },
            window: None,
            page_url: None,
        }
    }

    #[test]
    fn fresh_target_identity_keeps_the_browser_window_for_calibrated_focus() {
        let window = TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: None,
            cg_window_id: None,
        };
        let identity = fresh_captured_target_identity(
            "com.google.Chrome",
            42,
            platform::ProcessLaunchIdentity {
                seconds: 1,
                microseconds: 2,
            },
            Some(window.clone()),
        );

        assert_eq!(identity.window, Some(window));
    }

    #[test]
    fn background_page_detection_enriches_only_the_current_browser_session() {
        let state = PromptPickSessionState::default();
        state.begin(7);
        state.set_captured_if_current(
            7,
            prompt_target("Chrome", "com.google.Chrome", Some(42)),
            captured_identity("com.google.Chrome", std::process::id()),
        );

        assert!(state.set_page_url_if_current(
            7,
            "com.google.Chrome",
            "https://chatgpt.com/c/123".to_string(),
        ));
        assert_eq!(
            state.captured_identity().unwrap().page_url.as_deref(),
            Some("https://chatgpt.com/c/123")
        );
        assert!(!state.set_page_url_if_current(
            6,
            "com.google.Chrome",
            "https://gemini.google.com/app/123".to_string(),
        ));
    }

    #[test]
    fn background_capture_cannot_replace_an_already_frozen_window() {
        let state = PromptPickSessionState::default();
        state.begin(8);
        let target = prompt_target("Chrome", "com.google.Chrome", Some(42));
        let mut frozen = captured_identity("com.google.Chrome", std::process::id());
        frozen.window = Some(TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("frozen".to_string()),
            cg_window_id: None,
        });
        let mut background = frozen.clone();
        background.window.as_mut().unwrap().frame.x = 500.0;
        background.window.as_mut().unwrap().title_hash = Some("background".to_string());

        assert!(state.set_captured_if_current(8, target.clone(), frozen.clone()));
        assert!(state.set_captured_if_current(8, target, background));

        assert_eq!(state.captured_identity(), Some(frozen));
    }

    #[test]
    fn background_capture_fills_a_missing_frozen_window() {
        let state = PromptPickSessionState::default();
        state.begin(8);
        let target = prompt_target("Chrome", "com.google.Chrome", Some(42));
        let mut frozen = captured_identity("com.google.Chrome", std::process::id());
        frozen.window = None;
        let mut background = frozen.clone();
        background.window = Some(TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("background".to_string()),
            cg_window_id: None,
        });

        assert!(state.set_captured_if_current(8, target.clone(), frozen));
        assert!(state.set_captured_if_current(8, target, background.clone()));

        assert_eq!(state.captured_identity(), Some(background));
    }

    #[test]
    fn target_application_identity_rejects_restarted_process() {
        let identity = TargetApplicationIdentity {
            bundle_id: "com.anthropic.claudefordesktop".to_string(),
            main_pid: 42,
            launch_identity: platform::ProcessLaunchIdentity {
                seconds: 100,
                microseconds: 200,
            },
        };

        assert!(target_application_identity_matches(
            &identity,
            Some(platform::ProcessLaunchIdentity {
                seconds: 100,
                microseconds: 200,
            })
        ));
        assert!(!target_application_identity_matches(
            &identity,
            Some(platform::ProcessLaunchIdentity {
                seconds: 101,
                microseconds: 0,
            })
        ));
    }

    #[test]
    fn captured_target_validity_uses_process_identity_not_stale_window_geometry() {
        let mut identity = captured_identity("com.anthropic.claudefordesktop", std::process::id());
        identity.window = Some(TargetWindowIdentity {
            owner_pid: std::process::id(),
            frame: CandidateInput {
                x: -5000.0,
                y: -5000.0,
                width: 1.0,
                height: 1.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("stale".to_string()),
            cg_window_id: None,
        });

        assert!(captured_target_identity_is_current(&identity));
    }

    #[test]
    fn claude_and_wechat_require_window_identity_but_codex_does_not() {
        assert!(bundle_requires_exact_window(
            "com.anthropic.claudefordesktop"
        ));
        assert!(bundle_requires_exact_window("com.tencent.xinWeChat"));
        assert!(!bundle_requires_exact_window("com.openai.codex"));
    }

    #[test]
    fn stores_and_reads_last_input_target() {
        let state = LastInputTargetState::default();
        let target = LastInputTarget {
            app: FrontmostApp {
                name: "Notes".to_string(),
                bundle_id: "com.apple.Notes".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        };

        state.set(target);

        assert_eq!(state.get().unwrap().app.bundle_id, "com.apple.Notes");
        assert_eq!(state.get().unwrap().pid, None);
    }

    #[test]
    fn cloned_last_input_target_state_shares_target() {
        let state = LastInputTargetState::default();
        let cloned = state.clone();

        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 12.0, y: 34.0 }),
        });

        assert_eq!(cloned.get().unwrap().app.bundle_id, "com.openai.codex");
    }

    #[test]
    fn cloned_prompt_pick_session_state_shares_target() {
        let state = PromptPickSessionState::default();
        let cloned = state.clone();

        state.begin(7);
        assert!(cloned.set_if_current(
            7,
            PromptPickSessionTarget {
                app: FrontmostApp {
                    name: "Codex".to_string(),
                    bundle_id: "com.openai.codex".to_string(),
                },
                pid: None,
                observed_at_ms: now_ms(),
                click_point: None,
            }
        ));

        assert_eq!(state.take().unwrap().app.bundle_id, "com.openai.codex");
    }

    #[test]
    fn begin_if_new_preserves_target_for_current_session() {
        let state = PromptPickSessionState::default();
        state.begin(7);
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        });

        state.begin_if_new(7);
        assert_eq!(state.get().unwrap().app.bundle_id, "com.openai.codex");

        state.begin_if_new(8);
        assert!(state.get().is_none());
    }

    #[test]
    fn session_is_consumed_reflects_active_session_and_consumed_flag() {
        let state = PromptPickSessionState::default();
        state.begin(7);
        assert!(!state.is_consumed(7));
        assert!(state.is_consumed(8)); // stale session

        let _ = state.take(); // consumed
        assert!(state.is_consumed(7));
    }

    #[test]
    fn set_window_if_current_only_fills_a_missing_window() {
        let state = PromptPickSessionState::default();
        state.begin(11);
        state.set_captured_if_current(
            11,
            prompt_target("Chrome", "com.google.Chrome", Some(42)),
            captured_identity("com.google.Chrome", std::process::id()),
        );
        assert!(state.captured_identity().unwrap().window.is_none());

        let first = TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("first".to_string()),
            cg_window_id: None,
        };
        let second = TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 500.0,
                y: 600.0,
                width: 1000.0,
                height: 700.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("second".to_string()),
            cg_window_id: None,
        };

        assert!(state.set_window_if_current(11, first.clone()));
        assert_eq!(
            state.captured_identity().unwrap().window.as_ref(),
            Some(&first)
        );
        assert!(state.set_window_if_current(11, second));
        assert_eq!(
            state.captured_identity().unwrap().window.as_ref(),
            Some(&first)
        );
    }

    #[test]
    fn target_capture_complete_requires_window_and_browser_page_url() {
        let state = PromptPickSessionState::default();
        state.begin(12);
        assert!(!state.target_capture_complete(12));

        state.set_captured_if_current(
            12,
            prompt_target("Chrome", "com.google.Chrome", Some(42)),
            captured_identity("com.google.Chrome", std::process::id()),
        );
        assert!(!state.target_capture_complete(12));

        let window = TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: Some("complete".to_string()),
            cg_window_id: None,
        };
        assert!(state.set_window_if_current(12, window));
        assert!(!state.target_capture_complete(12));

        assert!(state.set_page_url_if_current(
            12,
            "com.google.Chrome",
            "https://chatgpt.com/c/123".to_string(),
        ));
        assert!(state.target_capture_complete(12));

        // Non-browser apps do not require page_url.
        state.begin(13);
        let mut codex = captured_identity("com.openai.codex", std::process::id());
        codex.window = Some(TargetWindowIdentity {
            owner_pid: 42,
            frame: CandidateInput {
                x: 1.0,
                y: 2.0,
                width: 800.0,
                height: 600.0,
            },
            role: Some("AXWindow".to_string()),
            title_hash: None,
            cg_window_id: None,
        });
        state.set_captured_if_current(
            13,
            prompt_target("Codex", "com.openai.codex", Some(42)),
            codex,
        );
        assert!(state.target_capture_complete(13));
        assert!(state.target_capture_complete(99)); // stale session exits promptly
    }

    #[test]
    fn prompt_pick_session_capture_wait_is_bounded() {
        let state = PromptPickSessionState::default();
        state.begin(7);
        assert!(state.mark_capture_pending(7));

        let started = std::time::Instant::now();
        assert!(!state.wait_for_capture(std::time::Duration::from_millis(20)));
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
    }

    #[test]
    fn stale_capture_completion_does_not_finish_the_current_session() {
        let state = PromptPickSessionState::default();
        state.begin(7);
        assert!(state.mark_capture_pending(7));
        state.begin(8);
        assert!(state.mark_capture_pending(8));

        assert!(!state.finish_capture(7));
        assert!(!state.wait_for_capture(std::time::Duration::from_millis(1)));
        assert!(state.finish_capture(8));
        assert!(state.wait_for_capture(std::time::Duration::from_millis(1)));
    }

    #[test]
    fn consumed_session_rejects_late_background_capture() {
        let state = PromptPickSessionState::default();
        state.begin(9);
        state.set(prompt_target("Codex", "com.openai.codex", Some(42)));
        assert!(state.take().is_some());

        assert!(!state.set_if_current(9, prompt_target("Chrome", "com.google.Chrome", Some(84))));
    }

    #[test]
    fn prompt_pick_session_freezes_the_current_frontmost_app() {
        let session = PromptPickSessionState::default();
        let recent = LastInputTarget {
            app: FrontmostApp {
                name: "Old App".to_string(),
                bundle_id: "com.example.old".to_string(),
            },
            pid: Some(42),
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        };

        session.begin(11);
        let frozen = freeze_prompt_pick_session_target(
            &session,
            Some(FrontmostAppWithPid {
                app: FrontmostApp {
                    name: "Current App".to_string(),
                    bundle_id: "com.example.current".to_string(),
                },
                pid: Some(84),
            }),
            Some(recent),
            11,
        );

        assert_eq!(frozen.unwrap().bundle_id, "com.example.current");
        let target = session
            .get()
            .expect("frontmost app should be frozen for the active session");
        assert_eq!(target.pid, Some(84));
        assert_eq!(target.click_point, None);
    }

    #[test]
    fn prompt_pick_session_reuses_recent_click_point_only_for_the_same_app() {
        let session = PromptPickSessionState::default();
        session.begin(12);
        let recent = LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: Some(42),
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        };

        freeze_prompt_pick_session_target(
            &session,
            Some(FrontmostAppWithPid {
                app: FrontmostApp {
                    name: "Codex".to_string(),
                    bundle_id: "com.openai.codex".to_string(),
                },
                pid: Some(42),
            }),
            Some(recent),
            12,
        );

        assert_eq!(
            session.get().unwrap().click_point,
            Some(TargetClickPoint { x: 640.0, y: 720.0 })
        );
    }

    #[test]
    fn background_capture_cannot_replace_the_frozen_app() {
        let session = PromptPickSessionState::default();
        session.begin(13);
        session.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Current App".to_string(),
                bundle_id: "com.example.current".to_string(),
            },
            pid: Some(84),
            observed_at_ms: now_ms(),
            click_point: None,
        });

        assert!(enrich_prompt_pick_session_target_if_matching(
            &session,
            PromptPickSessionTarget {
                app: FrontmostApp {
                    name: "Other App".to_string(),
                    bundle_id: "com.example.other".to_string(),
                },
                pid: Some(126),
                observed_at_ms: now_ms(),
                click_point: Some(TargetClickPoint { x: 10.0, y: 20.0 }),
            },
            13,
        )
        .is_none());
        assert_eq!(session.get().unwrap().app.bundle_id, "com.example.current");
        assert_eq!(session.get().unwrap().click_point, None);
    }

    #[test]
    fn autosend_and_prompt_capture_commands_use_spawn_blocking() {
        let source = include_str!("lib.rs");

        assert!(source.contains("async fn begin_prompt_pick_session"));
        assert!(source.contains("async fn paste_prompt_and_submit_to_last_target"));
        assert!(source.contains("async fn paste_prompt_sequence_and_submit_to_last_target"));
        assert!(
            source
                .matches("tauri::async_runtime::spawn_blocking(move ||")
                .count()
                >= 3
        );
    }

    #[test]
    fn prompt_capture_command_begins_session_before_recording_target() {
        let source = include_str!("lib.rs");
        let start = source
            .find("async fn begin_prompt_pick_session")
            .expect("begin_prompt_pick_session should exist");
        let end = source[start..]
            .find("#[tauri::command]\nfn paste_prompt")
            .expect("next command should follow begin_prompt_pick_session");
        let command_source = &source[start..start + end];

        assert!(command_source.contains("session_state.begin_if_new(session_id);"));
        assert!(command_source.contains("record_prompt_pick_session_target_if_valid"));
        assert!(command_source.contains("platform::macos::active_browser_page_url"));
        assert!(command_source.contains("set_page_url_if_current"));
        assert!(!command_source.contains("session_state.clear_if_current(session_id);"));
    }

    #[test]
    fn autosend_command_impls_use_activating_session_sender() {
        let source = include_str!("lib.rs");
        let single_start = source
            .find("fn paste_prompt_and_submit_to_last_target_impl")
            .expect("single autosend impl should exist");
        let single_end = source[single_start..]
            .find("fn copy_text_to_clipboard")
            .expect("copy helper should follow single autosend impl");
        let single_source = &source[single_start..single_start + single_end];

        assert!(single_source.contains("paste_prompt_and_submit_to_session_target_with_senders"));
        assert!(single_source.contains("paste_prompt_and_submit_to_app_clipboard_with_copier"));
        assert!(single_source.contains("captured_page_url.as_deref()"));
        assert!(single_source.contains("state,\n        None,"));
        assert!(!single_source.contains("focus_preserving_prompt_to_last_target_impl("));

        let sequence_start = source
            .find("fn paste_prompt_sequence_and_submit_to_last_target_impl")
            .expect("sequence autosend impl should exist");
        let sequence_end = source[sequence_start..]
            .find("fn paste_prompt_and_submit_to_last_target_impl")
            .expect("single autosend impl should follow sequence autosend impl");
        let sequence_source = &source[sequence_start..sequence_start + sequence_end];

        assert!(sequence_source
            .contains("paste_prompt_sequence_and_submit_to_session_target_with_senders"));
        assert!(sequence_source.contains("paste_prompt_and_submit_to_app_clipboard_with_copier"));
        assert!(sequence_source.contains("state,\n        None,"));
        assert!(!sequence_source.contains("focus_preserving_prompt_sequence_to_last_target_impl("));
    }

    #[test]
    fn legacy_activating_paste_commands_are_not_registered() {
        let source = include_str!("lib.rs");
        let handler_start = source
            .find("tauri::generate_handler![")
            .expect("invoke handler should be registered");
        let handler_rest = &source[handler_start..];
        let handler_end = handler_rest
            .find("])")
            .expect("invoke handler should close");
        let handler_source = &handler_rest[..handler_end];

        assert!(!handler_source.contains(concat!("paste_prompt", "_to_app,")));
        assert!(!handler_source.contains(concat!("paste_prompt", "_to_last_target,")));
    }

    #[test]
    fn parses_saved_startup_button_position() {
        let settings = r#"{
            "version": 1,
            "overlayPlacement": {
                "buttonOffset": null,
                "buttonPosition": { "x": 1765, "y": 419 }
            },
            "floatingButton": { "visible": true }
        }"#;

        assert_eq!(parse_saved_button_position(settings), Some((1765.0, 419.0)));
    }

    #[test]
    fn ignores_invalid_saved_startup_button_position() {
        let settings = r#"{
            "version": 1,
            "overlayPlacement": {
                "buttonPosition": { "x": "bad", "y": 419 }
            }
        }"#;

        assert_eq!(parse_saved_button_position(settings), None);
    }

    #[test]
    fn resolves_menu_labels_by_language() {
        assert_eq!(menu_labels_for_language("zh-CN").open_main, "管理提示词...");
        assert_eq!(menu_labels_for_language("zh-CN").open_settings, "设置...");
        assert_eq!(
            menu_labels_for_language("zh-CN").show_button,
            "显示 Sleepy Cat"
        );
        assert_eq!(
            menu_labels_for_language("zh-CN").hide_button,
            "隐藏 Sleepy Cat"
        );
        assert_eq!(
            menu_labels_for_language("en-US").open_main,
            "Manage Prompts..."
        );
        assert_eq!(
            menu_labels_for_language("en-US").show_button,
            "Show Sleepy Cat"
        );
        assert_eq!(
            menu_labels_for_language("en-US").hide_button,
            "Hide Sleepy Cat"
        );
        assert_eq!(menu_labels_for_language("bad").quit, "Quit Sleepy Cat");
    }

    #[test]
    fn records_non_prompt_picker_input_target() {
        let state = LastInputTargetState::default();
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            window_frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            button_position: (10.0, 10.0),
            click_point: (6.0, 5.0),
            app: Some(FrontmostApp {
                name: "Notes".to_string(),
                bundle_id: "com.apple.Notes".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        assert_eq!(state.get().unwrap().app.bundle_id, "com.apple.Notes");
        assert_eq!(state.get().unwrap().pid, Some(std::process::id()));
        assert_eq!(state.get().unwrap().click_point.unwrap().x, 6.0);
    }

    #[test]
    fn records_input_click_point_separately_from_button_position() {
        let state = LastInputTargetState::default();
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 700.0,
                y: 680.0,
                width: 500.0,
                height: 96.0,
            },
            window_frame: CandidateInput {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            },
            button_position: (1200.0, 776.0),
            click_point: (760.0, 728.0),
            app: Some(FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        let stored = state.get().unwrap();
        let click_point = stored.click_point.unwrap();
        assert_eq!(click_point.x, 760.0);
        assert_eq!(click_point.y, 728.0);
        assert_ne!(click_point.x, target.button_position.0);
    }

    #[test]
    fn rejects_wechat_target_without_exact_window_identity() {
        let state = LastInputTargetState::default();
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            window_frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            button_position: (90.0, 90.0),
            click_point: (50.0, 50.0),
            app: Some(FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        assert!(state.get().is_none());
    }

    #[test]
    fn clears_unsafe_finder_target_without_focused_input_frame() {
        let state = LastInputTargetState::default();
        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        });
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            window_frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            button_position: (90.0, 90.0),
            click_point: (50.0, 50.0),
            app: Some(FrontmostApp {
                name: "Finder".to_string(),
                bundle_id: "com.apple.finder".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        assert!(state.get().is_none());
    }

    #[test]
    fn rejects_claude_target_without_exact_window_identity() {
        let state = LastInputTargetState::default();
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            window_frame: CandidateInput {
                x: 100.0,
                y: 200.0,
                width: 800.0,
                height: 600.0,
            },
            button_position: (876.0, 776.0),
            click_point: (500.0, 735.0),
            app: Some(FrontmostApp {
                name: "Claude".to_string(),
                bundle_id: "com.anthropic.claudefordesktop".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        assert!(state.get().is_none());
    }

    #[test]
    fn recovery_click_point_prefers_recorded_input_point_over_pointer_fallback() {
        let recorded = Some(TargetClickPoint { x: 400.0, y: 700.0 });
        let pointer = Some(TargetClickPoint { x: 100.0, y: 100.0 });
        let window = CandidateInput {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        };

        assert_eq!(
            choose_recovery_click_point(recorded, pointer, Some(&window), None),
            recorded
        );
    }

    #[test]
    fn recovery_click_point_uses_pointer_only_inside_target_window() {
        let pointer = Some(TargetClickPoint { x: 100.0, y: 100.0 });
        let fallback = Some(TargetClickPoint { x: 500.0, y: 735.0 });
        let window = CandidateInput {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };

        assert_eq!(
            choose_recovery_click_point(None, pointer, Some(&window), fallback),
            pointer
        );
    }

    #[test]
    fn recovery_click_point_uses_pointer_before_generic_fallback() {
        let pointer = Some(TargetClickPoint { x: 120.0, y: 140.0 });
        let fallback = Some(TargetClickPoint { x: 500.0, y: 735.0 });
        let window = CandidateInput {
            x: 100.0,
            y: 100.0,
            width: 600.0,
            height: 500.0,
        };

        assert_eq!(
            choose_recovery_click_point(None, pointer, Some(&window), fallback),
            pointer
        );
    }

    #[test]
    fn recovery_click_point_rejects_pointer_outside_target_window() {
        let pointer = Some(TargetClickPoint { x: 300.0, y: 300.0 });
        let fallback = Some(TargetClickPoint { x: 500.0, y: 735.0 });
        let window = CandidateInput {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };

        assert_eq!(
            choose_recovery_click_point(None, pointer, Some(&window), fallback),
            fallback
        );
    }

    #[test]
    fn skips_prompt_picker_as_last_input_target() {
        let state = LastInputTargetState::default();
        let target = platform::InputTarget {
            frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            window_frame: CandidateInput {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            button_position: (10.0, 10.0),
            click_point: (6.0, 5.0),
            app: Some(FrontmostApp {
                name: PRODUCT_NAME.to_string(),
                bundle_id: "local.promptpicker.dev".to_string(),
            }),
            pid: std::process::id(),
        };

        record_last_input_target_if_valid(&state, &target);

        assert!(state.get().is_none());
    }

    #[test]
    fn prompt_pick_session_uses_frontmost_business_app() {
        let state = PromptPickSessionState::default();
        state.begin(1);
        freeze_prompt_pick_session_target(
            &state,
            Some(frontmost_target(
                "WeChat",
                "com.tencent.xinWeChat",
                Some(123),
            )),
            None,
            1,
        );
        let target = state.get().unwrap();

        assert_eq!(target.app.bundle_id, "com.tencent.xinWeChat");
        assert_eq!(target.pid, Some(123));
    }

    #[test]
    fn prompt_pick_session_synchronously_captures_a_live_target_identity() {
        let mut child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .expect("test helper process should start");
        let pid = child.id();
        let state = PromptPickSessionState::default();
        state.begin(2);

        freeze_prompt_pick_session_target(
            &state,
            Some(frontmost_target("Chrome", "com.google.Chrome", Some(pid))),
            None,
            2,
        );

        let identity = state
            .captured_identity()
            .expect("live target identity should be captured before the popover opens");
        assert_eq!(identity.application.main_pid, pid);
        assert_eq!(identity.application.bundle_id, "com.google.Chrome");
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn captured_target_matches_only_same_bundle_and_pid() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: Some(123),
            observed_at_ms: now_ms(),
            click_point: None,
        };
        let same = frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let different_pid = frontmost_target("WeChat", "com.tencent.xinWeChat", Some(456));
        let different_bundle = frontmost_target("Notes", "com.apple.Notes", Some(123));

        assert!(captured_target_matches_frontmost(&target, Some(&same)));
        assert!(!captured_target_matches_frontmost(
            &target,
            Some(&different_pid)
        ));
        assert!(!captured_target_matches_frontmost(
            &target,
            Some(&different_bundle)
        ));
        assert!(!captured_target_matches_frontmost(&target, None));
    }

    #[test]
    fn captured_target_falls_back_to_bundle_when_pid_is_unavailable() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Notes".to_string(),
                bundle_id: "com.apple.Notes".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: None,
        };
        let frontmost = frontmost_target("Notes", "com.apple.Notes", Some(123));

        assert!(captured_target_matches_frontmost(&target, Some(&frontmost)));
    }

    #[test]
    fn classifies_frontmost_target_status() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: Some(123),
            observed_at_ms: now_ms(),
            click_point: None,
        };

        assert_eq!(
            classify_target_frontmost(
                &target,
                Some(&frontmost_target("Codex", "com.openai.codex", Some(123)))
            ),
            TargetFrontmostStatus::Target
        );
        assert_eq!(
            classify_target_frontmost(
                &target,
                Some(&frontmost_target(
                    PRODUCT_NAME,
                    "local.promptpicker.dev",
                    Some(1)
                ))
            ),
            TargetFrontmostStatus::PromptPicker
        );
        assert_eq!(
            classify_target_frontmost(
                &target,
                Some(&frontmost_target("Notes", "com.apple.Notes", Some(456)))
            ),
            TargetFrontmostStatus::OtherOrUnknown
        );
        assert_eq!(
            classify_target_frontmost(&target, None),
            TargetFrontmostStatus::OtherOrUnknown
        );
    }

    #[test]
    fn autosend_diagnostic_line_includes_classification_and_click_point_state() {
        let line = autosend_diagnostic_line(
            "before-paste",
            Some("com.openai.codex"),
            true,
            Some("local.promptpicker.dev"),
            Some(TargetFrontmostStatus::PromptPicker),
        );

        assert!(line.contains("before-paste"));
        assert!(line.contains("target=com.openai.codex"));
        assert!(line.contains("has_click_point=true"));
        assert!(line.contains("frontmost=local.promptpicker.dev"));
        assert!(line.contains("classification=PromptPicker"));
        assert!(!line.contains("prompt body"));
    }

    #[test]
    fn submit_key_parser_accepts_known_values() {
        assert_eq!(
            native_submit_key_from_arg(None).unwrap(),
            platform::macos::NativeSubmitKey::Enter
        );
        assert_eq!(
            native_submit_key_from_arg(Some("none".to_string())).unwrap(),
            platform::macos::NativeSubmitKey::None
        );
        assert_eq!(
            native_submit_key_from_arg(Some("command_enter".to_string())).unwrap(),
            platform::macos::NativeSubmitKey::CommandEnter
        );
    }

    #[test]
    fn submit_key_parser_rejects_unknown_values() {
        assert_eq!(
            native_submit_key_from_arg(Some("space".to_string())).unwrap_err(),
            "Invalid submit key: space"
        );
    }

    #[test]
    fn global_submit_behavior_fails_closed_when_settings_cannot_be_read() {
        assert_eq!(
            authoritative_submit_key(Err("settings unavailable".to_string())),
            platform::macos::NativeSubmitKey::None
        );
        assert_eq!(
            authoritative_submit_key(Ok(Some("not json".to_string()))),
            platform::macos::NativeSubmitKey::None
        );
        assert_eq!(
            authoritative_submit_key(Ok(None)),
            platform::macos::NativeSubmitKey::None
        );
    }

    #[test]
    fn global_submit_behavior_reads_all_supported_settings() {
        let settings_for = |mode| {
            Ok(Some(
                serde_json::json!({
                    "promptInsertion": { "mode": mode }
                })
                .to_string(),
            ))
        };

        assert_eq!(
            authoritative_submit_key(settings_for("paste_only")),
            platform::macos::NativeSubmitKey::None
        );
        assert_eq!(
            authoritative_submit_key(settings_for("paste_enter")),
            platform::macos::NativeSubmitKey::Enter
        );
        assert_eq!(
            authoritative_submit_key(settings_for("paste_command_enter")),
            platform::macos::NativeSubmitKey::Enter
        );
        assert_eq!(
            authoritative_submit_key(settings_for("paste_and_submit")),
            platform::macos::NativeSubmitKey::Enter
        );
    }

    #[test]
    fn focus_preserving_autosend_stops_before_paste_when_frontmost_changed() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let mut frontmost = VecDeque::from([frontmost_target("Notes", "com.apple.Notes", Some(9))]);

        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |body| {
                assert_eq!(body, "hello");
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| Err("repair unavailable".to_string()),
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(outcome.copied);
        assert!(!outcome.sent);
        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
        assert_eq!(&*events.borrow(), &["copy"]);
    }

    #[test]
    fn target_recovery_can_recover_before_paste_if_picker_is_frontmost() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let mut frontmost = VecDeque::from([
            frontmost_target("Prompt Drawer", "local.promptpicker.dev", Some(1)),
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
        ]);

        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| {
                events.borrow_mut().push("recover");
                Ok(())
            },
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(outcome.sent);
        assert_eq!(
            &*events.borrow(),
            &["copy", "recover", "paste", "sleep", "submit"]
        );
    }

    #[test]
    fn autosend_recovers_app_only_target_with_recorded_click_point_when_picker_is_frontmost() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        };
        let mut frontmost = vec![
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
        ]
        .into_iter();
        let recovered = RefCell::new(false);
        let pasted = RefCell::new(false);
        let submitted = RefCell::new(false);

        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| Ok(()),
            || frontmost.next().flatten(),
            |recover_target| {
                recovered.replace(true);
                assert_eq!(recover_target.app.bundle_id, "com.openai.codex");
                assert_eq!(recover_target.click_point.unwrap().x, 640.0);
                Ok(())
            },
            || {
                pasted.replace(true);
                Ok(())
            },
            |_| {
                submitted.replace(true);
                Ok(())
            },
            |_| {},
        );

        assert!(outcome.sent);
        assert!(*recovered.borrow());
        assert!(*pasted.borrow());
        assert!(*submitted.borrow());
    }

    #[test]
    fn autosend_refuses_when_another_app_is_frontmost_before_paste() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: Some(42),
            observed_at_ms: now_ms(),
            click_point: None,
        };
        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| Ok(()),
            || Some(frontmost_target("Notes", "com.apple.Notes", Some(9))),
            |_| panic!("must not recover when a third-party app is frontmost"),
            || panic!("must not paste into the wrong app"),
            |_| panic!("must not submit into the wrong app"),
            |_| {},
        );

        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
        assert!(outcome.copied);
        assert!(!outcome.sent);
    }

    #[test]
    fn autosend_refuses_when_prompt_picker_recovery_does_not_restore_target() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: Some(42),
            observed_at_ms: now_ms(),
            click_point: None,
        };
        let mut frontmost = vec![
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
        ]
        .into_iter();
        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| Ok(()),
            || frontmost.next().flatten(),
            |_| Ok(()),
            || panic!("must not paste before target is restored"),
            |_| panic!("must not submit before target is restored"),
            |_| {},
        );

        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
        assert!(outcome.copied);
        assert!(!outcome.sent);
    }

    #[test]
    fn focus_preserving_autosend_skips_submit_when_frontmost_changes_after_paste() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let mut frontmost = VecDeque::from([
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("Notes", "com.apple.Notes", Some(9)),
        ]);

        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| Err("repair unavailable".to_string()),
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(outcome.copied);
        assert!(!outcome.sent);
        assert_eq!(
            outcome.reason,
            Some(AutosendFailureReason::ReturnEventFailed)
        );
        assert_eq!(&*events.borrow(), &["copy", "paste", "sleep"]);
    }

    #[test]
    fn focus_preserving_autosend_pastes_and_submits_when_target_stays_frontmost() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let submitted = RefCell::new(None);
        let mut frontmost = VecDeque::from([
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
        ]);

        let outcome = guarded_focus_preserving_autosend_with_senders(
            "hello",
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| {
                events.borrow_mut().push("repair");
                Err("repair must not run".to_string())
            },
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |key| {
                events.borrow_mut().push("submit");
                submitted.replace(Some(key));
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(outcome.sent);
        assert_eq!(
            *submitted.borrow(),
            Some(platform::macos::NativeSubmitKey::Enter)
        );
        assert_eq!(&*events.borrow(), &["copy", "paste", "sleep", "submit"]);
    }

    #[test]
    fn sequence_autosend_recovers_target_when_prompt_picker_is_frontmost() {
        let target = PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: Some(42),
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        };
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut frontmost = vec![
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
            Some(frontmost_target("Codex", "com.openai.codex", Some(42))),
        ]
        .into_iter();
        let recovered_count = RefCell::new(0);
        let pasted_count = RefCell::new(0);
        let submitted_count = RefCell::new(0);

        let outcome = focus_preserving_prompt_sequence_for_target_with_senders(
            &bodies,
            700,
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| Ok(()),
            || frontmost.next().flatten(),
            |_| {
                *recovered_count.borrow_mut() += 1;
                Ok(())
            },
            || {
                *pasted_count.borrow_mut() += 1;
                Ok(())
            },
            |_| {
                *submitted_count.borrow_mut() += 1;
                Ok(())
            },
            |_| {},
        );

        assert!(outcome.sent);
        assert_eq!(outcome.sent_count, 2);
        assert_eq!(*recovered_count.borrow(), 1);
        assert_eq!(*pasted_count.borrow(), 2);
        assert_eq!(*submitted_count.borrow(), 2);
    }

    #[test]
    fn focus_preserving_sequence_stops_before_first_paste_when_frontmost_changed() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut frontmost = VecDeque::from([frontmost_target("Notes", "com.apple.Notes", Some(9))]);

        let outcome = focus_preserving_prompt_sequence_for_target_with_senders(
            &bodies,
            700,
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| Err("repair unavailable".to_string()),
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(!outcome.sent);
        assert_eq!(outcome.sent_count, 0);
        assert_eq!(outcome.failed_index, Some(1));
        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
        assert_eq!(&*events.borrow(), &["copy"]);
    }

    #[test]
    fn focus_preserving_sequence_stops_before_second_paste_when_frontmost_changed() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut frontmost = VecDeque::from([
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("Notes", "com.apple.Notes", Some(9)),
        ]);

        let outcome = focus_preserving_prompt_sequence_for_target_with_senders(
            &bodies,
            700,
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| Err("repair unavailable".to_string()),
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(!outcome.sent);
        assert_eq!(outcome.sent_count, 1);
        assert_eq!(outcome.failed_index, Some(2));
        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
        assert_eq!(
            &*events.borrow(),
            &["copy", "paste", "sleep", "submit", "sleep", "copy"]
        );
    }

    #[test]
    fn focus_preserving_sequence_skips_submit_when_frontmost_changes_after_paste() {
        let target = prompt_target("WeChat", "com.tencent.xinWeChat", Some(123));
        let events = RefCell::new(Vec::new());
        let bodies = vec!["one".to_string()];
        let mut frontmost = VecDeque::from([
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(123)),
            frontmost_target("Notes", "com.apple.Notes", Some(9)),
        ]);

        let outcome = focus_preserving_prompt_sequence_for_target_with_senders(
            &bodies,
            700,
            &target,
            platform::macos::NativeSubmitKey::Enter,
            |_| {
                events.borrow_mut().push("copy");
                Ok(())
            },
            || frontmost.pop_front(),
            |_| Err("repair unavailable".to_string()),
            || {
                events.borrow_mut().push("paste");
                Ok(())
            },
            |_| {
                events.borrow_mut().push("submit");
                Ok(())
            },
            |_| events.borrow_mut().push("sleep"),
        );

        assert!(!outcome.sent);
        assert_eq!(outcome.sent_count, 0);
        assert_eq!(outcome.failed_index, Some(1));
        assert_eq!(
            outcome.reason,
            Some(AutosendFailureReason::ReturnEventFailed)
        );
        assert_eq!(&*events.borrow(), &["copy", "paste", "sleep"]);
    }

    #[test]
    fn prompt_pick_session_rejects_visible_app_without_recent_exact_target() {
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            None,
        );

        assert!(target.is_none());
    }

    #[test]
    fn prompt_pick_session_uses_topmost_underlying_app_when_picker_stole_activation() {
        let frontmost = prompt_pick_frontmost_candidate(
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(std::process::id()),
            )),
            Some(frontmost_target(
                "Google Chrome",
                "com.google.Chrome",
                Some(456),
            )),
        );
        let target = prompt_pick_session_target(
            frontmost,
            Some(LastInputTarget {
                app: FrontmostApp {
                    name: "Claude".to_string(),
                    bundle_id: "com.anthropic.claudefordesktop".to_string(),
                },
                pid: Some(123),
                observed_at_ms: now_ms() - PROMPT_PICK_RECENT_TARGET_MAX_AGE_MS - 1,
                click_point: None,
            }),
        )
        .unwrap();

        assert_eq!(target.app.bundle_id, "com.google.Chrome");
        assert_eq!(target.pid, Some(456));
    }

    #[test]
    fn prompt_pick_session_keeps_a_real_frontmost_app_over_the_underlying_fallback() {
        let frontmost = prompt_pick_frontmost_candidate(
            Some(frontmost_target("Codex", "com.openai.codex", Some(123))),
            Some(frontmost_target(
                "Google Chrome",
                "com.google.Chrome",
                Some(456),
            )),
        )
        .unwrap();

        assert_eq!(frontmost.app.bundle_id, "com.openai.codex");
        assert_eq!(frontmost.pid, Some(123));
    }

    #[test]
    fn prompt_pick_session_uses_recent_target_without_considering_visible_apps() {
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(LastInputTarget {
                app: FrontmostApp {
                    name: "Codex".to_string(),
                    bundle_id: "com.openai.codex".to_string(),
                },
                pid: None,
                observed_at_ms: now_ms(),
                click_point: None,
            }),
        )
        .unwrap();

        assert_eq!(target.app.bundle_id, "com.openai.codex");
        assert!(target.click_point.is_none());
    }

    #[test]
    fn prompt_pick_session_uses_recent_target_when_prompt_picker_has_no_visible_app() {
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(LastInputTarget {
                app: FrontmostApp {
                    name: "WeChat".to_string(),
                    bundle_id: "com.tencent.xinWeChat".to_string(),
                },
                pid: Some(456),
                observed_at_ms: now_ms(),
                click_point: None,
            }),
        )
        .unwrap();

        assert_eq!(target.app.bundle_id, "com.tencent.xinWeChat");
        assert_eq!(target.pid, Some(456));
    }

    #[test]
    fn prompt_pick_session_uses_recent_target_when_picker_is_frontmost() {
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "Prompt Drawer",
                "local.promptpicker.dev",
                Some(1),
            )),
            Some(LastInputTarget {
                app: FrontmostApp {
                    name: "WeChat".to_string(),
                    bundle_id: "com.tencent.xinWeChat".to_string(),
                },
                pid: None,
                observed_at_ms: now_ms(),
                click_point: Some(TargetClickPoint { x: 400.0, y: 700.0 }),
            }),
        )
        .unwrap();

        assert_eq!(target.app.bundle_id, "com.tencent.xinWeChat");
        assert_eq!(target.click_point.unwrap().x, 400.0);
    }

    #[test]
    fn prompt_pick_session_uses_recent_target_when_frontmost_pid_is_picker_process() {
        // Regression: when PP's own popover becomes frontmost, lsappinfo may
        // report PP's PID (== std::process::id()) even if name/bundle look
        // foreign (binary launch mode). The function must NOT return None
        // early — it must fall through to the recent_target fallback so the
        // user's last used app (e.g. Codex) is picked.
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "UnknownApp",
                "unknown.bundle.id",
                Some(std::process::id()),
            )),
            Some(LastInputTarget {
                app: FrontmostApp {
                    name: "Codex".to_string(),
                    bundle_id: "com.openai.codex".to_string(),
                },
                pid: Some(456),
                observed_at_ms: now_ms(),
                click_point: None,
            }),
        )
        .unwrap();

        assert_eq!(target.app.bundle_id, "com.openai.codex");
        assert_eq!(target.pid, Some(456));
    }

    #[test]
    fn prompt_pick_session_rejects_visible_app_when_picker_pid_is_frontmost() {
        let target = prompt_pick_session_target(
            Some(frontmost_target(
                "UnknownApp",
                "unknown.bundle.id",
                Some(std::process::id()),
            )),
            None,
        );

        assert!(target.is_none());
    }

    #[test]
    fn autosend_without_last_target_copies_without_sending() {
        let state = PromptPickSessionState::default();
        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |_, _, _, _| panic!("app sender must not run without a target"),
            |body| {
                assert_eq!(body, "hello");
                Ok(())
            },
        );

        let outcome = result.unwrap();
        assert!(outcome.copied);
        assert!(!outcome.sent);
        assert_eq!(outcome.reason, Some(AutosendFailureReason::NoSafeTarget));
    }

    #[test]
    fn autosend_falls_back_to_recent_target_when_prompt_session_is_not_ready() {
        let session_state = PromptPickSessionState::default();
        let recent_state = LastInputTargetState::default();
        recent_state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: Some(TargetClickPoint { x: 640.0, y: 720.0 }),
        });

        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &session_state,
            Some(&recent_state),
            platform::macos::NativeSubmitKey::Enter,
            |body, bundle_id, click_point, submit_key| {
                assert_eq!(body, "hello");
                assert_eq!(bundle_id, "com.openai.codex");
                assert_eq!(click_point.unwrap().x, 640.0);
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when recent target is usable"),
        );

        let outcome = result.unwrap();
        assert!(outcome.sent);
        assert!(session_state.get().is_none());
    }

    #[test]
    fn stale_prompt_pick_session_capture_is_ignored_after_new_session_begins() {
        let session_state = PromptPickSessionState::default();
        session_state.begin(1);
        session_state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: None,
        });
        assert!(record_prompt_pick_session_target_if_valid(
            &session_state,
            PromptPickSessionTarget {
                app: FrontmostApp {
                    name: "WeChat".to_string(),
                    bundle_id: "com.tencent.xinWeChat".to_string(),
                },
                pid: Some(std::process::id()),
                observed_at_ms: now_ms(),
                click_point: None,
            },
            captured_identity("com.tencent.xinWeChat", std::process::id()),
            1,
        )
        .is_some());
        assert_eq!(
            session_state.get().unwrap().app.bundle_id,
            "com.tencent.xinWeChat"
        );

        session_state.begin(2);
        assert!(record_prompt_pick_session_target_if_valid(
            &session_state,
            PromptPickSessionTarget {
                app: FrontmostApp {
                    name: "WeChat".to_string(),
                    bundle_id: "com.tencent.xinWeChat".to_string(),
                },
                pid: Some(std::process::id()),
                observed_at_ms: now_ms(),
                click_point: None,
            },
            captured_identity("com.tencent.xinWeChat", std::process::id()),
            1,
        )
        .is_none());
        assert!(session_state.get().is_none());

        let recent_state = LastInputTargetState::default();
        recent_state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: None,
        });

        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &session_state,
            Some(&recent_state),
            platform::macos::NativeSubmitKey::Enter,
            |_, bundle_id, _, submit_key| {
                assert_eq!(bundle_id, "com.openai.codex");
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when recent target is usable"),
        );

        assert!(result.unwrap().sent);
    }

    #[test]
    fn autosend_session_target_uses_app_sender_with_click_point() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: Some(TargetClickPoint { x: 420.0, y: 720.0 }),
        });

        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |body, bundle_id, click_point, submit_key| {
                assert_eq!(body, "hello");
                assert_eq!(bundle_id, "com.tencent.xinWeChat");
                assert_eq!(click_point.unwrap().y, 720.0);
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run for a safe app-only target"),
        );

        let outcome = result.unwrap();
        assert!(outcome.copied);
        assert!(outcome.sent);
        assert!(state.get().is_none());
    }

    #[test]
    fn autosend_session_target_does_not_use_click_point() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |body, bundle_id, click_point, submit_key| {
                assert_eq!(body, "hello");
                assert_eq!(bundle_id, "com.openai.codex");
                assert!(click_point.is_none());
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when a click point exists"),
        );

        let outcome = result.unwrap();
        assert!(outcome.copied);
        assert!(outcome.sent);
    }

    #[test]
    fn autosend_reports_app_sender_failure_without_returning_err() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        let result = paste_prompt_and_submit_to_session_target_with_senders(
            "hello",
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |_, _, _, _| AutosendOutcome::paste_event_failed("app paste failed".to_string()),
            |_| panic!("copy sender must not run when a click point exists"),
        );

        let outcome = result.unwrap();
        assert!(outcome.copied);
        assert!(!outcome.sent);
        assert_eq!(outcome.error.as_deref(), Some("app paste failed"));
        assert_eq!(
            outcome.reason,
            Some(AutosendFailureReason::PasteEventFailed)
        );
    }

    #[test]
    fn autosend_sequence_uses_one_session_target_for_all_bodies() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });
        let bodies = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        let mut sent: Vec<(String, String)> = Vec::new();
        let mut sleeps = Vec::new();

        let result = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |body, bundle_id, _, submit_key| {
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                sent.push((body.to_string(), bundle_id.to_string()));
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when target exists"),
            |delay_ms| sleeps.push(delay_ms),
        )
        .unwrap();

        assert!(result.sent);
        assert_eq!(result.sent_count, 3);
        assert!(state.get().is_none());
        assert_eq!(
            sent,
            vec![
                ("one".to_string(), "com.tencent.xinWeChat".to_string()),
                ("two".to_string(), "com.tencent.xinWeChat".to_string()),
                ("three".to_string(), "com.tencent.xinWeChat".to_string()),
            ]
        );
        assert_eq!(sleeps, vec![700, 700]);
    }

    #[test]
    fn paste_only_sequence_joins_bodies_and_reports_processed_without_submit() {
        let state = PromptPickSessionState::default();
        state.set(prompt_target(
            "Codex",
            "com.openai.codex",
            Some(std::process::id()),
        ));
        let bodies = vec!["first".to_string(), "second".to_string()];
        let outcome = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &state,
            None,
            platform::macos::NativeSubmitKey::None,
            |body, bundle_id, _, submit_key| {
                assert_eq!(body, "first\n\nsecond");
                assert_eq!(bundle_id, "com.openai.codex");
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::None);
                AutosendOutcome::pasted_only()
            },
            |_| panic!("copy fallback must not run"),
            |_| panic!("joined paste-only group must not sleep between bodies"),
        )
        .unwrap();

        assert!(!outcome.sent);
        assert_eq!(outcome.sent_count, 0);
        assert_eq!(outcome.processed_count, 2);
        assert_eq!(
            outcome.completion,
            Some(platform::AutosendCompletion::PastedOnly)
        );
        assert_eq!(outcome.failed_index, None);
    }

    #[test]
    fn unknown_wechat_group_effective_paste_only_keeps_every_body() {
        let state = PromptPickSessionState::default();
        state.set(prompt_target(
            "WeChat",
            "com.tencent.xinWeChat",
            Some(std::process::id()),
        ));
        let bodies = vec!["first".to_string(), "second".to_string()];
        let sender_calls = std::cell::Cell::new(0);

        let outcome = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &state,
            None,
            platform::macos::NativeSubmitKey::None,
            |body, bundle_id, _, submit_key| {
                sender_calls.set(sender_calls.get() + 1);
                assert_eq!(body, "first\n\nsecond");
                assert_eq!(bundle_id, "com.tencent.xinWeChat");
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::None);
                AutosendOutcome::pasted_only()
            },
            |_| panic!("copy fallback must not run"),
            |_| panic!("joined paste-only group must not sleep between bodies"),
        )
        .unwrap();

        assert_eq!(sender_calls.get(), 1);
        assert_eq!(outcome.processed_count, bodies.len());
        assert_eq!(outcome.sent_count, 0);
        assert_eq!(
            outcome.completion,
            Some(platform::AutosendCompletion::PastedOnly)
        );
        assert_eq!(outcome.failed_index, None);
    }

    #[test]
    fn paste_only_sequence_success_is_not_serialized_as_a_failure() {
        let outcome = AutosendSequenceOutcome::from_paste_only(AutosendOutcome::pasted_only(), 3);

        assert!(outcome.copied);
        assert!(!outcome.sent);
        assert_eq!(outcome.processed_count, 3);
        assert_eq!(outcome.failed_index, None);
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.reason, None);
    }

    #[test]
    fn autosend_sequence_clamps_interval_to_milliseconds_range() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut sleeps = Vec::new();

        paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            10,
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |_, _, _, _| AutosendOutcome::sent(),
            |_| panic!("copy sender must not run when target exists"),
            |delay_ms| sleeps.push(delay_ms),
        )
        .unwrap();

        assert_eq!(sleeps, vec![MIN_SEQUENCE_INTERVAL_MS]);
    }

    #[test]
    fn autosend_sequence_stops_at_first_failed_body() {
        let state = PromptPickSessionState::default();
        state.set(PromptPickSessionTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });
        let bodies = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        let mut sent = Vec::new();
        let mut sleeps = Vec::new();

        let result = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |body, _, _, _| {
                sent.push(body.to_string());
                if body == "two" {
                    return AutosendOutcome::paste_event_failed("paste failed".to_string());
                }
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when target exists"),
            |delay_ms| sleeps.push(delay_ms),
        )
        .unwrap();

        assert!(!result.sent);
        assert_eq!(result.sent_count, 1);
        assert_eq!(result.failed_index, Some(2));
        assert_eq!(sent, vec!["one".to_string(), "two".to_string()]);
        assert_eq!(sleeps, vec![700]);
    }

    #[test]
    fn autosend_sequence_without_target_copies_first_body_without_sending() {
        let state = PromptPickSessionState::default();
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut copied = String::new();

        let result = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &state,
            None,
            platform::macos::NativeSubmitKey::Enter,
            |_, _, _, _| panic!("app sender must not run without target"),
            |body| {
                copied = body.to_string();
                Ok(())
            },
            |_| panic!("sleeper must not run without target"),
        )
        .unwrap();

        assert_eq!(copied, "one");
        assert!(!result.sent);
        assert_eq!(result.sent_count, 0);
        assert_eq!(result.failed_index, Some(1));
        assert_eq!(result.reason, Some(AutosendFailureReason::NoSafeTarget));
    }

    #[test]
    fn autosend_sequence_falls_back_to_recent_target_when_prompt_session_is_not_ready() {
        let session_state = PromptPickSessionState::default();
        let recent_state = LastInputTargetState::default();
        recent_state.set(LastInputTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: now_ms(),
            click_point: None,
        });
        let bodies = vec!["one".to_string(), "two".to_string()];
        let mut sent = Vec::new();

        let result = paste_prompt_sequence_and_submit_to_session_target_with_senders(
            &bodies,
            700,
            &session_state,
            Some(&recent_state),
            platform::macos::NativeSubmitKey::Enter,
            |body, bundle_id, click_point, submit_key| {
                assert_eq!(submit_key, platform::macos::NativeSubmitKey::Enter);
                sent.push((body.to_string(), bundle_id.to_string(), click_point));
                AutosendOutcome::sent()
            },
            |_| panic!("copy sender must not run when recent target is usable"),
            |_| {},
        )
        .unwrap();

        assert!(result.sent);
        assert_eq!(result.sent_count, 2);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].1, "com.tencent.xinWeChat");
        assert!(sent[0].2.is_none());
    }

    #[test]
    fn accepts_non_codex_target_for_autosend() {
        let state = LastInputTargetState::default();
        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        assert_eq!(
            last_target_bundle_id(&state).unwrap(),
            "com.tencent.xinWeChat"
        );
    }

    #[test]
    fn last_target_for_wechat_does_not_need_click_point() {
        let state = LastInputTargetState::default();
        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "WeChat".to_string(),
                bundle_id: "com.tencent.xinWeChat".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        assert_eq!(
            last_target_bundle_id(&state).unwrap(),
            "com.tencent.xinWeChat"
        );
    }

    #[test]
    fn accepts_codex_target_for_autosend() {
        let state = LastInputTargetState::default();
        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        assert_eq!(last_target_bundle_id(&state).unwrap(), "com.openai.codex");
    }

    #[test]
    fn autosend_does_not_require_click_point_for_codex() {
        let state = LastInputTargetState::default();
        state.set(LastInputTarget {
            app: FrontmostApp {
                name: "Codex".to_string(),
                bundle_id: "com.openai.codex".to_string(),
            },
            pid: None,
            observed_at_ms: 123,
            click_point: None,
        });

        assert_eq!(last_target_bundle_id(&state).unwrap(), "com.openai.codex");
    }

    #[test]
    fn records_frontmost_app_as_last_target_fallback() {
        let state = LastInputTargetState::default();
        let mut child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .expect("test helper process should start");
        let pid = child.id();
        record_last_app_if_valid(
            &state,
            frontmost_target("WeChat", "com.tencent.xinWeChat", Some(pid)),
        );

        assert_eq!(state.get().unwrap().app.bundle_id, "com.tencent.xinWeChat");
        assert_eq!(state.get().unwrap().pid, Some(pid));
        child.kill().expect("test helper process should stop");
        child.wait().expect("test helper process should be reaped");
    }

    #[test]
    fn skips_prompt_picker_as_frontmost_app_fallback() {
        let state = LastInputTargetState::default();
        record_last_app_if_valid(
            &state,
            frontmost_target("Prompt Drawer", "local.promptpicker.dev", Some(1)),
        );

        assert!(state.get().is_none());
    }
}

#[cfg(test)]
mod menu_bar_app_tests {
    use super::*;

    #[test]
    fn maps_tray_menu_item_ids_to_actions() {
        assert_eq!(
            tray_menu_action(TRAY_OPEN_MAIN_ID),
            TrayMenuAction::OpenMainWindow
        );
        assert_eq!(
            tray_menu_action(TRAY_SHOW_BUTTON_ID),
            TrayMenuAction::ShowFloatingButton
        );
        assert_eq!(
            tray_menu_action(TRAY_HIDE_BUTTON_ID),
            TrayMenuAction::HideFloatingButton
        );
        assert_eq!(
            tray_menu_action(TRAY_OPEN_ACCESSIBILITY_ID),
            TrayMenuAction::OpenAccessibilitySettings
        );
        assert_eq!(
            tray_menu_action(TRAY_OPEN_SETTINGS_ID),
            TrayMenuAction::OpenSettingsWindow
        );
        assert_eq!(tray_menu_action(TRAY_QUIT_ID), TrayMenuAction::Quit);
    }

    #[test]
    fn ignores_unknown_tray_menu_item_ids() {
        assert_eq!(tray_menu_action("unknown"), TrayMenuAction::Unknown);
    }

    #[test]
    fn menubar_template_icon_is_transparent_mask() {
        let icon = menubar_template_icon();

        assert_eq!(icon.width(), MENUBAR_TEMPLATE_ICON_SIZE);
        assert_eq!(icon.height(), MENUBAR_TEMPLATE_ICON_SIZE);
        assert_eq!(
            icon.rgba().len(),
            (MENUBAR_TEMPLATE_ICON_SIZE * MENUBAR_TEMPLATE_ICON_SIZE * 4) as usize
        );
        let alpha_values: std::collections::BTreeSet<u8> =
            icon.rgba().chunks_exact(4).map(|pixel| pixel[3]).collect();
        let opaque_pixels = icon
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] == 255)
            .count();

        assert!(alpha_values.contains(&0));
        assert!(alpha_values.contains(&255));
        assert!(alpha_values
            .iter()
            .all(|alpha| *alpha == 0 || *alpha == 255));
        assert!((60..=220).contains(&opaque_pixels));
    }

    #[test]
    fn default_settings_tracks_accessibility_prompt_history() {
        let settings = default_settings_value();

        assert_eq!(
            settings.pointer("/permissions/accessibilityPromptRequested"),
            Some(&serde_json::Value::Bool(false))
        );
    }

    #[test]
    fn permission_status_does_not_require_accessibility_on_non_macos() {
        let status = prompt_interaction_permission_status_from_parts(
            false,
            false,
            false,
            "zh-CN".to_string(),
        );

        assert!(!status.required);
        assert!(status.trusted);
        assert!(!status.native_prompt_requested);
        assert_eq!(status.language, "zh-CN");
    }

    #[test]
    fn permission_status_reports_untrusted_macos_prompt_state() {
        let status =
            prompt_interaction_permission_status_from_parts(true, false, true, "en-US".to_string());

        assert!(status.required);
        assert!(!status.trusted);
        assert!(status.native_prompt_requested);
        assert_eq!(status.language, "en-US");
    }

    #[test]
    fn macos_info_plist_marks_app_as_menu_bar_app() {
        let info_plist = include_str!("../Info.plist");

        assert!(info_plist.contains("<key>LSUIElement</key>"));
        assert!(info_plist.contains("<true/>"));
    }

    #[test]
    fn macos_info_plist_declares_apple_events_usage() {
        let info_plist = include_str!("../Info.plist");

        assert!(info_plist.contains("<key>NSAppleEventsUsageDescription</key>"));
        assert!(info_plist.contains("send keyboard events"));
    }

    #[test]
    fn tauri_capabilities_do_not_allow_blocking_message_dialogs() {
        let capabilities = include_str!("../capabilities/default.json");

        assert!(!capabilities.contains("\"dialog:allow-message\""));
    }

    #[test]
    fn prompt_button_monitor_builds_when_enabled_button_is_missing() {
        assert_eq!(
            prompt_button_ensure_action(true, false, false, false),
            PromptButtonEnsureAction::BuildMissing
        );
    }

    #[test]
    fn prompt_button_monitor_shows_enabled_hidden_button() {
        assert_eq!(
            prompt_button_ensure_action(true, true, false, true),
            PromptButtonEnsureAction::ShowExisting
        );
    }

    #[test]
    fn prompt_button_monitor_leaves_enabled_visible_button_alone() {
        assert_eq!(
            prompt_button_ensure_action(true, true, true, true),
            PromptButtonEnsureAction::None
        );
    }

    #[test]
    fn prompt_button_monitor_keeps_unready_window_hidden() {
        assert_eq!(
            prompt_button_ensure_action(true, true, false, false),
            PromptButtonEnsureAction::None
        );
    }

    #[test]
    fn prompt_button_monitor_never_revives_user_disabled_button() {
        assert_eq!(
            prompt_button_ensure_action(false, false, false, false),
            PromptButtonEnsureAction::None
        );
        assert_eq!(
            prompt_button_ensure_action(false, true, false, true),
            PromptButtonEnsureAction::None
        );
    }

    #[test]
    fn disabled_visibility_wins_over_an_in_flight_show() {
        let state = PromptButtonVisibilityState::new(true);
        let show_generation = state.generation();
        state.set(false);

        assert!(!state.may_show(show_generation));
    }

    #[test]
    fn saved_visibility_initializes_state_once() {
        let settings = serde_json::json!({ "floatingButton": { "visible": false } });
        let state = PromptButtonVisibilityState::from_settings(&settings);

        assert!(!state.desired_visible());
    }

    fn temporary_settings_path(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("prompt-picker-{name}-{nonce}"))
            .join("settings.json")
    }

    #[test]
    fn malformed_frontend_settings_leave_existing_file_intact() {
        let path = temporary_settings_path("malformed");
        let state = SettingsFileState::new(path.clone());
        let visibility = PromptButtonVisibilityState::new(true);
        state
            .write_value_unlocked(&serde_json::json!({ "preserved": true }))
            .unwrap();

        assert!(state.write_frontend_text("{", &visibility).is_err());
        assert_eq!(state.read_value()["preserved"], true);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn frontend_settings_write_preserves_unknown_fields_and_native_visibility() {
        let path = temporary_settings_path("unknown-fields");
        let state = SettingsFileState::new(path.clone());
        let visibility = PromptButtonVisibilityState::new(false);

        state
            .write_frontend_text(
                r#"{"version":1,"future":{"enabled":true},"floatingButton":{"visible":true}}"#,
                &visibility,
            )
            .unwrap();

        let saved = state.read_value();
        assert_eq!(
            saved.pointer("/future/enabled"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            saved.pointer("/floatingButton/visible"),
            Some(&serde_json::json!(false))
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn accessibility_patch_does_not_overwrite_visibility() {
        let path = temporary_settings_path("permission-patch");
        let state = SettingsFileState::new(path.clone());
        state
            .patch_bool(&["floatingButton", "visible"], false)
            .unwrap();

        set_accessibility_prompt_requested(&state, true).unwrap();

        let saved = state.read_value();
        assert_eq!(
            saved.pointer("/floatingButton/visible"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            saved.pointer("/permissions/accessibilityPromptRequested"),
            Some(&serde_json::json!(true))
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn settings_write_creates_missing_app_data_directory() {
        let path = temporary_settings_path("missing-directory");
        let state = SettingsFileState::new(path.clone());

        state
            .patch_bool(&["floatingButton", "visible"], true)
            .unwrap();

        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
