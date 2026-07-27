use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventMask};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const BUTTON_WINDOW_LABEL: &str = "prompt-button";
pub const BUTTON_INPUT_WINDOW_LABEL: &str = "prompt-button-input";
pub const POPOVER_WINDOW_LABEL: &str = "prompt-popover";

pub const BUTTON_VISUAL_WIDTH: f64 = 132.0;
pub const BUTTON_VISUAL_HEIGHT: f64 = 132.0;
pub const BUTTON_WINDOW_WIDTH: f64 = 288.0;
pub const BUTTON_WINDOW_HEIGHT: f64 = 288.0;
pub const BUTTON_WINDOW_PADDING_X: f64 = (BUTTON_WINDOW_WIDTH - BUTTON_VISUAL_WIDTH) / 2.0;
pub const BUTTON_WINDOW_PADDING_Y: f64 = (BUTTON_WINDOW_HEIGHT - BUTTON_VISUAL_HEIGHT) / 2.0;
pub const BUTTON_WINDOW_TRANSPARENT: bool = true;
pub const POPOVER_WIDTH: f64 = 280.0;
pub const POPOVER_HEIGHT: f64 = 432.0;
pub const POPOVER_WINDOW_PADDING: f64 = 0.0;
pub const POPOVER_WINDOW_WIDTH: f64 = POPOVER_WIDTH;
pub const POPOVER_WINDOW_HEIGHT: f64 = POPOVER_HEIGHT;
pub const BUTTON_CONTROLS_WIDTH: f64 = 156.0;
pub const BUTTON_CONTROLS_HEIGHT: f64 = 72.0;
pub const POPOVER_GAP: f64 = 4.0;

static OUTSIDE_CLICK_MONITOR_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static OUTSIDE_CLICK_MONITOR_INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PopoverWindowAction {
    ReuseExisting,
    BuildMissing,
}

fn popover_window_action(
    existing_mode: Option<&str>,
    _requested_mode: &str,
) -> PopoverWindowAction {
    if existing_mode.is_some() {
        PopoverWindowAction::ReuseExisting
    } else {
        PopoverWindowAction::BuildMissing
    }
}

struct PendingPopoverModeRequest {
    id: u64,
    mode: String,
    sender: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Default)]
struct PopoverModeRequestInner {
    next_id: u64,
    pending: Option<PendingPopoverModeRequest>,
}

#[derive(Default)]
pub(crate) struct PopoverModeRequestState {
    inner: std::sync::Mutex<PopoverModeRequestInner>,
}

impl PopoverModeRequestState {
    fn begin(&self, mode: &str) -> (u64, tokio::sync::oneshot::Receiver<()>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut inner = self.inner.lock().expect("popover request lock poisoned");
        inner.next_id = inner.next_id.wrapping_add(1);
        let id = inner.next_id;
        inner.pending = Some(PendingPopoverModeRequest {
            id,
            mode: mode.to_string(),
            sender: Some(sender),
        });
        (id, receiver)
    }

    fn acknowledge(&self, id: u64, mode: &str) -> bool {
        let sender = {
            let mut inner = self.inner.lock().expect("popover request lock poisoned");
            let Some(pending) = inner.pending.as_mut() else {
                return false;
            };
            if pending.id != id || pending.mode != mode {
                return false;
            }
            pending.sender.take()
        };
        sender.is_some_and(|sender| sender.send(()).is_ok())
    }

    fn complete_if_current(&self, id: u64, mode: &str) -> bool {
        let mut inner = self.inner.lock().expect("popover request lock poisoned");
        let is_current = inner
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id && pending.mode == mode);
        if is_current {
            inner.pending = None;
        }
        is_current
    }

    fn cancel_if_current(&self, id: u64) -> bool {
        let mut inner = self.inner.lock().expect("popover request lock poisoned");
        if inner
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            inner.next_id = inner.next_id.wrapping_add(1);
            inner.pending = None;
            return true;
        }
        false
    }

    fn cancel(&self) -> bool {
        let mut inner = self.inner.lock().expect("popover request lock poisoned");
        inner.next_id = inner.next_id.wrapping_add(1);
        inner.pending.take().is_some()
    }

    fn is_pending_mode(&self, mode: &str) -> bool {
        self.inner
            .lock()
            .expect("popover request lock poisoned")
            .pending
            .as_ref()
            .is_some_and(|pending| pending.mode == mode)
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PopoverModeRequestPayload {
    request_id: u64,
    mode: String,
}

#[derive(Clone, Copy)]
struct MonitorBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Copy)]
struct WindowRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Copy)]
struct PopoverSize {
    width: f64,
    height: f64,
}

fn point_inside_rect(point: (f64, f64), rect: WindowRect) -> bool {
    point.0 >= rect.x
        && point.0 <= rect.x + rect.width
        && point.1 >= rect.y
        && point.1 <= rect.y + rect.height
}

fn should_dismiss_popover_for_click(
    point: (f64, f64),
    button: Option<WindowRect>,
    popover: Option<WindowRect>,
) -> bool {
    if popover.is_none() {
        return false;
    }
    if button.is_some_and(|rect| point_inside_rect(point, rect)) {
        return false;
    }
    if popover.is_some_and(|rect| point_inside_rect(point, rect)) {
        return false;
    }
    true
}

fn bottom_left_to_top_left_point(point: (f64, f64), bounds: MonitorBounds) -> (f64, f64) {
    (point.0, bounds.y + bounds.height - point.1)
}

fn logical_position(x: f64, y: f64) -> tauri::Position {
    tauri::Position::Logical(tauri::LogicalPosition { x, y })
}

fn prompt_button_visual_to_window_position(x: f64, y: f64) -> (f64, f64) {
    (x - BUTTON_WINDOW_PADDING_X, y - BUTTON_WINDOW_PADDING_Y)
}

fn prompt_button_window_to_visual_position(x: f64, y: f64) -> (f64, f64) {
    (x + BUTTON_WINDOW_PADDING_X, y + BUTTON_WINDOW_PADDING_Y)
}

fn prompt_button_position_from_visual(x: f64, y: f64) -> tauri::Position {
    let (window_x, window_y) = prompt_button_visual_to_window_position(x, y);
    logical_position(window_x, window_y)
}

fn monitor_bounds(monitor: &tauri::Monitor) -> MonitorBounds {
    let scale = monitor.scale_factor();
    MonitorBounds {
        x: monitor.position().x as f64 / scale,
        y: monitor.position().y as f64 / scale,
        width: monitor.size().width as f64 / scale,
        height: monitor.size().height as f64 / scale,
    }
}

fn clamp_button_position_in_bounds(x: f64, y: f64, bounds: Option<MonitorBounds>) -> (f64, f64) {
    let Some(bounds) = bounds else {
        return (x, y);
    };

    let margin = 8.0;
    let min_x = bounds.x + margin;
    let min_y = bounds.y + margin;
    let max_x = bounds.x + bounds.width - BUTTON_VISUAL_WIDTH - margin;
    let max_y = bounds.y + bounds.height - BUTTON_VISUAL_HEIGHT - margin;

    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

fn clamp_button_position_for_monitor(
    x: f64,
    y: f64,
    monitor: Option<&tauri::Monitor>,
) -> (f64, f64) {
    clamp_button_position_in_bounds(x, y, monitor.map(monitor_bounds))
}

fn positions_are_close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
}

fn popover_mode_state() -> &'static std::sync::Mutex<Option<String>> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
        std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(None))
}

fn current_popover_mode() -> Option<String> {
    popover_mode_state()
        .lock()
        .expect("popover mode lock poisoned")
        .clone()
}

fn set_popover_mode(mode: Option<&str>) {
    *popover_mode_state()
        .lock()
        .expect("popover mode lock poisoned") = mode.map(str::to_string);
}

fn set_outside_click_monitor_active(active: bool) {
    OUTSIDE_CLICK_MONITOR_ACTIVE.store(active, Ordering::SeqCst);
}

fn cancel_pending_popover_request(app: &tauri::AppHandle) -> bool {
    app.state::<PopoverModeRequestState>().cancel()
}

fn should_reuse_popover(existing_mode: Option<&str>, requested_mode: &str) -> bool {
    existing_mode == Some(requested_mode)
}

fn should_close_prompt_popover_on_toggle(existing_mode: Option<&str>, is_visible: bool) -> bool {
    existing_mode == Some("popover") && is_visible
}

