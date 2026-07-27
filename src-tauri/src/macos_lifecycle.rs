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
//! Memory pressure is handled at the controller level; the OS-level dispatch
//! source that would trigger it is intentionally NOT installed here (see the
//! handoff notes — it is left as a runtime-validation item rather than shipped
//! unverified).
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
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidWakeNotification,
        NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName};
    use std::ptr::NonNull;
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

            Self { observers }
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
}
