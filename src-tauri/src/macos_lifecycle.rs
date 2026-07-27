//! Application-lifetime observers that keep the Calico visual-health controller
//! in sync with macOS lifecycle events.
//!
//! The substantive behavior — pausing deadlines on sleep, settling for a bounded
//! deadline then issuing exactly one probe on wake, and confirming-with-a-probe
//! under memory pressure — lives in `visual_liveness` and is unit-tested there
//! (`set_sleeping`, `note_memory_pressure`). This module only wires the OS
//! signals into that controller.
//!
//! Sleep, wake, and display-wake are observed via `NSWorkspace` notifications.
//! Memory pressure is observed via a libdispatch
//! `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` source (WARN + CRITICAL) that calls the
//! controller's `note_memory_pressure`, which confirms the live frame with a
//! probe rather than escalating.
//!
//! Everything here is `#[cfg(target_os = "macos")]`; on other platforms
//! `install` is a no-op so Windows compiles unchanged.

/// Owns the lifecycle observers for the lifetime of the app. Registered once at
/// setup (so observers never accumulate across rebuilds) and torn down on drop.
pub(crate) struct SystemLifecycleObservers {
    // This field exists purely for its `Drop` (deregisters the observers); it is
    // never read, which is intentional for an RAII guard.
    #[allow(dead_code)]
    #[cfg(target_os = "macos")]
    inner: macos::Observers,
}