fn popover_size_for_mode(mode: &str) -> PopoverSize {
    if mode == "button-controls" {
        return PopoverSize {
            width: BUTTON_CONTROLS_WIDTH,
            height: BUTTON_CONTROLS_HEIGHT,
        };
    }

    PopoverSize {
        width: POPOVER_WIDTH,
        height: POPOVER_HEIGHT,
    }
}

fn popover_window_padding_for_mode(mode: &str) -> f64 {
    if mode == "popover" {
        POPOVER_WINDOW_PADDING
    } else {
        0.0
    }
}

fn popover_window_size_for_mode(mode: &str) -> PopoverSize {
    if mode == "popover" {
        return PopoverSize {
            width: POPOVER_WINDOW_WIDTH,
            height: POPOVER_WINDOW_HEIGHT,
        };
    }

    popover_size_for_mode(mode)
}

fn popover_window_position_from_visual_position(x: f64, y: f64, mode: &str) -> (f64, f64) {
    let padding = popover_window_padding_for_mode(mode);
    (x - padding, y - padding)
}

fn visual_popover_rect_from_window_rect(rect: WindowRect, mode: &str) -> WindowRect {
    let padding = popover_window_padding_for_mode(mode);
    WindowRect {
        x: rect.x + padding,
        y: rect.y + padding,
        width: rect.width - (padding * 2.0),
        height: rect.height - (padding * 2.0),
    }
}

fn should_use_transparent_popover_window(mode: Option<&str>) -> bool {
    matches!(mode, Some("popover" | "button-controls"))
}

fn emit_popover_opened(app: &tauri::AppHandle, mode: &str) {
    let _ = app.emit_to(POPOVER_WINDOW_LABEL, "prompt-popover-opened", mode);
}

fn emit_popover_dismissed(app: &tauri::AppHandle) {
    let _ = app.emit("prompt-popover-dismissed", ());
}

fn window_rect(app: &tauri::AppHandle, label: &str) -> Option<WindowRect> {
    let window = app.get_webview_window(label)?;
    if !window.is_visible().ok()? {
        return None;
    }

    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);

    Some(WindowRect {
        x: position.x as f64 / scale,
        y: position.y as f64 / scale,
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
    })
}

fn visual_button_rect_from_window_rect(rect: WindowRect) -> WindowRect {
    let (x, y) = prompt_button_window_to_visual_position(rect.x, rect.y);
    WindowRect {
        x,
        y,
        width: BUTTON_VISUAL_WIDTH,
        height: BUTTON_VISUAL_HEIGHT,
    }
}

#[cfg(target_os = "macos")]
fn current_mouse_point_for_popover(app: &tauri::AppHandle) -> Option<(f64, f64)> {
    let popover = app.get_webview_window(POPOVER_WINDOW_LABEL)?;
    let monitor = popover
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let bounds = monitor_bounds(&monitor);
    let point = NSEvent::mouseLocation();

    Some(bottom_left_to_top_left_point((point.x, point.y), bounds))
}

#[cfg(target_os = "macos")]
fn handle_prompt_popover_outside_click(app: tauri::AppHandle) {
    if !OUTSIDE_CLICK_MONITOR_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let current_mode = current_popover_mode();
    if !should_use_transparent_popover_window(current_mode.as_deref()) {
        set_outside_click_monitor_active(false);
        return;
    }

    let Some(popover_window) = app.get_webview_window(POPOVER_WINDOW_LABEL) else {
        set_outside_click_monitor_active(false);
        return;
    };
    if !popover_window.is_visible().unwrap_or(false) {
        set_outside_click_monitor_active(false);
        return;
    }

    let Some(point) = current_mouse_point_for_popover(&app) else {
        return;
    };
    let button = window_rect(&app, BUTTON_WINDOW_LABEL).map(visual_button_rect_from_window_rect);
    let popover = window_rect(&app, POPOVER_WINDOW_LABEL).map(|rect| {
        visual_popover_rect_from_window_rect(rect, current_mode.as_deref().unwrap_or_default())
    });
    if should_dismiss_popover_for_click(point, button, popover) {
        let _ = popover_window.hide();
        set_outside_click_monitor_active(false);
        emit_popover_dismissed(&app);
    }
}

#[cfg(target_os = "macos")]
fn retain_event_monitor(monitor: Retained<AnyObject>) {
    let _ = Retained::into_raw(monitor);
}

#[cfg(target_os = "macos")]
fn install_prompt_popover_outside_click_monitor(app: &tauri::AppHandle) -> Result<(), String> {
    if OUTSIDE_CLICK_MONITOR_INSTALLED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let dispatch_app = app.clone();
    let monitor_app = app.clone();
    let result = crate::macos_panels::run_on_main_thread_sync(&dispatch_app, move || {
        install_prompt_popover_outside_click_monitor_on_main_thread(&monitor_app);
        Ok(())
    });
    if result.is_err() {
        OUTSIDE_CLICK_MONITOR_INSTALLED.store(false, Ordering::SeqCst);
    }
    result
}

#[cfg(target_os = "macos")]
fn install_prompt_popover_outside_click_monitor_on_main_thread(app: &tauri::AppHandle) {
    let mask =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;

    let global_app = app.clone();
    let global_block = RcBlock::new(move |_event: std::ptr::NonNull<NSEvent>| {
        handle_prompt_popover_outside_click(global_app.clone());
    });
    if let Some(monitor) =
        NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global_block)
    {
        retain_event_monitor(monitor);
    }

    let local_app = app.clone();
    let local_block = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        handle_prompt_popover_outside_click(local_app.clone());
        event.as_ptr()
    });
    if let Some(monitor) =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local_block) }
    {
        retain_event_monitor(monitor);
    }
}

#[cfg(not(target_os = "macos"))]
fn install_prompt_popover_outside_click_monitor(_app: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

fn enable_prompt_popover_outside_click_monitor(app: &tauri::AppHandle) -> Result<(), String> {
    install_prompt_popover_outside_click_monitor(app)?;
    set_outside_click_monitor_active(true);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct PromptButtonPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(serde::Serialize)]
pub struct PromptPopoverToggleOutcome {
    pub opened: bool,
}

#[tauri::command]
pub fn prompt_button_position_cmd(
    app: tauri::AppHandle,
) -> Result<Option<PromptButtonPosition>, String> {
    let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) else {
        return Ok(None);
    };
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let (x, y) = prompt_button_window_to_visual_position(
        position.x as f64 / scale,
        position.y as f64 / scale,
    );
    Ok(Some(PromptButtonPosition { x, y }))
}

#[tauri::command]
pub fn move_prompt_button_to(x: f64, y: f64, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) {
        let monitor = window
            .current_monitor()
            .map_err(|e| e.to_string())?
            .or(app.primary_monitor().map_err(|e| e.to_string())?);
        let (x, y) = clamp_button_position_for_monitor(x, y, monitor.as_ref());
        window
            .set_position(prompt_button_position_from_visual(x, y))
            .map_err(|e| e.to_string())?;
        ensure_prompt_button_input_window(&app, x, y)?;
    }
    Ok(())
}

