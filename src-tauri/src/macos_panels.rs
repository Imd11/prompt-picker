#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::{
    ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
    runtime::{AnyClass, AnyObject, Bool, Imp, NSObject, NSObjectProtocol, Sel},
    sel, AllocAnyThread, ClassType, DefinedClass,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSBitmapFormat, NSBitmapImageRep, NSColor,
    NSEvent, NSImage, NSPanel, NSRunningApplication, NSScreenSaverWindowLevel, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSError, NSNumber, NSRect, NSString};
#[cfg(target_os = "macos")]
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};

#[cfg(target_os = "macos")]
const PROMPT_POPOVER_LABEL: &str = "prompt-popover";
#[cfg(target_os = "macos")]
const PROMPT_POPOVER_POINTER_EVENT: &str = "prompt-popover-pointer-position";
#[cfg(target_os = "macos")]
static PROMPT_POPOVER_POINTER_TRACKER_KEY: u8 = 0;

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, serde::Serialize)]
struct PromptPopoverPointerPosition {
    x: f64,
    y: f64,
    inside: bool,
}

#[cfg(target_os = "macos")]
struct PromptPopoverPointerTrackerIvars {
    app: tauri::AppHandle,
}

#[cfg(target_os = "macos")]
objc2::define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PromptDrawerPopoverPointerTracker"]
    #[ivars = PromptPopoverPointerTrackerIvars]
    struct PromptPopoverPointerTracker;

    unsafe impl NSObjectProtocol for PromptPopoverPointerTracker {}

    impl PromptPopoverPointerTracker {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            self.emit_pointer_position(event, true);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.emit_pointer_position(event, true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: &NSEvent) {
            self.emit_pointer_position(event, false);
        }
    }
);

#[cfg(target_os = "macos")]
impl PromptPopoverPointerTracker {
    fn new(app: tauri::AppHandle) -> objc2::rc::Retained<Self> {
        let tracker = Self::alloc().set_ivars(PromptPopoverPointerTrackerIvars { app });
        unsafe { objc2::msg_send![super(tracker), init] }
    }

    fn emit_pointer_position(&self, event: &NSEvent, inside: bool) {
        let position = if inside {
            pointer_position_from_event(event).unwrap_or(PromptPopoverPointerPosition {
                x: 0.0,
                y: 0.0,
                inside: false,
            })
        } else {
            PromptPopoverPointerPosition {
                x: 0.0,
                y: 0.0,
                inside: false,
            }
        };
        let _ =
            self.ivars()
                .app
                .emit_to(PROMPT_POPOVER_LABEL, PROMPT_POPOVER_POINTER_EVENT, position);
    }
}

#[cfg(target_os = "macos")]
fn pointer_position_from_event(event: &NSEvent) -> Option<PromptPopoverPointerPosition> {
    let mtm = objc2::MainThreadMarker::new()?;
    let window = event.window(mtm)?;
    let content_view = window.contentView()?;
    let bounds = content_view.bounds();
    let point = content_view.convertPoint_fromView(event.locationInWindow(), None);
    let (x, y) = top_left_pointer_position(
        point.x - bounds.origin.x,
        point.y - bounds.origin.y,
        bounds.size.height,
        content_view.isFlipped(),
    );
    Some(PromptPopoverPointerPosition { x, y, inside: true })
}

#[cfg(target_os = "macos")]
fn current_prompt_popover_pointer_position(
    window: &tauri::WebviewWindow,
) -> Result<PromptPopoverPointerPosition, String> {
    let _mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| "Prompt popover pointer snapshot must run on the main thread".to_string())?;
    let ns_window_ptr = window.ns_window().map_err(|error| error.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("ns_window returned null".to_string());
    }

    let ns_window = unsafe { &*(ns_window_ptr.cast::<NSWindow>()) };
    let content_view = ns_window
        .contentView()
        .ok_or_else(|| "Prompt popover has no content view".to_string())?;
    let bounds = content_view.bounds();
    let point = content_view.convertPoint_fromView(
        ns_window.mouseLocationOutsideOfEventStream(),
        None,
    );
    let local_x = point.x - bounds.origin.x;
    let local_y = point.y - bounds.origin.y;
    let inside = local_x >= 0.0
        && local_y >= 0.0
        && local_x < bounds.size.width
        && local_y < bounds.size.height;
    let (x, y) = top_left_pointer_position(
        local_x,
        local_y,
        bounds.size.height,
        content_view.isFlipped(),
    );

    Ok(PromptPopoverPointerPosition { x, y, inside })
}