/// Register all lifecycle observers. Call once during setup and keep the
/// returned guard managed for the app's lifetime.
pub(crate) fn install(app: &tauri::AppHandle) -> SystemLifecycleObservers {
    SystemLifecycleObservers {
        #[cfg(target_os = "macos")]
        inner: macos::Observers::register(app),
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::visual_liveness::PromptButtonVisualHealthState;
    use block2::RcBlock;
    use dispatch2::{
        dispatch_source_memorypressure_flags_t, dispatch_source_type_t, DispatchObject,
        DispatchRetained, DispatchSource, _dispatch_source_type_memorypressure,
    };
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidWakeNotification,
        NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName};
    use std::ffi::c_void;
    use std::ptr::{addr_of, NonNull};
    use tauri::Manager;

    /// A retained `NSWorkspace` notification observer token.
    ///
    /// The token is an opaque retained object reference; we only ever call
    /// `removeObserver:` on it (from `Drop`) and never share mutable access
    /// across threads, so marking it `Send + Sync` is sound. This lets the guard
    /// live in Tauri managed state, which requires `Send + Sync`.
    struct ObserverHandle(Retained<ProtocolObject<dyn NSObjectProtocol>>);

    // SAFETY: see `ObserverHandle` docs — the handle is only used to deregister.
    unsafe impl Send for ObserverHandle {}
    unsafe impl Sync for ObserverHandle {}

    pub(super) struct Observers {
        observers: Vec<ObserverHandle>,
        // Kept for its `Drop` (cancels the dispatch source); the field itself is
        // never read, which is intentional for an RAII guard.
        #[allow(dead_code)]
        memory_pressure: MemoryPressureSource,
    }

    impl Observers {
        pub(super) fn register(app: &tauri::AppHandle) -> Self {
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            // The NSWorkspace notification-name constants are extern statics;
            // reading them is unsafe (they are immutable and 'static).
            let (will_sleep, did_wake, screens_wake) = unsafe {
                (
                    NSWorkspaceWillSleepNotification,
                    NSWorkspaceDidWakeNotification,
                    NSWorkspaceScreensDidWakeNotification,
                )
            };
            let mut observers = Vec::new();

            // Before sleep: pause health deadlines. Being asleep is NOT a
            // failure, so no recovery is declared while suspended.
            observers.push(ObserverHandle(register_observer(&center, will_sleep, {
                let app = app.clone();
                move |_note| set_sleeping(&app, true)
            })));

            // After system wake and after display wake: settle for a bounded
            // controller deadline, then a single probe. The controller owns the
            // "exactly one probe, not multiple rebuilds" guarantee.
            observers.push(ObserverHandle(register_observer(&center, did_wake, {
                let app = app.clone();
                move |_note| set_sleeping(&app, false)
            })));
            observers.push(ObserverHandle(register_observer(&center, screens_wake, {
                let app = app.clone();
                move |_note| set_sleeping(&app, false)
            })));

            // System memory pressure: confirm the live frame with a probe (the
            // controller owns the "never escalate, never wake a sleeping cat"
            // guarantee). WARN + CRITICAL only — NORMAL fires too often to be
            // actionable for a tiny overlay.
            let memory_pressure = register_memory_pressure(app);

            Self {
                observers,
                memory_pressure,
            }
        }
    }

    impl Drop for Observers {
        fn drop(&mut self) {
            // Remove every observer from the (singleton) workspace notification
            // center so nothing fires after teardown.
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            for handle in &self.observers {
                unsafe { center.removeObserver(handle.0.as_ref()) };
            }
        }
    }

    fn set_sleeping(app: &tauri::AppHandle, sleeping: bool) {
        app.state::<PromptButtonVisualHealthState>()
            .set_sleeping(sleeping);
        crate::record_visual_liveness_event(
            app,
            if sleeping {
                "lifecycle_sleep"
            } else {
                "lifecycle_wake"
            },
        );
    }

    fn register_observer<F>(
        center: &NSNotificationCenter,
        name: &NSNotificationName,
        handler: F,
    ) -> Retained<ProtocolObject<dyn NSObjectProtocol>>
    where
        F: Fn(NonNull<NSNotification>) + Send + 'static,
    {
        let block = RcBlock::new(handler);
        unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
    }

    /// A retained libdispatch memory-pressure source.
    ///
    /// The source is only ever cancelled (from `Drop`) and otherwise driven by
    /// the dispatch runtime; the event handler reads an immutable, leaked
    /// `AppHandle`, so sharing the handle across threads is exactly the dispatch
    /// contract and marking it `Send + Sync` is sound. This lets the guard live
    /// in Tauri managed state, which requires `Send + Sync`.
    struct MemoryPressureSource(DispatchRetained<DispatchSource>);

    // SAFETY: see `MemoryPressureSource` docs — cancellation is thread-safe and
    // the handler only reads a leaked, immutable `AppHandle`.
    unsafe impl Send for MemoryPressureSource {}
    unsafe impl Sync for MemoryPressureSource {}

    impl Drop for MemoryPressureSource {
        fn drop(&mut self) {
            // Cancel the source; the finalizer reclaims the leaked `AppHandle`
            // once the runtime releases its last reference.
            self.0.cancel();
        }
    }

    fn register_memory_pressure(app: &tauri::AppHandle) -> MemoryPressureSource {
        let mask = (dispatch_source_memorypressure_flags_t::DISPATCH_MEMORYPRESSURE_WARN.0
            | dispatch_source_memorypressure_flags_t::DISPATCH_MEMORYPRESSURE_CRITICAL.0)
            as usize;
        // The source type constants are extern statics; taking their address is
        // unsafe (they are immutable and 'static). `handle` is unused (n/a) for
        // memory-pressure sources, and `queue: None` targets the default global
        // priority queue.
        let source = unsafe {
            DispatchSource::new(
                addr_of!(_dispatch_source_type_memorypressure) as dispatch_source_type_t,
                0,
                mask,
                None,
            )
        };
        // Hand the handler a stable `AppHandle` via the source context. It is
        // reclaimed by `free_memory_pressure_context` after cancellation.
        let context = Box::into_raw(Box::new(app.clone())) as *mut c_void;
        unsafe {
            source.set_context(context);
            source.set_finalizer_f(free_memory_pressure_context);
        }
        source.set_event_handler_f(on_memory_pressure);
        source.resume();
        MemoryPressureSource(source)
    }

    extern "C" fn on_memory_pressure(context: *mut c_void) {
        if context.is_null() {
            return;
        }
        // SAFETY: `context` is the live `Box<AppHandle>` installed in
        // `register_memory_pressure`, freed only by the finalizer after cancel.
        let app = unsafe { &*(context as *const tauri::AppHandle) };
        app.state::<PromptButtonVisualHealthState>()
            .note_memory_pressure();
        crate::record_visual_liveness_event(app, "memory_pressure");
    }

    extern "C" fn free_memory_pressure_context(context: *mut c_void) {
        if context.is_null() {
            return;
        }
        // SAFETY: reclaims the exact `Box<AppHandle>` from
        // `register_memory_pressure`; runs once after the source is cancelled.
        unsafe { drop(Box::from_raw(context as *mut tauri::AppHandle)) };
    }
}