async fn show_popover_mode(
    x: f64,
    y: f64,
    mode: &str,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let popover_size = popover_window_size_for_mode(mode);
    let (window_x, window_y) = popover_window_position_from_visual_position(x, y, mode);
    if let Some(window) = app.get_webview_window(POPOVER_WINDOW_LABEL) {
        let existing_mode = current_popover_mode();
        debug_assert_eq!(
            popover_window_action(Some(existing_mode.as_deref().unwrap_or("")), mode),
            PopoverWindowAction::ReuseExisting
        );
        if should_reuse_popover(existing_mode.as_deref(), mode) {
            window
                .set_position(logical_position(window_x, window_y))
                .map_err(|e| e.to_string())?;
            window
                .set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: popover_size.width,
                    height: popover_size.height,
                }))
                .map_err(|e| e.to_string())?;
            if should_use_transparent_popover_window(Some(mode)) {
                crate::macos_panels::configure_transparent_webview_window(&window)?;
                enable_prompt_popover_outside_click_monitor(app)?;
            } else {
                set_outside_click_monitor_active(false);
            }
            show_non_activating_overlay_window(&window)?;
            emit_popover_opened(app, mode);
            return Ok(());
        }

        window.hide().map_err(|e| e.to_string())?;
        set_outside_click_monitor_active(false);
        set_popover_mode(Some(mode));
        window
            .set_position(logical_position(window_x, window_y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: popover_size.width,
                height: popover_size.height,
            }))
            .map_err(|e| e.to_string())?;

        let request_state = app.state::<PopoverModeRequestState>();
        let (request_id, receiver) = request_state.begin(mode);
        let payload = PopoverModeRequestPayload {
            request_id,
            mode: mode.to_string(),
        };
        if let Err(error) = app.emit_to(
            POPOVER_WINDOW_LABEL,
            "prompt-popover-mode-requested",
            payload,
        ) {
            request_state.cancel_if_current(request_id);
            return Err(error.to_string());
        }

        match tokio::time::timeout(std::time::Duration::from_secs(1), receiver).await {
            Ok(Ok(())) if request_state.complete_if_current(request_id, mode) => {}
            Ok(Ok(())) => return Err("Prompt popover mode request was superseded.".to_string()),
            Ok(Err(_)) => return Err("Prompt popover mode request was superseded.".to_string()),
            Err(_) => {
                if request_state.cancel_if_current(request_id) {
                    set_outside_click_monitor_active(false);
                }
                return Err("Prompt popover mode request timed out.".to_string());
            }
        }

        if should_use_transparent_popover_window(Some(mode)) {
            crate::macos_panels::configure_transparent_webview_window(&window)?;
            enable_prompt_popover_outside_click_monitor(app)?;
        }
        show_non_activating_overlay_window(&window)?;
        emit_popover_opened(app, mode);
        return Ok(());
    }

    let window = build_prompt_popover_window(app, window_x, window_y, mode)?;
    if should_use_transparent_popover_window(Some(mode)) {
        crate::macos_panels::configure_transparent_webview_window(&window)?;
        enable_prompt_popover_outside_click_monitor(app)?;
    } else {
        set_outside_click_monitor_active(false);
    }
    show_non_activating_overlay_window(&window)?;
    set_popover_mode(Some(mode));
    emit_popover_opened(app, mode);
    Ok(())
}

fn build_prompt_popover_window(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
    mode: &str,
) -> Result<tauri::WebviewWindow, String> {
    let popover_size = popover_window_size_for_mode(mode);
    let url = format!("index.html?mode={}", mode);
    WebviewWindowBuilder::new(app, POPOVER_WINDOW_LABEL, WebviewUrl::App(url.into()))
        .title(crate::PRODUCT_NAME)
        .inner_size(popover_size.width, popover_size.height)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .accept_first_mouse(true)
        .skip_taskbar(true)
        .focusable(false)
        .visible(false)
        .position(x, y)
        .build()
        .map_err(|e| e.to_string())
}

pub(crate) fn prewarm_prompt_popover(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window(POPOVER_WINDOW_LABEL).is_some() {
        return Ok(());
    }

    let window = build_prompt_popover_window(app, 0.0, 0.0, "popover")?;
    crate::macos_panels::configure_non_activating_panel(&window)?;
    if should_use_transparent_popover_window(Some("popover")) {
        crate::macos_panels::configure_transparent_webview_window(&window)?;
    }
    set_popover_mode(Some("popover"));
    Ok(())
}

fn show_non_activating_overlay_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos_panels::show_non_activating_panel(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.show().map_err(|error| error.to_string())
    }
}

fn start_prompt_pick_session_capture(
    session_id: u64,
    session_state: crate::PromptPickSessionState,
    recent_state: crate::LastInputTargetState,
) {
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            // One full capture to establish identity and recent_state (runs
            // current_input_target() exactly once).
            crate::capture_prompt_pick_session_target(&session_state, &recent_state, session_id);
            // Lightweight refresh: re-read only the cheap missing pieces until the
            // identity is complete or the user picks. No current_input_target() and
            // no session-reset on retries.
            run_prompt_pick_session_capture_loop(session_id, &session_state, std::thread::sleep);
            session_state.finish_capture(session_id);
        })
        .await;
        if let Err(error) = result {
            eprintln!("Prompt pick session task failed: {error}");
        }
    });
}

/// Bounded refresh loop. Stops as soon as the captured identity is complete, the
/// session is consumed, or MAX_ATTEMPTS is reached. Each iteration re-reads only
/// the cheap missing fields (window, page_url) — never the full input traversal.
fn run_prompt_pick_session_capture_loop(
    session_id: u64,
    session_state: &crate::PromptPickSessionState,
    mut sleep: impl FnMut(std::time::Duration),
) {
    const MAX_ATTEMPTS: u32 = 10;
    const PACING: std::time::Duration = std::time::Duration::from_millis(250);

    for attempt in 0..MAX_ATTEMPTS {
        if session_state.target_capture_complete(session_id) {
            break;
        }
        if session_state.is_consumed(session_id) {
            break;
        }
        refresh_prompt_pick_session_capture(session_id, session_state);
        if attempt + 1 < MAX_ATTEMPTS {
            sleep(PACING);
        }
    }
}

/// Re-read only the cheap, possibly-missing pieces for the frozen target and
/// merge them via the existing guarded setters.
fn refresh_prompt_pick_session_capture(
    session_id: u64,
    session_state: &crate::PromptPickSessionState,
) {
    let Some(target) = session_state.get() else {
        return;
    };
    let Some(pid) = target.pid else {
        return;
    };
    let bundle_id = target.app.bundle_id.clone();
    if let Some(window) = crate::platform::macos::current_target_window_identity(pid) {
        session_state.set_window_if_current(session_id, window);
    }
    if let Some(page_url) = crate::platform::macos::active_browser_page_url(pid, &bundle_id) {
        session_state.set_page_url_if_current(session_id, &bundle_id, page_url);
    }
}

pub(crate) fn build_prompt_button_window(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
) -> Result<tauri::WebviewWindow, String> {
    let renderer_state = app.state::<crate::PromptButtonRendererState>();
    let renderer_instance_id = renderer_state.allocate_instance();
    let visual_health = app.state::<crate::visual_liveness::PromptButtonVisualHealthState>();
    visual_health.register_instance(renderer_instance_id, false, false);
    let monitor = app.primary_monitor().map_err(|e| e.to_string())?;
    let (x, y) = clamp_button_position_for_monitor(x, y, monitor.as_ref());
    let (window_x, window_y) = prompt_button_visual_to_window_position(x, y);
    let overlay_url = format!("overlay.html?rendererInstanceId={renderer_instance_id}");
    let window = match WebviewWindowBuilder::new(
        app,
        BUTTON_WINDOW_LABEL,
        WebviewUrl::App(overlay_url.into()),
    )
    .title(crate::PRODUCT_NAME)
    .inner_size(BUTTON_WINDOW_WIDTH, BUTTON_WINDOW_HEIGHT)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .accept_first_mouse(true)
    .skip_taskbar(true)
    .focusable(false)
    .visible(false)
    .position(window_x, window_y)
    .build()
    {
        Ok(window) => window,
        Err(error) => {
            visual_health.record_recovery_failure();
            return Err(error.to_string());
        }
    };

    let recovery_url = window.url().map_err(|error| error.to_string())?;
    app.state::<crate::PromptButtonRecoveryUrlState>()
        .store(recovery_url);

    let pair_result = (|| {
        if BUTTON_WINDOW_TRANSPARENT {
            crate::macos_panels::configure_transparent_webview_window(&window)?;
        }
        build_prompt_button_input_window(app, x, y)?;
        Ok::<(), String>(())
    })();
    if let Err(error) = pair_result {
        let _ = window.hide();
        let _ = window.destroy();
        if let Some(input) = app.get_webview_window(BUTTON_INPUT_WINDOW_LABEL) {
            let _ = input.hide();
            let _ = input.destroy();
        }
        visual_health.record_recovery_failure();
        return Err(error);
    }
    visual_health.commit_pair(renderer_instance_id);
    if renderer_state.is_ready() && visual_health.may_present(renderer_instance_id) {
        show_ready_prompt_button_window(app)?;
    }
    Ok(window)
}