#[cfg(target_os = "macos")]
pub(crate) fn emit_current_prompt_popover_pointer_position(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let app = window.app_handle().clone();
    let task_app = app.clone();
    let task_window = window.clone();
    let position = run_on_main_thread_sync(&task_app, move || {
        current_prompt_popover_pointer_position(&task_window)
    })?;
    app.emit_to(PROMPT_POPOVER_LABEL, PROMPT_POPOVER_POINTER_EVENT, position)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn top_left_pointer_position(x: f64, y: f64, height: f64, is_flipped: bool) -> (f64, f64) {
    let top = if is_flipped { y } else { height - y };
    (x.max(0.0), top.max(0.0))
}

#[cfg(target_os = "macos")]
fn prompt_popover_pointer_tracking_options() -> NSTrackingAreaOptions {
    NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::MouseMoved
        | NSTrackingAreaOptions::ActiveAlways
        | NSTrackingAreaOptions::InVisibleRect
}

#[cfg(target_os = "macos")]
objc2::define_class!(
    #[unsafe(super(NSPanel))]
    #[name = "PromptDrawerOverlayPanel"]
    struct PromptDrawerOverlayPanel;

    unsafe impl NSObjectProtocol for PromptDrawerOverlayPanel {}

    impl PromptDrawerOverlayPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            false
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

#[cfg(target_os = "macos")]
pub(crate) fn run_on_main_thread_sync<T, F>(app: &tauri::AppHandle, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    if objc2::MainThreadMarker::new().is_some() {
        return task();
    }

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = sender.send(task());
    })
    .map_err(|error| format!("Failed to schedule macOS window task: {error}"))?;

    receiver
        .recv()
        .map_err(|_| "macOS window task ended without a result".to_string())?
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_running_application(app: &tauri::AppHandle, pid: u32) -> Result<(), String> {
    run_on_main_thread_sync(app, move || {
        let running = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
            .ok_or_else(|| format!("Target process {pid} is no longer running."))?;
        running
            .activateWithOptions(NSApplicationActivationOptions::empty())
            .then_some(())
            .ok_or_else(|| format!("Target process {pid} could not be activated."))
    })
}

#[cfg(target_os = "macos")]
pub fn activate_main_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| "activate_main_window must run on the main thread".to_string())?;
    let ns_window_ptr = window.ns_window().map_err(|e| e.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("ns_window returned null".to_string());
    }

    unsafe {
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        ns_window.makeKeyAndOrderFront(None);
        ns_window.makeMainWindow();
        ns_window.makeKeyWindow();
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn configure_non_activating_panel(window: &tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle().clone();
    let window = window.clone();
    run_on_main_thread_sync(&app, move || {
        configure_non_activating_panel_on_main_thread(&window)
    })
}

#[cfg(target_os = "macos")]
fn configure_non_activating_panel_on_main_thread(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let _mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| "configure_non_activating_panel must run on the main thread".to_string())?;
    let ns_window_ptr = window.ns_window().map_err(|e| e.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("ns_window returned null".to_string());
    }

    unsafe {
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        let object: &AnyObject = ns_window.as_ref();
        let original_class_name = object.class().name().to_string_lossy().to_string();
        let action = ensure_native_overlay_panel(window, &original_class_name)?;
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        let mask = ns_window.styleMask()
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::UtilityWindow;
        ns_window.setStyleMask(mask);
        ns_window.setLevel(NSScreenSaverWindowLevel);
        ns_window.setCanHide(false);
        ns_window.setHidesOnDeactivate(false);
        ns_window.setIgnoresMouseEvents(false);
        ns_window.setAcceptsMouseMovedEvents(true);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::CanJoinAllApplications
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        let panel: &NSPanel = &*(ns_window_ptr.cast::<NSPanel>());
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        configure_non_activating_webview_on_main_thread(window)?;

        let is_native_panel: bool = objc2::msg_send![ns_window, isKindOfClass: NSPanel::class()];
        let can_become_key: Bool = objc2::msg_send![ns_window, canBecomeKeyWindow];
        let can_become_main: Bool = objc2::msg_send![ns_window, canBecomeMainWindow];
        if !is_native_panel || can_become_key.as_bool() || can_become_main.as_bool() {
            return Err(format!(
                "Overlay {} is not a non-activating native NSPanel.",
                window.label()
            ));
        }

        if focus_diagnostics_enabled() {
            let report = PanelKeyBehaviorReport {
                label: window.label().to_string(),
                class_name: object.class().name().to_string_lossy().to_string(),
                action,
                can_become_key: Some(can_become_key.as_bool()),
                can_become_main: Some(can_become_main.as_bool()),
            };
            eprintln!("{}", format_panel_key_behavior_report(&report));
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn show_non_activating_panel(window: &tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle().clone();
    let window = window.clone();
    run_on_main_thread_sync(&app, move || {
        configure_non_activating_panel_on_main_thread(&window)?;
        let ns_window_ptr = window.ns_window().map_err(|error| error.to_string())?;
        if ns_window_ptr.is_null() {
            return Err("ns_window returned null".to_string());
        }

        unsafe {
            let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
            ns_window.orderFrontRegardless();
            let visible: Bool = objc2::msg_send![ns_window, isVisible];
            visible
                .as_bool()
                .then_some(())
                .ok_or_else(|| "Overlay window did not become visible.".to_string())
        }
    })
}

#[cfg(target_os = "macos")]
extern "C-unwind" fn reject_webview_key_focus(_: &AnyObject, _: Sel) -> Bool {
    Bool::NO
}

#[cfg(target_os = "macos")]
fn configure_non_activating_webview_on_main_thread(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let label = window.label().to_string();
    let app = window.app_handle().clone();
    window
        .with_webview(move |webview| {
            let object = unsafe { &*(webview.inner().cast::<AnyObject>()) };
            let result = configure_non_key_webview_object(object, &label)
                .and_then(|_| configure_prompt_popover_pointer_tracking(&app, object, &label));
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;

    receiver
        .recv()
        .map_err(|_| "Overlay webview focus configuration produced no result.".to_string())?
}

#[cfg(target_os = "macos")]
fn configure_prompt_popover_pointer_tracking(
    app: &tauri::AppHandle,
    object: &AnyObject,
    label: &str,
) -> Result<(), String> {
    if label != PROMPT_POPOVER_LABEL {
        return Ok(());
    }

    let key = std::ptr::addr_of!(PROMPT_POPOVER_POINTER_TRACKER_KEY).cast();
    let existing_tracker = unsafe { objc2::ffi::objc_getAssociatedObject(object, key) };
    if !existing_tracker.is_null() {
        return Ok(());
    }

    let tracker = PromptPopoverPointerTracker::new(app.clone());
    let tracking_area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            prompt_popover_pointer_tracking_options(),
            Some(&tracker),
            None,
        )
    };
    let view = unsafe { &*(object as *const AnyObject).cast::<NSView>() };

    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            object as *const AnyObject as *mut AnyObject,
            key,
            objc2::rc::Retained::as_ptr(&tracker) as *mut AnyObject,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
        view.addTrackingArea(&tracking_area);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_non_key_webview_object(object: &AnyObject, label: &str) -> Result<(), String> {
    let class = object.class();
    let selector = sel!(needsPanelToBecomeKey);
    let inherited_method = class.instance_method(selector).ok_or_else(|| {
        format!("Overlay {label} webview does not implement needsPanelToBecomeKey.")
    })?;

    // Adding an override to WryWebView avoids changing the class of a live
    // WKWebView, which invalidates AppKit's cached view properties. This
    // selector is only consulted by NSPanel when becomesKeyOnlyIfNeeded is set.
    unsafe {
        let implementation: Imp = std::mem::transmute(
            reject_webview_key_focus as extern "C-unwind" fn(&AnyObject, Sel) -> Bool,
        );
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            implementation,
            objc2::ffi::method_getTypeEncoding(inherited_method),
        );
    }

    let needs_panel_key: Bool = unsafe { objc2::msg_send![object, needsPanelToBecomeKey] };
    if needs_panel_key.as_bool() {
        Err(format!("Overlay {label} webview still requests key focus."))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelClassAction {
    AlreadyNativePanel,
    ConvertToNativePanel,
}

fn panel_class_action_for_name(class_name: &str) -> PanelClassAction {
    if class_name.contains("PromptDrawerOverlayPanel") {
        PanelClassAction::AlreadyNativePanel
    } else {
        PanelClassAction::ConvertToNativePanel
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PanelKeyBehaviorReport {
    label: String,
    class_name: String,
    action: PanelClassAction,
    can_become_key: Option<bool>,
    can_become_main: Option<bool>,
}

fn format_panel_key_behavior_report(report: &PanelKeyBehaviorReport) -> String {
    format!(
        "prompt-picker-panel label={} class={} action={:?} can_become_key={} can_become_main={}",
        report.label,
        report.class_name,
        report.action,
        option_bool_label(report.can_become_key),
        option_bool_label(report.can_become_main)
    )
}

fn option_bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn focus_diagnostics_enabled() -> bool {
    std::env::var("PROMPT_PICKER_FOCUS_DIAGNOSTICS").is_ok()
}

#[cfg(target_os = "macos")]
fn ensure_native_overlay_panel(
    window: &tauri::WebviewWindow,
    class_name: &str,
) -> Result<PanelClassAction, String> {
    let action = panel_class_action_for_name(class_name);
    if action == PanelClassAction::ConvertToNativePanel {
        let ns_window_ptr = window.ns_window().map_err(|error| error.to_string())?;
        if ns_window_ptr.is_null() {
            return Err("ns_window returned null".to_string());
        }
        unsafe {
            let object = &*(ns_window_ptr.cast::<AnyObject>());
            let current_class = object.class();
            let panel_class = PromptDrawerOverlayPanel::class();
            if panel_class.instance_size() > current_class.instance_size() {
                return Err(
                    "Native NSPanel class does not fit the Tauri window allocation.".to_string(),
                );
            }

            // The window is still hidden here. Replace only its Objective-C class;
            // the NSWindow object, WKWebView, delegate, and ownership stay unchanged.
            let previous_class = AnyObject::set_class(object, panel_class);
            if previous_class.name().to_string_lossy() != class_name {
                return Err("Overlay window class changed during NSPanel conversion.".to_string());
            }
        }
    }
    Ok(action)
}

#[cfg(target_os = "macos")]
pub fn configure_transparent_webview_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle().clone();
    let window = window.clone();
    run_on_main_thread_sync(&app, move || {
        configure_transparent_webview_window_on_main_thread(&window)
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn request_calico_webview_snapshot<F>(
    window: &tauri::WebviewWindow,
    completion: F,
) -> Result<(), String>
where
    F: FnOnce(Result<u64, String>) + Send + 'static,
{
    let completion = std::sync::Arc::new(std::sync::Mutex::new(Some(completion)));
    window
        .with_webview(move |webview| {
            let view = unsafe { &*webview.inner().cast::<WKWebView>() };
            let completion_for_block = completion.clone();
            let block = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
                let result = if image.is_null() {
                    Err("WKWebView snapshot returned no image.".to_string())
                } else {
                    count_visible_snapshot_pixels(unsafe { &*image })
                };
                if let Some(completion) = completion_for_block
                    .lock()
                    .expect("snapshot completion lock poisoned")
                    .take()
                {
                    completion(result);
                }
            });
            let mtm = objc2::MainThreadMarker::new().expect("WKWebView callback is on main thread");
            let configuration = unsafe { WKSnapshotConfiguration::new(mtm) };
            unsafe {
                configuration.setAfterScreenUpdates(true);
                view.takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &block);
            }
        })
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn count_visible_snapshot_pixels(image: &NSImage) -> Result<u64, String> {
    let data = image
        .TIFFRepresentation()
        .ok_or_else(|| "WKWebView snapshot has no TIFF representation.".to_string())?;
    let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &data)
        .ok_or_else(|| "WKWebView snapshot could not be decoded.".to_string())?;
    if bitmap.isPlanar() || bitmap.bitsPerSample() != 8 || !bitmap.hasAlpha() {
        return Err("WKWebView snapshot has an unsupported pixel layout.".to_string());
    }
    let width = bitmap.pixelsWide().max(0) as usize;
    let height = bitmap.pixelsHigh().max(0) as usize;
    let samples = bitmap.samplesPerPixel().max(0) as usize;
    let bytes_per_row = bitmap.bytesPerRow().max(0) as usize;
    let data = bitmap.bitmapData();
    if width == 0 || height == 0 || samples < 2 || bytes_per_row == 0 || data.is_null() {
        return Err("WKWebView snapshot has no pixel data.".to_string());
    }
    let alpha_index = if bitmap.bitmapFormat().contains(NSBitmapFormat::AlphaFirst) {
        0
    } else {
        samples - 1
    };
    let bytes = unsafe { std::slice::from_raw_parts(data, bytes_per_row * height) };
    Ok(count_visible_alpha_pixels_in_central_roi(
        bytes,
        width,
        height,
        samples,
        bytes_per_row,
        alpha_index,
    ))
}

/// Counts pixels with alpha > 8 inside the central 27%–73% region (both axes).
///
/// The ROI deliberately excludes the outer ~27% margin on every side so that a
/// status bubble rendered near an edge/corner cannot contribute to the "cat is
/// alive" signal; only the cat's body occupies the centre. Alpha > 8 ignores
/// anti-aliasing fringe on an otherwise transparent overlay.
///
/// This is pure byte math (no AppKit) so the ROI/threshold logic is unit testable
/// on every platform; the native snapshot path decodes the image and feeds the
/// bytes in.
#[cfg(target_os = "macos")]
fn count_visible_alpha_pixels_in_central_roi(
    bytes: &[u8],
    width: usize,
    height: usize,
    samples: usize,
    bytes_per_row: usize,
    alpha_index: usize,
) -> u64 {
    if width == 0 || height == 0 || samples < 2 || bytes_per_row == 0 {
        return 0;
    }
    let min_x = width * 27 / 100;
    let max_x = width * 73 / 100;
    let min_y = height * 27 / 100;
    let max_y = height * 73 / 100;
    let mut visible = 0u64;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let offset = y * bytes_per_row + x * samples + alpha_index;
            if offset < bytes.len() && bytes[offset] > 8 {
                visible += 1;
            }
        }
    }
    visible
}

#[cfg(target_os = "macos")]
fn configure_transparent_webview_window_on_main_thread(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let _mtm = objc2::MainThreadMarker::new().ok_or_else(|| {
        "configure_transparent_webview_window must run on the main thread".to_string()
    })?;
    let ns_window_ptr = window.ns_window().map_err(|e| e.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("ns_window returned null".to_string());
    }

    unsafe {
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        let clear = NSColor::clearColor();
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&clear));
        ns_window.setHasShadow(false);
    }

    window
        .with_webview(|webview| unsafe {
            let view: &WKWebView = &*webview.inner().cast();
            let draws_background = NSNumber::new_bool(false);
            let key = NSString::from_str("drawsBackground");
            let _: () = objc2::msg_send![view, setValue: &*draws_background, forKey: &*key];
        })
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
pub fn present_prompt_button_pair(
    app: &tauri::AppHandle,
    visual: &tauri::WebviewWindow,
    input: &tauri::WebviewWindow,
) -> Result<(), String> {
    let visual = visual.clone();
    let input = input.clone();
    run_on_main_thread_sync(app, move || {
        present_prompt_button_pair_on_main_thread(&visual, &input)
    })
}

#[cfg(target_os = "macos")]
fn present_prompt_button_pair_on_main_thread(
    visual: &tauri::WebviewWindow,
    input: &tauri::WebviewWindow,
) -> Result<(), String> {
    // Configure both panels before either is shown so the pair can never be observed
    // half-configured. Neither call activates the process or makes it key.
    configure_transparent_webview_window_on_main_thread(visual)?;
    configure_transparent_webview_window_on_main_thread(input)?;
    configure_non_activating_panel_on_main_thread(visual)?;
    configure_non_activating_panel_on_main_thread(input)?;

    // Present the visual panel first and confirm it is actually on screen before the
    // interactive input panel appears; there is no asynchronous gap between the two.
    show_overlay_on_main_thread(visual)?;
    show_overlay_on_main_thread(input)
}

#[cfg(target_os = "macos")]
fn show_overlay_on_main_thread(window: &tauri::WebviewWindow) -> Result<(), String> {
    let ns_window_ptr = window.ns_window().map_err(|error| error.to_string())?;
    if ns_window_ptr.is_null() {
        return Err("ns_window returned null".to_string());
    }
    unsafe {
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        ns_window.orderFrontRegardless();
        let visible: Bool = objc2::msg_send![ns_window, isVisible];
        visible
            .as_bool()
            .then_some(())
            .ok_or_else(|| "Overlay window did not become visible.".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn present_prompt_button_pair(
    _app: &tauri::AppHandle,
    visual: &tauri::WebviewWindow,
    input: &tauri::WebviewWindow,
) -> Result<(), String> {
    visual.show().map_err(|error| error.to_string())?;
    input.show().map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_main_window(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn configure_non_activating_panel(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn configure_transparent_webview_window(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a width×height RGBA buffer (alpha last) whose per-pixel alpha is
    /// chosen by `alpha_at(x, y)`; RGB is opaque white so only alpha governs the
    /// ROI count.
    #[cfg(target_os = "macos")]
    fn rgba_buffer(width: usize, height: usize, alpha_at: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut bytes = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * 4;
                bytes[offset] = 255;
                bytes[offset + 1] = 255;
                bytes[offset + 2] = 255;
                bytes[offset + 3] = alpha_at(x, y);
            }
        }
        bytes
    }

    #[cfg(target_os = "macos")]
    fn central_opaque_count(bytes: &[u8], width: usize, height: usize) -> u64 {
        count_visible_alpha_pixels_in_central_roi(bytes, width, height, 4, width * 4, 3)
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn roi_counts_only_the_central_region_of_a_fully_opaque_image() {
        // A fully opaque 100×100 frame: only the central 27%–73% box (46×46) counts.
        let bytes = rgba_buffer(100, 100, |_, _| 255);
        assert_eq!(central_opaque_count(&bytes, 100, 100), 46 * 46);
        // And that is far above the alive threshold, while a blank frame is zero.
        assert!(46 * 46 > crate::visual_liveness::MIN_VISIBLE_ALPHA_PIXELS);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn roi_ignores_an_opaque_corner_status_bubble() {
        // Opaque only in the four corners (outside 27%–73% on both axes): the ROI
        // must contribute nothing, so a status bubble cannot fake "alive".
        let bytes = rgba_buffer(100, 100, |x, y| {
            let in_corner = (x < 20 || x >= 80) && (y < 20 || y >= 80);
            if in_corner {
                255
            } else {
                0
            }
        });
        assert_eq!(central_opaque_count(&bytes, 100, 100), 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn blank_frame_yields_no_visible_pixels() {
        let bytes = rgba_buffer(100, 100, |_, _| 0);
        assert_eq!(central_opaque_count(&bytes, 100, 100), 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn alpha_threshold_rejects_fringe_at_eight_and_accepts_above() {
        // A single central pixel at alpha 8 is fringe → ignored; at 9 → counted.
        let fringe = rgba_buffer(100, 100, |x, y| if (x, y) == (50, 50) { 8 } else { 0 });
        assert_eq!(central_opaque_count(&fringe, 100, 100), 0);
        let solid = rgba_buffer(100, 100, |x, y| if (x, y) == (50, 50) { 9 } else { 0 });
        assert_eq!(central_opaque_count(&solid, 100, 100), 1);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn alpha_first_pixel_layout_is_supported() {
        // ARGB-style layout: alpha at index 0. A central opaque pixel still counts.
        let mut bytes = vec![0u8; 100 * 100 * 4];
        let offset = (50 * 100 + 50) * 4;
        bytes[offset] = 255; // alpha first
        let count = count_visible_alpha_pixels_in_central_roi(&bytes, 100, 100, 4, 100 * 4, 0);
        assert_eq!(count, 1);
    }

    #[test]
    fn panel_class_action_keeps_existing_native_panel() {
        assert_eq!(
            panel_class_action_for_name("PromptDrawerOverlayPanel"),
            PanelClassAction::AlreadyNativePanel
        );
    }

    #[test]
    fn panel_class_action_converts_tao_wry_to_native_panel() {
        assert_eq!(
            panel_class_action_for_name("TaoWindow"),
            PanelClassAction::ConvertToNativePanel
        );
        assert_eq!(
            panel_class_action_for_name("WryWindow"),
            PanelClassAction::ConvertToNativePanel
        );
    }

    #[test]
    fn panel_diagnostic_format_includes_key_behavior() {
        let report = PanelKeyBehaviorReport {
            label: "prompt-popover".to_string(),
            class_name: "TaoWindow".to_string(),
            action: PanelClassAction::ConvertToNativePanel,
            can_become_key: Some(true),
            can_become_main: Some(true),
        };

        let formatted = format_panel_key_behavior_report(&report);

        assert!(formatted.contains("prompt-popover"));
        assert!(formatted.contains("TaoWindow"));
        assert!(formatted.contains("can_become_key=true"));
        assert!(formatted.contains("can_become_main=true"));
    }

    #[test]
    fn non_activating_panel_configuration_uses_native_nspanel() {
        let source = include_str!("macos_panels.rs");

        assert!(source.contains("PromptDrawerOverlayPanel"));
        assert!(source.contains("NSPanel::class()"));
        assert!(source.contains("isKindOfClass"));
    }

    #[test]
    fn overlay_webview_rejects_key_focus_before_panel_is_shown() {
        let source = include_str!("macos_panels.rs");
        let start = source
            .find("fn configure_non_activating_panel_on_main_thread")
            .unwrap();
        let end = source[start..]
            .find("extern \"C-unwind\" fn reject_webview_key_focus")
            .unwrap();
        let configuration = &source[start..start + end];
        let webview_configuration_start =
            source.find("fn configure_non_key_webview_object").unwrap();
        let webview_configuration_end = source[webview_configuration_start..]
            .find("enum PanelClassAction")
            .unwrap();
        let webview_configuration = &source
            [webview_configuration_start..webview_configuration_start + webview_configuration_end];

        assert!(source.contains("sel!(needsPanelToBecomeKey)"));
        assert!(webview_configuration.contains("class_addMethod"));
        assert!(!webview_configuration.contains("acceptsFirstResponder"));
        assert!(!webview_configuration.contains("set_class"));
        assert!(configuration.contains("configure_non_activating_webview_on_main_thread"));
        assert!(configuration.contains("setAcceptsMouseMovedEvents(true)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prompt_popover_pointer_tracking_stays_active_without_key_focus() {
        let options = prompt_popover_pointer_tracking_options();

        assert!(options.contains(NSTrackingAreaOptions::MouseEnteredAndExited));
        assert!(options.contains(NSTrackingAreaOptions::MouseMoved));
        assert!(options.contains(NSTrackingAreaOptions::ActiveAlways));
        assert!(options.contains(NSTrackingAreaOptions::InVisibleRect));
        assert!(!options.contains(NSTrackingAreaOptions::ActiveInKeyWindow));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_pointer_coordinates_are_converted_to_web_coordinates() {
        assert_eq!(
            top_left_pointer_position(20.0, 75.0, 120.0, false),
            (20.0, 45.0)
        );
        assert_eq!(
            top_left_pointer_position(20.0, 75.0, 120.0, true),
            (20.0, 75.0)
        );
    }

    #[test]
    fn overlay_configuration_and_presentation_are_separate() {
        let source = include_str!("macos_panels.rs");
        let configuration_start = source
            .find("fn configure_non_activating_panel_on_main_thread")
            .expect("native panel configuration helper should exist");
        let configuration_end = source[configuration_start..]
            .find("pub fn show_non_activating_panel")
            .expect("native panel presentation helper should follow configuration");
        let configuration = &source[configuration_start..configuration_start + configuration_end];
        let presentation_start = configuration_start + configuration_end;
        let presentation_end = source[presentation_start..]
            .find("extern \"C-unwind\" fn reject_webview_key_focus")
            .expect("webview configuration should follow presentation");
        let presentation = &source[presentation_start..presentation_start + presentation_end];

        assert!(!configuration.contains("orderFrontRegardless"));
        assert!(presentation.contains("run_on_main_thread_sync"));
        assert!(presentation.contains("configure_non_activating_panel_on_main_thread"));
        assert!(presentation.contains("orderFrontRegardless"));
        assert!(
            presentation
                .find("configure_non_activating_panel_on_main_thread")
                .unwrap()
                < presentation.find("orderFrontRegardless").unwrap()
        );
    }

    #[test]
    fn main_window_activation_remains_separate_from_overlay_configuration() {
        let source = include_str!("macos_panels.rs");

        assert!(source.contains("pub fn activate_main_window"));
        assert!(source.contains("pub fn configure_non_activating_panel"));
    }

    #[test]
    fn target_activation_is_pid_bound_and_dispatched_to_the_main_thread() {
        let source = include_str!("macos_panels.rs");
        let start = source
            .find("pub(crate) fn activate_running_application")
            .unwrap();
        let end = source[start..].find("pub fn activate_main_window").unwrap();
        let activation = &source[start..start + end];

        assert!(activation.contains("run_on_main_thread_sync"));
        assert!(activation.contains("runningApplicationWithProcessIdentifier"));
        assert!(!activation.contains("bundle"));
    }

    #[test]
    fn native_window_configuration_is_dispatched_to_the_main_thread() {
        let source = include_str!("macos_panels.rs");

        assert!(source.contains("pub(crate) fn run_on_main_thread_sync"));
        assert!(source.contains("configure_non_activating_panel_on_main_thread"));
        assert!(source.contains("configure_transparent_webview_window_on_main_thread"));
        assert!(source.matches("run_on_main_thread_sync").count() >= 3);
        assert!(source.matches("MainThreadMarker::new()").count() >= 3);
    }

    #[test]
    fn prompt_button_pair_is_configured_and_presented_as_one_non_activating_transaction() {
        let source = include_str!("macos_panels.rs");
        let start = source
            .find("pub fn present_prompt_button_pair")
            .expect("pair presentation helper should exist");
        let end = source[start..]
            .find("#[cfg(not(target_os = \"macos\"))]")
            .expect("non-macos stub should follow the macos pair helpers");
        let block = &source[start..start + end];

        // Dispatched as a single main-thread transaction.
        assert!(block.contains("run_on_main_thread_sync"));
        assert!(block.contains("present_prompt_button_pair_on_main_thread"));
        // Both panels are configured (never-key + transparency) before any show.
        assert!(
            block
                .matches("configure_non_activating_panel_on_main_thread")
                .count()
                >= 2
        );
        assert!(
            block
                .matches("configure_transparent_webview_window_on_main_thread")
                .count()
                >= 2
        );
        // Visual is shown and verified before the interactive input panel.
        let visual_show = block
            .find("show_overlay_on_main_thread(visual)")
            .expect("visual shown first");
        let input_show = block
            .find("show_overlay_on_main_thread(input)")
            .expect("input shown second");
        assert!(visual_show < input_show);
        // Presentation is non-activating and never makes the process key.
        assert!(!block.contains("makeKeyAndOrderFront"));
        assert!(!block.contains("activateIgnoringOtherApps"));
        assert!(!block.contains("makeKeyWindow"));
    }

    #[test]
    fn pair_show_verifies_native_visibility_after_ordering_front() {
        let source = include_str!("macos_panels.rs");
        let start = source
            .find("fn show_overlay_on_main_thread")
            .expect("overlay show helper should exist");
        let end = source[start..]
            .find("#[cfg(not(target_os = \"macos\"))]")
            .expect("non-macos stubs should follow");
        let helper = &source[start..start + end];

        assert!(helper.contains("orderFrontRegardless"));
        assert!(helper.contains("isVisible"));
        assert!(helper.find("orderFrontRegardless").unwrap() < helper.find("isVisible").unwrap());
    }
}