pub(crate) fn destroy_prompt_button_pair<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    hide_prompt_button_pair(app)?;
    if let Some(window) = app.get_webview_window(BUTTON_INPUT_WINDOW_LABEL) {
        window.destroy().map_err(|error| error.to_string())?;
    }
    if let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) {
        window.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn build_prompt_button_input_window(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
) -> Result<tauri::WebviewWindow, String> {
    let window = WebviewWindowBuilder::new(
        app,
        BUTTON_INPUT_WINDOW_LABEL,
        WebviewUrl::App("overlay-interaction.html".into()),
    )
    .title("Sleepy Cat Input")
    .inner_size(BUTTON_VISUAL_WIDTH, BUTTON_VISUAL_HEIGHT)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .accept_first_mouse(true)
    .skip_taskbar(true)
    .focusable(false)
    .visible(false)
    .position(x, y)
    .build()
    .map_err(|e| e.to_string())?;

    if BUTTON_WINDOW_TRANSPARENT {
        crate::macos_panels::configure_transparent_webview_window(&window)?;
    }
    Ok(window)
}

fn ensure_prompt_button_input_window(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
) -> Result<tauri::WebviewWindow, String> {
    let window = match app.get_webview_window(BUTTON_INPUT_WINDOW_LABEL) {
        Some(window) => window,
        None => return build_prompt_button_input_window(app, x, y),
    };

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: BUTTON_VISUAL_WIDTH,
            height: BUTTON_VISUAL_HEIGHT,
        }))
        .map_err(|e| e.to_string())?;
    window
        .set_position(logical_position(x, y))
        .map_err(|e| e.to_string())?;
    Ok(window)
}

pub(crate) fn show_ready_prompt_button_window(app: &tauri::AppHandle) -> Result<(), String> {
    if app
        .try_state::<crate::visual_liveness::PromptButtonVisualHealthState>()
        .is_some_and(|state| !state.may_present_current())
    {
        return Ok(());
    }
    let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) else {
        return Err("Prompt button window is missing.".to_string());
    };
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let (x, y) = prompt_button_window_to_visual_position(
        position.x as f64 / scale,
        position.y as f64 / scale,
    );
    let input = ensure_prompt_button_input_window(app, x, y)?;
    // Re-check user visibility and health immediately before the atomic presentation
    // so a concurrent disable cannot be raced into showing the pair.
    if app
        .try_state::<crate::PromptButtonVisibilityState>()
        .is_some_and(|state| !state.desired_visible())
    {
        return Ok(());
    }
    if app
        .try_state::<crate::visual_liveness::PromptButtonVisualHealthState>()
        .is_some_and(|state| !state.may_present_current())
    {
        return Ok(());
    }
    crate::macos_panels::present_prompt_button_pair(app, &window, &input)?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    input
        .set_ignore_cursor_events(false)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn show_prompt_button(x: f64, y: f64, app: tauri::AppHandle) -> Result<(), String> {
    if app
        .try_state::<crate::PromptButtonVisibilityState>()
        .is_some_and(|state| !state.desired_visible())
    {
        return Ok(());
    }
    if app
        .try_state::<crate::visual_liveness::PromptButtonVisualHealthState>()
        .is_some_and(|state| !state.allow_show_or_record_position(x, y))
    {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) {
        let monitor = window
            .current_monitor()
            .map_err(|e| e.to_string())?
            .or(app.primary_monitor().map_err(|e| e.to_string())?);
        let (x, y) = clamp_button_position_for_monitor(x, y, monitor.as_ref());
        let position = window.outer_position().map_err(|e| e.to_string())?;
        let scale = window.scale_factor().unwrap_or(1.0);
        let current = prompt_button_window_to_visual_position(
            position.x as f64 / scale,
            position.y as f64 / scale,
        );
        let visible = window.is_visible().unwrap_or(false);

        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: BUTTON_WINDOW_WIDTH,
                height: BUTTON_WINDOW_HEIGHT,
            }))
            .map_err(|e| e.to_string())?;
        if !positions_are_close(current, (x, y)) {
            window
                .set_position(prompt_button_position_from_visual(x, y))
                .map_err(|e| e.to_string())?;
        }
        let input = ensure_prompt_button_input_window(&app, x, y)?;
        if visible && !input.is_visible().unwrap_or(false) {
            show_ready_prompt_button_window(&app)?;
        }
        if !visible {
            if app
                .try_state::<crate::PromptButtonVisibilityState>()
                .is_some_and(|state| !state.desired_visible())
            {
                return Ok(());
            }
            let renderer_state = app.state::<crate::PromptButtonRendererState>();
            if renderer_state.is_ready() {
                show_ready_prompt_button_window(&app)?;
            } else if renderer_state.request_resume_once() {
                app.emit_to(
                    BUTTON_WINDOW_LABEL,
                    "prompt-button-renderer-resume-requested",
                    (),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    } else {
        if app
            .try_state::<crate::PromptButtonVisibilityState>()
            .is_some_and(|state| !state.desired_visible())
        {
            return Ok(());
        }
        build_prompt_button_window(&app, x, y)?;
        Ok(())
    }
}

#[tauri::command]
pub fn hide_prompt_button(app: tauri::AppHandle) -> Result<(), String> {
    hide_prompt_button_pair(&app)
}

pub(crate) fn hide_prompt_button_pair<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(BUTTON_INPUT_WINDOW_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    if let Some(window) = app.get_webview_window(BUTTON_WINDOW_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn show_prompt_popover(x: f64, y: f64, app: tauri::AppHandle) -> Result<(), String> {
    show_popover_mode(x, y, "popover", &app).await
}

#[tauri::command]
pub fn hide_prompt_popover(app: tauri::AppHandle) -> Result<(), String> {
    cancel_pending_popover_request(&app);
    if let Some(window) = app.get_webview_window(POPOVER_WINDOW_LABEL) {
        let was_visible = window.is_visible().unwrap_or(false);
        window.hide().map_err(|e| e.to_string())?;
        set_outside_click_monitor_active(false);
        if was_visible {
            if let Some(health) =
                app.try_state::<crate::visual_liveness::PromptButtonVisualHealthState>()
            {
                health.extend_interaction(crate::visual_liveness::POPOVER_DISMISS_GRACE);
            }
            emit_popover_dismissed(&app);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn show_prompt_popover_from_button(
    session_id: u64,
    session_state: tauri::State<'_, crate::PromptPickSessionState>,
    recent_state: tauri::State<'_, crate::LastInputTargetState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let session_state = session_state.inner().clone();
    let recent_state = recent_state.inner().clone();
    crate::prepare_prompt_pick_session_target(&session_state, &recent_state, session_id);
    start_prompt_pick_session_capture(session_id, session_state, recent_state);
    let position = button_relative_popover_position(
        &app,
        BUTTON_VISUAL_WIDTH,
        BUTTON_VISUAL_HEIGHT,
        "popover",
    );
    show_popover_mode(position.0, position.1, "popover", &app).await
}

#[tauri::command]
pub async fn toggle_prompt_popover_from_button(
    session_id: u64,
    session_state: tauri::State<'_, crate::PromptPickSessionState>,
    recent_state: tauri::State<'_, crate::LastInputTargetState>,
    app: tauri::AppHandle,
) -> Result<PromptPopoverToggleOutcome, String> {
    if let Some(health) = app.try_state::<crate::visual_liveness::PromptButtonVisualHealthState>() {
        health.extend_interaction(crate::visual_liveness::POINTER_LEASE_TIMEOUT);
    }
    let request_state = app.state::<PopoverModeRequestState>();
    if request_state.is_pending_mode("popover") {
        request_state.cancel();
        if let Some(window) = app.get_webview_window(POPOVER_WINDOW_LABEL) {
            window.hide().map_err(|e| e.to_string())?;
        }
        set_outside_click_monitor_active(false);
        emit_popover_dismissed(&app);
        return Ok(PromptPopoverToggleOutcome { opened: false });
    }
    if let Some(window) = app.get_webview_window(POPOVER_WINDOW_LABEL) {
        let visible = window.is_visible().unwrap_or(false);
        if should_close_prompt_popover_on_toggle(current_popover_mode().as_deref(), visible) {
            session_state.begin(session_id);
            window.hide().map_err(|e| e.to_string())?;
            set_outside_click_monitor_active(false);
            emit_popover_dismissed(&app);
            return Ok(PromptPopoverToggleOutcome { opened: false });
        }
    }

    let session_state = session_state.inner().clone();
    let recent_state = recent_state.inner().clone();
    crate::prepare_prompt_pick_session_target(&session_state, &recent_state, session_id);
    start_prompt_pick_session_capture(session_id, session_state, recent_state);
    let position = button_relative_popover_position(
        &app,
        BUTTON_VISUAL_WIDTH,
        BUTTON_VISUAL_HEIGHT,
        "popover",
    );
    show_popover_mode(position.0, position.1, "popover", &app).await?;
    Ok(PromptPopoverToggleOutcome { opened: true })
}

#[tauri::command]
pub async fn show_prompt_button_controls_from_button(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(health) = app.try_state::<crate::visual_liveness::PromptButtonVisualHealthState>() {
        health.extend_interaction(crate::visual_liveness::POINTER_LEASE_TIMEOUT);
    }
    let position = button_relative_popover_position(
        &app,
        BUTTON_VISUAL_WIDTH,
        BUTTON_VISUAL_HEIGHT,
        "button-controls",
    );
    show_popover_mode(position.0, position.1, "button-controls", &app).await
}

#[tauri::command]
pub fn acknowledge_prompt_popover_mode(
    request_id: u64,
    mode: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if app
        .state::<PopoverModeRequestState>()
        .acknowledge(request_id, &mode)
    {
        Ok(())
    } else {
        Err("Prompt popover mode request is no longer current.".to_string())
    }
}

fn button_relative_popover_position(
    app: &tauri::AppHandle,
    button_width: f64,
    button_height: f64,
    mode: &str,
) -> (f64, f64) {
    let popover_size = popover_size_for_mode(mode);
    let window_padding = popover_window_padding_for_mode(mode);
    app.get_webview_window(BUTTON_WINDOW_LABEL)
        .and_then(|window| {
            let position = window.outer_position().ok()?;
            let scale = window.scale_factor().unwrap_or(1.0);
            let native_x = position.x as f64 / scale;
            let native_y = position.y as f64 / scale;
            let (button_x, button_y) = prompt_button_window_to_visual_position(native_x, native_y);
            let monitor = window.current_monitor().ok().flatten();
            Some(clamp_popover_position_for_size(
                button_x,
                button_y,
                button_width,
                button_height,
                popover_size,
                window_padding,
                monitor.as_ref(),
            ))
        })
        .unwrap_or((100.0, 100.0))
}

fn clamp_popover_position_for_size(
    button_x: f64,
    button_y: f64,
    button_width: f64,
    button_height: f64,
    popover_size: PopoverSize,
    window_padding: f64,
    monitor: Option<&tauri::Monitor>,
) -> (f64, f64) {
    let Some(monitor) = monitor else {
        return clamp_popover_position_in_bounds_for_size(
            button_x,
            button_y,
            button_width,
            button_height,
            popover_size,
            window_padding,
            None,
        );
    };

    let scale = monitor.scale_factor();
    let bounds = MonitorBounds {
        x: monitor.position().x as f64 / scale,
        y: monitor.position().y as f64 / scale,
        width: monitor.size().width as f64 / scale,
        height: monitor.size().height as f64 / scale,
    };

    clamp_popover_position_in_bounds_for_size(
        button_x,
        button_y,
        button_width,
        button_height,
        popover_size,
        window_padding,
        Some(bounds),
    )
}

#[cfg(test)]
fn clamp_popover_position_in_bounds(
    button_x: f64,
    button_y: f64,
    button_width: f64,
    button_height: f64,
    bounds: Option<MonitorBounds>,
) -> (f64, f64) {
    clamp_popover_position_in_bounds_for_size(
        button_x,
        button_y,
        button_width,
        button_height,
        popover_size_for_mode("popover"),
        popover_window_padding_for_mode("popover"),
        bounds,
    )
}

fn clamp_popover_position_in_bounds_for_size(
    button_x: f64,
    button_y: f64,
    button_width: f64,
    button_height: f64,
    popover_size: PopoverSize,
    window_padding: f64,
    bounds: Option<MonitorBounds>,
) -> (f64, f64) {
    let centered_x = button_x + (button_width / 2.0) - (popover_size.width / 2.0);
    let above_y = button_y - popover_size.height - POPOVER_GAP;
    let below_y = button_y + button_height + POPOVER_GAP;

    let Some(bounds) = bounds else {
        return (centered_x, above_y);
    };

    let margin = 8.0;
    let monitor_right = bounds.x + bounds.width;
    let monitor_bottom = bounds.y + bounds.height;
    let min_x = bounds.x + margin + window_padding;
    let max_x = monitor_right - popover_size.width - margin - window_padding;
    let min_y = bounds.y + margin + window_padding;
    let max_y = monitor_bottom - popover_size.height - margin - window_padding;

    let x = centered_x.clamp(min_x, max_x);
    let y = if above_y >= min_y {
        above_y
    } else {
        below_y.clamp(min_y, max_y)
    };

    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_popover_above_centered_on_calico_when_there_is_room() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let position = clamp_popover_position_in_bounds(
            500.0,
            600.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            Some(bounds),
        );

        let expected_x = 500.0 + (BUTTON_VISUAL_WIDTH / 2.0) - (POPOVER_WIDTH / 2.0);
        let expected_y = 600.0 - POPOVER_HEIGHT - POPOVER_GAP;
        assert_eq!(position, (expected_x, expected_y));
    }

    #[test]
    fn prompt_popover_height_supports_category_tabs_and_prompt_list() {
        assert_eq!(POPOVER_HEIGHT, 432.0);
    }

    #[test]
    fn prompt_popover_native_window_matches_rounded_panel_size() {
        let visual_size = popover_size_for_mode("popover");
        let window_size = popover_window_size_for_mode("popover");

        assert_eq!(POPOVER_WINDOW_PADDING, 0.0);
        assert_eq!(visual_size.width, POPOVER_WIDTH);
        assert_eq!(visual_size.height, POPOVER_HEIGHT);
        assert_eq!(window_size.width, POPOVER_WINDOW_WIDTH);
        assert_eq!(window_size.height, POPOVER_WINDOW_HEIGHT);
        assert_eq!(window_size.width, POPOVER_WIDTH);
        assert_eq!(window_size.height, POPOVER_HEIGHT);
    }

    #[test]
    fn prompt_popover_visual_rect_matches_native_window_rect() {
        let rect = WindowRect {
            x: 120.0,
            y: 240.0,
            width: POPOVER_WINDOW_WIDTH,
            height: POPOVER_WINDOW_HEIGHT,
        };
        let visual = visual_popover_rect_from_window_rect(rect, "popover");

        assert_eq!(visual.x, rect.x);
        assert_eq!(visual.y, rect.y);
        assert_eq!(visual.width, rect.width);
        assert_eq!(visual.height, rect.height);
    }

    #[test]
    fn button_controls_uses_compact_popover_size() {
        let size = popover_size_for_mode("button-controls");
        let window_size = popover_window_size_for_mode("button-controls");

        assert_eq!(size.width, BUTTON_CONTROLS_WIDTH);
        assert_eq!(size.height, BUTTON_CONTROLS_HEIGHT);
        assert_eq!(window_size.width, BUTTON_CONTROLS_WIDTH);
        assert_eq!(window_size.height, BUTTON_CONTROLS_HEIGHT);
        assert_eq!(BUTTON_CONTROLS_WIDTH, 156.0);
        assert_eq!(BUTTON_CONTROLS_HEIGHT, 72.0);
    }

    #[test]
    fn prompt_popover_gap_keeps_list_close_to_calico() {
        assert_eq!(POPOVER_GAP, 4.0);
    }

    #[test]
    fn places_popover_below_calico_when_top_has_no_room() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let position = clamp_popover_position_in_bounds(
            500.0,
            20.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            Some(bounds),
        );

        assert_eq!(position.1, 20.0 + BUTTON_VISUAL_HEIGHT + POPOVER_GAP);
    }

    #[test]
    fn taller_popover_stays_inside_monitor_near_bottom_edge() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let position = clamp_popover_position_in_bounds(
            500.0,
            760.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            Some(bounds),
        );

        assert!(position.1 >= 8.0);
        assert!(position.1 + POPOVER_HEIGHT <= 892.0);
    }

    #[test]
    fn clamps_popover_horizontally_inside_monitor() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let left = clamp_popover_position_in_bounds(
            4.0,
            400.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            Some(bounds),
        );
        let right = clamp_popover_position_in_bounds(
            1390.0,
            400.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            Some(bounds),
        );

        assert_eq!(left.0, 8.0);
        assert_eq!(right.0, 1440.0 - POPOVER_WIDTH - 8.0);
    }

    #[test]
    fn button_controls_position_uses_compact_size() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };
        let position = clamp_popover_position_in_bounds_for_size(
            500.0,
            400.0,
            BUTTON_VISUAL_WIDTH,
            BUTTON_VISUAL_HEIGHT,
            popover_size_for_mode("button-controls"),
            popover_window_padding_for_mode("button-controls"),
            Some(bounds),
        );

        assert_eq!(
            position,
            (
                500.0 + (BUTTON_VISUAL_WIDTH / 2.0) - (BUTTON_CONTROLS_WIDTH / 2.0),
                400.0 - BUTTON_CONTROLS_HEIGHT - POPOVER_GAP
            )
        );
    }

    #[test]
    fn calico_button_window_uses_square_character_size() {
        assert_eq!(BUTTON_VISUAL_WIDTH, 132.0);
        assert_eq!(BUTTON_VISUAL_HEIGHT, 132.0);
    }

    #[test]
    fn calico_button_window_has_animation_padding() {
        assert_eq!(BUTTON_VISUAL_WIDTH, 132.0);
        assert_eq!(BUTTON_VISUAL_HEIGHT, 132.0);
        assert_eq!(BUTTON_WINDOW_WIDTH, 288.0);
        assert_eq!(BUTTON_WINDOW_HEIGHT, 288.0);
        assert_eq!(BUTTON_WINDOW_PADDING_X, 78.0);
        assert_eq!(BUTTON_WINDOW_PADDING_Y, 78.0);
    }

    #[test]
    fn calico_button_window_uses_larger_native_transparent_size() {
        assert!(BUTTON_WINDOW_WIDTH > BUTTON_VISUAL_WIDTH);
        assert!(BUTTON_WINDOW_HEIGHT > BUTTON_VISUAL_HEIGHT);
    }

    #[test]
    fn calico_button_window_uses_native_transparency() {
        assert!(BUTTON_WINDOW_TRANSPARENT);
    }

    #[test]
    fn calico_visual_window_is_click_through_while_input_window_is_centered_on_the_hit_area() {
        let source = include_str!("windows.rs");

        assert!(
            source.contains("pub const BUTTON_INPUT_WINDOW_LABEL: &str = \"prompt-button-input\";")
        );
        assert!(source.contains("overlay-interaction.html"));
        assert!(source.contains("set_ignore_cursor_events(true)"));
        assert!(source.contains("BUTTON_VISUAL_WIDTH, BUTTON_VISUAL_HEIGHT"));
    }

    #[test]
    fn visible_calico_recovers_a_missing_or_hidden_input_window() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub fn show_prompt_button")
            .expect("show_prompt_button command should exist");
        let end = source[start..]
            .find("#[tauri::command]\npub fn hide_prompt_button")
            .expect("hide_prompt_button should follow show_prompt_button");
        let command = &source[start..start + end];

        assert!(command.contains("if visible && !input.is_visible().unwrap_or(false)"));
        assert!(command.contains("show_ready_prompt_button_window(&app)?"));
    }

    #[test]
    fn clamps_calico_button_inside_monitor() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        let position = clamp_button_position_in_bounds(2000.0, -100.0, Some(bounds));

        assert_eq!(position, (1440.0 - BUTTON_VISUAL_WIDTH - 8.0, 8.0));
    }

    #[test]
    fn calico_visual_and_window_positions_round_trip() {
        let visual = (320.0, 240.0);
        let window = prompt_button_visual_to_window_position(visual.0, visual.1);

        assert_eq!(window, (242.0, 162.0));
        assert_eq!(
            prompt_button_window_to_visual_position(window.0, window.1),
            visual
        );
    }

    #[test]
    fn calico_button_position_from_visual_subtracts_padding_without_affecting_generic_windows() {
        let button_position = prompt_button_position_from_visual(320.0, 240.0);
        let generic_position = logical_position(320.0, 240.0);

        match button_position {
            tauri::Position::Logical(logical) => {
                assert_eq!(logical.x, 242.0);
                assert_eq!(logical.y, 162.0);
            }
            _ => panic!("prompt button position must use logical coordinates"),
        }

        match generic_position {
            tauri::Position::Logical(logical) => {
                assert_eq!(logical.x, 320.0);
                assert_eq!(logical.y, 240.0);
            }
            _ => panic!("generic position must use logical coordinates"),
        }
    }

    #[test]
    fn outside_click_uses_visual_button_rect_not_transparent_window_padding() {
        let native_button = WindowRect {
            x: 242.0,
            y: 162.0,
            width: BUTTON_WINDOW_WIDTH,
            height: BUTTON_WINDOW_HEIGHT,
        };
        let visual_button = visual_button_rect_from_window_rect(native_button);
        let popover = WindowRect {
            x: 800.0,
            y: 20.0,
            width: POPOVER_WIDTH,
            height: POPOVER_HEIGHT,
        };

        assert_eq!(visual_button.x, 320.0);
        assert_eq!(visual_button.y, 240.0);
        assert_eq!(visual_button.width, BUTTON_VISUAL_WIDTH);
        assert_eq!(visual_button.height, BUTTON_VISUAL_HEIGHT);
        assert!(should_dismiss_popover_for_click(
            (300.0, 220.0),
            Some(visual_button),
            Some(popover)
        ));
        assert!(!should_dismiss_popover_for_click(
            (340.0, 260.0),
            Some(visual_button),
            Some(popover)
        ));
    }

    #[test]
    fn outside_click_uses_popover_panel_rect_without_shadow_padding() {
        let native_popover = WindowRect {
            x: 800.0,
            y: 20.0,
            width: POPOVER_WINDOW_WIDTH,
            height: POPOVER_WINDOW_HEIGHT,
        };
        let visual_popover = visual_popover_rect_from_window_rect(native_popover, "popover");

        assert_eq!(visual_popover.x, native_popover.x);
        assert_eq!(visual_popover.y, native_popover.y);
        assert_eq!(visual_popover.width, native_popover.width);
        assert_eq!(visual_popover.height, native_popover.height);
        assert!(should_dismiss_popover_for_click(
            (790.0, 12.0),
            None,
            Some(visual_popover)
        ));
        assert!(!should_dismiss_popover_for_click(
            (810.0, 30.0),
            None,
            Some(visual_popover)
        ));
    }

    #[test]
    fn outside_click_dismisses_when_point_is_outside_button_and_popover() {
        let button = WindowRect {
            x: 100.0,
            y: 500.0,
            width: BUTTON_VISUAL_WIDTH,
            height: BUTTON_VISUAL_HEIGHT,
        };
        let popover = WindowRect {
            x: 20.0,
            y: 108.0,
            width: POPOVER_WIDTH,
            height: POPOVER_HEIGHT,
        };

        assert!(should_dismiss_popover_for_click(
            (900.0, 900.0),
            Some(button),
            Some(popover)
        ));
    }

    #[test]
    fn outside_click_keeps_popover_when_point_is_inside_popover_or_button() {
        let button = WindowRect {
            x: 100.0,
            y: 500.0,
            width: BUTTON_VISUAL_WIDTH,
            height: BUTTON_VISUAL_HEIGHT,
        };
        let popover = WindowRect {
            x: 20.0,
            y: 108.0,
            width: POPOVER_WIDTH,
            height: POPOVER_HEIGHT,
        };

        assert!(!should_dismiss_popover_for_click(
            (40.0, 120.0),
            Some(button),
            Some(popover)
        ));
        assert!(!should_dismiss_popover_for_click(
            (120.0, 520.0),
            Some(button),
            Some(popover)
        ));
    }

    #[test]
    fn outside_click_does_nothing_without_visible_popover_rect() {
        let button = WindowRect {
            x: 100.0,
            y: 500.0,
            width: BUTTON_VISUAL_WIDTH,
            height: BUTTON_VISUAL_HEIGHT,
        };

        assert!(!should_dismiss_popover_for_click(
            (900.0, 900.0),
            Some(button),
            None
        ));
    }

    #[test]
    fn converts_bottom_left_screen_point_to_top_left_logical_point() {
        let bounds = MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        assert_eq!(
            bottom_left_to_top_left_point((100.0, 200.0), bounds),
            (100.0, 700.0)
        );
    }

    #[test]
    fn transparent_popover_modes_enable_outside_click_monitor() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn show_popover_mode")
            .expect("popover mode function should exist");
        let end = source[start..]
            .find("#[tauri::command]")
            .expect("first popover command should follow show_popover_mode");
        let show_source = &source[start..start + end];

        assert!(should_use_transparent_popover_window(Some("popover")));
        assert!(should_use_transparent_popover_window(Some(
            "button-controls"
        )));
        assert!(show_source.contains("should_use_transparent_popover_window(Some(mode))"));
        assert!(show_source.contains("enable_prompt_popover_outside_click_monitor(app)?;"));
        assert!(show_source.contains("set_outside_click_monitor_active(false);"));
    }

    #[test]
    fn outside_click_monitor_uses_global_and_local_mouse_down_events() {
        let source = include_str!("windows.rs");
        assert!(source.contains("addGlobalMonitorForEventsMatchingMask_handler"));
        assert!(source.contains("addLocalMonitorForEventsMatchingMask_handler"));
        assert!(source.contains("NSEventMask::LeftMouseDown"));
        assert!(source.contains("NSEventMask::RightMouseDown"));
        assert!(source.contains("NSEventMask::OtherMouseDown"));
    }

    #[test]
    fn outside_click_monitor_installation_is_dispatched_to_the_main_thread() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn install_prompt_popover_outside_click_monitor")
            .expect("outside-click installer should exist");
        let end = source[start..]
            .find("fn enable_prompt_popover_outside_click_monitor")
            .expect("outside-click enabler should follow installer");
        let installer = &source[start..start + end];

        assert!(installer.contains("run_on_main_thread_sync"));
        assert!(installer.contains("install_prompt_popover_outside_click_monitor_on_main_thread"));
    }

    #[test]
    fn reuses_popover_only_for_the_same_mode() {
        assert!(should_reuse_popover(Some("popover"), "popover"));
        assert!(should_reuse_popover(
            Some("button-controls"),
            "button-controls"
        ));
        assert!(!should_reuse_popover(Some("button-controls"), "popover"));
        assert!(!should_reuse_popover(None, "popover"));
    }

    #[test]
    fn prompt_popover_toggle_closes_only_visible_prompt_lists() {
        assert!(should_close_prompt_popover_on_toggle(Some("popover"), true));
        assert!(!should_close_prompt_popover_on_toggle(
            Some("popover"),
            false
        ));
        assert!(!should_close_prompt_popover_on_toggle(
            Some("button-controls"),
            true
        ));
        assert!(!should_close_prompt_popover_on_toggle(None, true));
    }

    #[test]
    fn prompt_popover_toggle_invalidates_session_and_emits_dismissal_when_closing() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub async fn toggle_prompt_popover_from_button")
            .expect("toggle command should exist");
        let end = source[start..]
            .find("pub async fn show_prompt_button_controls_from_button")
            .expect("button controls command should follow toggle command");
        let command_source = &source[start..start + end];

        assert!(command_source.contains("should_close_prompt_popover_on_toggle"));
        assert!(command_source.contains("session_state.begin(session_id);"));
        assert!(command_source.contains("window.hide().map_err"));
        assert!(command_source.contains("emit_popover_dismissed(&app)"));
        assert!(command_source.contains("PromptPopoverToggleOutcome { opened: false }"));
        assert!(command_source.contains("PromptPopoverToggleOutcome { opened: true }"));
    }

    #[test]
    fn prompt_popover_open_freezes_immediately_and_enriches_in_background() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub async fn toggle_prompt_popover_from_button")
            .expect("toggle command should exist");
        let end = source[start..]
            .find("pub async fn show_prompt_button_controls_from_button")
            .expect("next command should follow toggle");
        let command_source = &source[start..start + end];
        let capture_start = source
            .find("fn start_prompt_pick_session_capture")
            .expect("capture helper should exist");
        let capture_end = source[capture_start..]
            .find("fn build_prompt_button_window")
            .expect("button builder should follow capture helper");
        let capture_source = &source[capture_start..capture_start + capture_end];

        assert!(command_source.contains("prepare_prompt_pick_session_target"));
        assert!(command_source.contains("start_prompt_pick_session_capture"));
        assert!(command_source.contains("show_popover_mode"));
        assert!(
            command_source
                .find("prepare_prompt_pick_session_target")
                .expect("target should be frozen")
                < command_source
                    .find("show_popover_mode")
                    .expect("popover should be shown")
        );
        assert!(!command_source.contains("capture_prompt_pick_session_before_show"));
        assert!(capture_source.contains("capture_prompt_pick_session_target"));
        assert!(capture_source.contains("spawn_blocking"));
        assert!(capture_source.contains("finish_capture"));
        assert!(capture_source.contains("run_prompt_pick_session_capture_loop"));
        assert!(capture_source.contains("refresh_prompt_pick_session_capture"));
        assert!(capture_source.contains("current_target_window_identity"));
        assert!(capture_source.contains("active_browser_page_url"));
        assert!(!capture_source.contains("begin_if_new"));
        // Full input traversal only once (the initial capture), not inside the refresh loop body.
        let refresh_start = capture_source
            .find("fn refresh_prompt_pick_session_capture")
            .expect("refresh helper should exist");
        let refresh_source = &capture_source[refresh_start..];
        assert!(!refresh_source.contains("current_input_target"));
        assert!(!refresh_source.contains("capture_prompt_pick_session_target"));
    }

    #[test]
    fn capture_refresh_loop_exits_when_session_is_consumed() {
        // Contract lock for the lightweight retry loop: no real AX needed; a consumed
        // session must stop the loop early instead of burning MAX_ATTEMPTS.
        let session_state = crate::PromptPickSessionState::default();
        let session_id = 42u64;
        session_state.begin(session_id);

        let mut sleep_count = 0u32;
        run_prompt_pick_session_capture_loop(session_id, &session_state, |_| {
            sleep_count += 1;
            if sleep_count == 1 {
                let _ = session_state.take();
            }
        });

        assert!(session_state.is_consumed(session_id));
        assert!(
            sleep_count >= 1 && sleep_count < 10,
            "loop should exit soon after consume, got {sleep_count} sleeps"
        );
    }

    #[test]
    fn start_prompt_pick_session_capture_does_not_block_and_signals_finish() {
        // The loop spawns and must not run heavy work inline. Real AX behavior is
        // covered by the manual QA matrix; here we lock: spawn is near-instant and
        // a consumed session resolves wait_for_capture quickly.
        // Uses a regular #[test] (not #[tokio::test]) — tokio macros/rt aren't
        // declared in this crate; tauri::async_runtime provides the runtime.
        let session_state = crate::PromptPickSessionState::default();
        let recent_state = crate::LastInputTargetState::default();
        let session_id = 42;
        session_state.begin(session_id);
        session_state.mark_capture_pending(session_id);

        let started = std::time::Instant::now();
        start_prompt_pick_session_capture(session_id, session_state.clone(), recent_state);
        assert!(started.elapsed() < std::time::Duration::from_millis(50));

        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = session_state.take(); // consumed → loop exits → finish_capture fires
        let ready = session_state.wait_for_capture(std::time::Duration::from_millis(500));
        assert!(ready);
    }

    #[test]
    fn prompt_popover_is_prewarmed_hidden_for_immediate_first_open() {
        let windows_source = include_str!("windows.rs");
        let lib_source = include_str!("lib.rs");
        let start = windows_source
            .find("pub(crate) fn prewarm_prompt_popover")
            .expect("popover prewarm helper should exist");
        let end = windows_source[start..]
            .find("fn show_non_activating_overlay_window")
            .expect("overlay show helper should follow prewarm");
        let prewarm = &windows_source[start..start + end];

        assert!(windows_source.contains("pub(crate) fn prewarm_prompt_popover"));
        assert!(windows_source.contains("build_prompt_popover_window(app, 0.0, 0.0, \"popover\")"));
        assert!(windows_source.contains(".visible(false)"));
        assert!(prewarm.contains("configure_non_activating_panel(&window)?"));
        assert!(!prewarm.contains("show_non_activating_panel"));
        assert!(lib_source.contains("crate::windows::prewarm_prompt_popover(app.handle())"));
    }

    #[test]
    fn hide_prompt_popover_emits_dismissal_after_hiding_visible_window() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub fn hide_prompt_popover")
            .expect("hide_prompt_popover command should exist");
        let end = source[start..]
            .find("#[tauri::command]\npub async fn show_prompt_popover_from_button")
            .expect("show_prompt_popover_from_button should follow hide_prompt_popover");
        let command_source = &source[start..start + end];

        assert!(command_source.contains("let was_visible = window.is_visible().unwrap_or(false);"));
        assert!(command_source.contains("if was_visible {"));
        assert!(command_source.contains("emit_popover_dismissed(&app);"));
        assert!(
            command_source
                .find("window.hide().map_err")
                .expect("hide should happen")
                < command_source
                    .find("emit_popover_dismissed(&app);")
                    .expect("dismissal should be emitted")
        );
    }

    #[test]
    fn reused_popover_is_repositioned_shown_and_announced() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn show_popover_mode")
            .expect("show_popover_mode should exist");
        let end = source[start..]
            .find("let url = format!")
            .expect("show_popover_mode should build a fresh popover after reuse branch");
        let reuse_source = &source[start..start + end];

        assert!(reuse_source.contains("should_reuse_popover("));
        assert!(reuse_source.contains("set_position(logical_position(window_x, window_y))"));
        assert!(reuse_source.contains("show_non_activating_overlay_window(&window)?"));
        assert!(reuse_source.contains("emit_popover_opened(app, mode)"));
    }

    #[test]
    fn popover_builder_creates_hidden_non_focusable_window_before_panel_configuration() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn build_prompt_popover_window")
            .expect("popover builder helper should exist");
        let end = source[start..]
            .find("pub(crate) fn prewarm_prompt_popover")
            .expect("popover prewarm helper should follow the builder");
        let block = &source[start..start + end];

        assert!(block.contains(".visible(false)"));
        assert!(block.contains(".focusable(false)"));
    }

    #[test]
    fn prompt_button_builder_creates_hidden_non_focusable_window() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn build_prompt_button_window")
            .expect("prompt button builder should exist");
        let end = source[start..]
            .find("fn build_prompt_button_input_window")
            .expect("input builder should follow the visual builder");
        let block = &source[start..start + end];

        assert!(block.contains(".visible(false)"));
        assert!(block.contains(".focusable(false)"));
        assert!(block.contains("renderer_state.allocate_instance()"));
        assert!(block.contains("overlay.html?rendererInstanceId="));
        assert!(!block.contains("show_non_activating_overlay_window(&window)?"));
    }

    #[test]
    fn reused_popover_configures_panel_before_any_show_call() {
        let source = include_str!("windows.rs");
        let start = source
            .find("if should_reuse_popover")
            .expect("reused popover branch should exist");
        let end = source[start..]
            .find("return Ok(());")
            .expect("reused popover branch should return");
        let block = &source[start..start + end];

        assert!(block.contains("show_non_activating_overlay_window(&window)?"));
        assert!(!block.contains("window.show().map_err"));
    }

    #[test]
    fn overlay_visibility_uses_single_non_activating_show_helper() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn show_non_activating_overlay_window")
            .expect("overlay show helper should exist");
        let end = source[start..]
            .find("fn build_prompt_button_window")
            .expect("button builder should follow overlay show helper");
        let helper = &source[start..start + end];

        assert!(!helper.contains("window.set_focusable(false)"));
        assert!(helper.contains("show_non_activating_panel"));
        assert!(helper.contains("#[cfg(target_os = \"macos\")]"));
        assert!(helper.contains("#[cfg(not(target_os = \"macos\"))]"));
        assert!(helper.contains("window.show()"));
    }

    #[test]
    fn popover_toggle_still_uses_visible_state_to_close_open_prompt_list() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub async fn toggle_prompt_popover_from_button")
            .expect("toggle command should exist");
        let end = source[start..]
            .find("pub async fn show_prompt_button_controls_from_button")
            .expect("button controls command should follow toggle command");
        let block = &source[start..start + end];

        assert!(block.contains("window.is_visible().unwrap_or(false)"));
        assert!(block.contains("should_close_prompt_popover_on_toggle"));
        assert!(block.contains("window.hide().map_err"));
    }

    #[test]
    fn outside_click_paths_keep_using_visible_state() {
        let source = include_str!("windows.rs");

        assert!(source.contains("fn handle_prompt_popover_outside_click"));
        assert!(source.contains("popover_window.is_visible().unwrap_or(false)"));
        assert!(source.contains("window_rect(&app, POPOVER_WINDOW_LABEL)"));
    }

    #[test]
    fn popover_modes_use_transparent_native_window() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn show_popover_mode")
            .expect("show_popover_mode should exist");
        let end = source[start..]
            .find("#[tauri::command]\npub fn show_prompt_button")
            .expect("show_prompt_button should follow show_popover_mode");
        let command_source = &source[start..start + end];

        assert!(command_source.contains("should_use_transparent_popover_window(Some(mode))"));
        assert!(command_source.contains("configure_transparent_webview_window(&window)?"));
        assert!(command_source.contains("show_non_activating_overlay_window(&window)?"));
    }

    #[test]
    fn popover_mode_switch_reuses_the_existing_window() {
        assert_eq!(
            popover_window_action(Some("popover"), "button-controls"),
            PopoverWindowAction::ReuseExisting
        );
    }

    #[test]
    fn overlay_runtime_has_no_prompt_button_rebuild_helper() {
        let source = include_str!("windows.rs");
        let rebuild_helper = ["rebuild_prompt", "_button_window"].concat();
        let close_call = ["window", ".close()"].concat();

        assert!(!source.contains(&rebuild_helper));
        assert!(!source.contains(&close_call));
    }

    #[test]
    fn newer_popover_request_supersedes_the_previous_waiter() {
        let state = PopoverModeRequestState::default();
        let (_, mut first_receiver) = state.begin("popover");
        let (second_id, _second_receiver) = state.begin("button-controls");

        assert_eq!(
            first_receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(!state.acknowledge(second_id - 1, "popover"));
        assert!(state.acknowledge(second_id, "button-controls"));
    }

    #[test]
    fn hiding_popover_cancels_pending_mode_acknowledgement() {
        let state = PopoverModeRequestState::default();
        let (request_id, mut receiver) = state.begin("popover");

        assert!(state.cancel());
        assert_eq!(
            receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        );
        assert!(!state.acknowledge(request_id, "popover"));
    }

    #[test]
    fn ready_button_show_rechecks_gates_and_presents_the_pair_atomically() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub(crate) fn show_ready_prompt_button_window")
            .expect("show_ready_prompt_button_window should exist");
        let end = source[start..]
            .find("#[tauri::command]\npub fn show_prompt_button")
            .expect("show_prompt_button should follow show_ready_prompt_button_window");
        let body = &source[start..start + end];

        // Uses the single main-thread pair transaction instead of separate shows.
        assert!(body.contains("present_prompt_button_pair(app, &window, &input)?"));
        assert!(!body.contains("show_non_activating_overlay_window(&window)?"));
        // Re-checks user visibility and health immediately before presenting.
        assert!(body.contains("desired_visible()"));
        assert!(body.contains("may_present_current()"));
        // Visual stays click-through while input remains interactive.
        let visual_ignore = body
            .find("set_ignore_cursor_events(true)")
            .expect("visual ignores cursor");
        let input_ignore = body
            .find("set_ignore_cursor_events(false)")
            .expect("input accepts cursor");
        assert!(visual_ignore < input_ignore);
        // Never activates the process or makes the overlay key.
        assert!(!body.contains("makeKeyAndOrderFront"));
        assert!(!body.contains("activateIgnoringOtherApps"));
    }

    #[test]
    fn hide_prompt_button_pair_hides_input_before_visual() {
        let source = include_str!("windows.rs");
        let start = source
            .find("pub(crate) fn hide_prompt_button_pair")
            .expect("hide_prompt_button_pair should exist");
        let end = source[start..]
            .find("#[tauri::command]")
            .expect("a command should follow hide_prompt_button_pair");
        let body = &source[start..start + end];

        let input_hide = body
            .find("BUTTON_INPUT_WINDOW_LABEL")
            .expect("input hidden");
        let visual_hide = body.find("BUTTON_WINDOW_LABEL").expect("visual hidden");
        assert!(input_hide < visual_hide);
    }

    #[test]
    fn prompt_button_pair_build_rolls_back_every_created_label() {
        let source = include_str!("windows.rs");
        let start = source
            .find("fn build_prompt_button_window")
            .expect("prompt button builder should exist");
        let end = source[start..]
            .find("fn build_prompt_button_input_window")
            .expect("input builder should follow the visual builder");
        let body = &source[start..start + end];

        // Candidate construction commits only after both halves succeed.
        assert!(body.contains("visual_health.commit_pair(renderer_instance_id);"));
        // Failure tears down every created label and records the failure.
        assert!(body.contains("window.destroy()"));
        assert!(body.contains("input.destroy()"));
        assert!(body.contains("visual_health.record_recovery_failure();"));
        // Rollback precedes the commit.
        let rollback = body
            .find("if let Err(error) = pair_result")
            .expect("rollback branch");
        let commit = body.find("visual_health.commit_pair").expect("commit");
        assert!(rollback < commit);
    }
}
