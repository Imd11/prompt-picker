use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) const MONITOR_TICK: Duration = Duration::from_secs(10);
pub(crate) const VISUAL_EVIDENCE_STALE: Duration = Duration::from_secs(20);
pub(crate) const PROBE_ACK_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const POINTER_LEASE_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const DRAG_LEASE_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const POPOVER_DISMISS_GRACE: Duration = Duration::from_millis(500);
pub(crate) const MIN_VISIBLE_ALPHA_PIXELS: u64 = 64;

const BACKOFF_DELAYS: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];
const SAME_WEBVIEW_RELOAD_LIMIT: u8 = 2;

/// How long a controlled rebuild waits for destroyed labels to leave Tauri's
/// window registry before giving up on the attempt and entering backoff.
pub(crate) const REGISTRY_REMOVAL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a controlled rebuild waits for the freshly built candidate renderer
/// to report ready before treating the attempt as failed.
pub(crate) const REBUILD_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Monitor cadence while a phased rebuild is in flight. Fast enough to notice
/// the registry clearing promptly, slow enough to never busy-loop.
const REBUILD_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// After a system wake (or display wake) the renderer and its decoded surfaces
/// need a moment to settle before a probe is meaningful. Wait this bounded
/// controller deadline, then issue exactly one nonce probe.
pub(crate) const WAKE_PROBE_DELAY: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VisualHealthStage {
    Initializing,
    Healthy,
    Probing,
    Reloading,
    Rebuilding,
    Backoff,
}

impl VisualHealthStage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Initializing => "initializing",
            Self::Healthy => "healthy",
            Self::Probing => "probing",
            Self::Reloading => "reloading",
            Self::Rebuilding => "rebuilding",
            Self::Backoff => "backoff",
        }
    }

    /// The recovery level a stage corresponds to (for diagnostics):
    /// L0 healthy, L1 probe/force-redraw, L2 same-WebView reload,
    /// L3 controlled rebuild, L4 bounded backoff.
    pub(crate) fn recovery_level(self) -> &'static str {
        match self {
            Self::Initializing | Self::Healthy => "L0",
            Self::Probing => "L1",
            Self::Reloading => "L2",
            Self::Rebuilding => "L3",
            Self::Backoff => "L4",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualProbeRequest {
    pub renderer_instance_id: u64,
    pub recovery_generation: u64,
    pub nonce: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VisualMonitorAction {
    None,
    Probe(VisualProbeRequest),
    Reload,
    Rebuild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualProbeAck {
    pub accepted: bool,
    pub visually_alive: bool,
}

/// The ordered phases of a controlled, registry-safe rebuild. A rebuild never
/// builds the replacement pair until the destroyed labels have provably left the
/// window registry, which is what prevents two Calicos from ever coexisting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RebuildPhase {
    HideOld,
    DestroyOld,
    WaitForRegistryRemoval,
    BuildCandidate,
    AwaitReady,
    Verify,
    Show,
}

impl RebuildPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::HideOld => "hide_old",
            Self::DestroyOld => "destroy_old",
            Self::WaitForRegistryRemoval => "wait_for_registry_removal",
            Self::BuildCandidate => "build_candidate",
            Self::AwaitReady => "await_ready",
            Self::Verify => "verify",
            Self::Show => "show",
        }
    }
}

/// What the monitor should do for the single rebuild transition it is allowed
/// this tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RebuildStep {
    /// Execute this phase's side effect now (hide / destroy / build / show).
    Run(RebuildPhase),
    /// No side effect this tick: either still waiting on a bounded condition or
    /// a pure condition-check just advanced the phase pointer.
    Wait,
    /// The attempt failed for a categorized reason; enter non-terminal backoff.
    Failed(RebuildFailure),
    /// The rebuild completed; resume normal monitoring.
    Finished,
}

/// Categorized rebuild failure reasons. Kept free of arbitrary error strings so
/// diagnostics never leak paths or content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RebuildFailure {
    RegistryNotCleared,
    CandidateNotReady,
}

impl RebuildFailure {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RegistryNotCleared => "registry_not_cleared",
            Self::CandidateNotReady => "candidate_not_ready",
        }
    }
}

#[derive(Debug)]
struct VisualHealthInner {
    stage: VisualHealthStage,
    renderer_instance_id: u64,
    recovery_generation: u64,
    next_probe_nonce: u64,
    pending_probe: Option<(u64, Instant)>,
    instance_started_at: Instant,
    last_visual_receipt: Option<Instant>,
    pair_committed: bool,
    renderer_ready: bool,
    reload_attempts: u8,
    failure_count: usize,
    next_retry_at: Option<Instant>,
    interaction_until: Option<Instant>,
    autosend_active: bool,
    sleeping: bool,
    resume_probing_at: Option<Instant>,
    latest_position: Option<(f64, f64)>,
    ignore_termination_until: Option<Instant>,
    rebuild_phase: Option<RebuildPhase>,
    rebuild_phase_deadline: Option<Instant>,
    rebuild_position: Option<(f64, f64)>,
    last_probe_duration_ms: Option<u128>,
}

impl VisualHealthInner {
    fn new(now: Instant) -> Self {
        Self {
            stage: VisualHealthStage::Initializing,
            renderer_instance_id: 0,
            recovery_generation: 0,
            next_probe_nonce: 0,
            pending_probe: None,
            instance_started_at: now,
            last_visual_receipt: None,
            pair_committed: false,
            renderer_ready: false,
            reload_attempts: 0,
            failure_count: 0,
            next_retry_at: None,
            interaction_until: None,
            autosend_active: false,
            sleeping: false,
            resume_probing_at: None,
            latest_position: None,
            ignore_termination_until: None,
            rebuild_phase: None,
            rebuild_phase_deadline: None,
            rebuild_position: None,
            last_probe_duration_ms: None,
        }
    }

    fn interaction_active(&self, now: Instant) -> bool {
        self.autosend_active
            || self
                .interaction_until
                .is_some_and(|deadline| deadline > now)
    }
}

pub(crate) struct PromptButtonVisualHealthState {
    inner: Mutex<VisualHealthInner>,
}

impl Default for PromptButtonVisualHealthState {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl PromptButtonVisualHealthState {
    fn new(now: Instant) -> Self {
        Self {
            inner: Mutex::new(VisualHealthInner::new(now)),
        }
    }

    pub(crate) fn stage(&self) -> VisualHealthStage {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .stage
    }

    pub(crate) fn diagnostic_identity(&self) -> (u64, u64) {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        (inner.renderer_instance_id, inner.recovery_generation)
    }

    /// Round-trip duration of the most recently accepted probe, in ms (for
    /// diagnostics). `None` until the first probe is acknowledged.
    pub(crate) fn last_probe_duration_ms(&self) -> Option<u128> {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .last_probe_duration_ms
    }

    pub(crate) fn register_instance(
        &self,
        renderer_instance_id: u64,
        pair_committed: bool,
        is_reload: bool,
    ) -> u64 {
        self.register_instance_at(
            renderer_instance_id,
            pair_committed,
            is_reload,
            Instant::now(),
        )
    }

    fn register_instance_at(
        &self,
        renderer_instance_id: u64,
        pair_committed: bool,
        is_reload: bool,
        now: Instant,
    ) -> u64 {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
        inner.renderer_instance_id = renderer_instance_id;
        inner.stage = if is_reload {
            VisualHealthStage::Reloading
        } else {
            VisualHealthStage::Initializing
        };
        inner.pending_probe = None;
        inner.instance_started_at = now;
        inner.last_visual_receipt = None;
        inner.pair_committed = pair_committed;
        inner.renderer_ready = false;
        inner.next_retry_at = None;
        inner.recovery_generation
    }

    pub(crate) fn commit_pair(&self, renderer_instance_id: u64) -> bool {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if inner.renderer_instance_id != renderer_instance_id {
            return false;
        }
        inner.pair_committed = true;
        if inner.renderer_ready {
            inner.stage = VisualHealthStage::Healthy;
            inner.last_visual_receipt = Some(Instant::now());
            inner.reload_attempts = 0;
            inner.failure_count = 0;
        }
        true
    }

    pub(crate) fn note_renderer_ready(&self, renderer_instance_id: u64, ready: bool) -> bool {
        self.note_renderer_ready_at(renderer_instance_id, ready, Instant::now())
    }

    fn note_renderer_ready_at(&self, renderer_instance_id: u64, ready: bool, now: Instant) -> bool {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if inner.renderer_instance_id != renderer_instance_id {
            return false;
        }
        inner.renderer_ready = ready;
        if ready && inner.pair_committed {
            inner.stage = VisualHealthStage::Healthy;
            inner.last_visual_receipt = Some(now);
            inner.pending_probe = None;
            inner.reload_attempts = 0;
            inner.failure_count = 0;
            inner.next_retry_at = None;
        } else if !ready {
            inner.last_visual_receipt = None;
            inner.pending_probe = None;
            inner.stage = VisualHealthStage::Probing;
        }
        inner.pair_committed
    }

    pub(crate) fn may_present(&self, renderer_instance_id: u64) -> bool {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        inner.renderer_instance_id == renderer_instance_id
            && inner.pair_committed
            && matches!(
                inner.stage,
                VisualHealthStage::Healthy | VisualHealthStage::Probing
            )
    }

    pub(crate) fn may_present_current(&self) -> bool {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        inner.renderer_instance_id != 0
            && inner.pair_committed
            && matches!(
                inner.stage,
                VisualHealthStage::Healthy | VisualHealthStage::Probing
            )
    }

    pub(crate) fn allow_show_or_record_position(&self, x: f64, y: f64) -> bool {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.latest_position = Some((x, y));
        (inner.renderer_instance_id == 0 || inner.pair_committed)
            && matches!(
                inner.stage,
                VisualHealthStage::Healthy
                    | VisualHealthStage::Probing
                    | VisualHealthStage::Initializing
            )
    }

    pub(crate) fn latest_position(&self) -> Option<(f64, f64)> {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .latest_position
    }

    pub(crate) fn set_interaction_active(&self, active: bool, drag: bool) {
        self.set_interaction_active_at(active, drag, Instant::now());
    }

    fn set_interaction_active_at(&self, active: bool, drag: bool, now: Instant) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.interaction_until = active.then(|| {
            now + if drag {
                DRAG_LEASE_TIMEOUT
            } else {
                POINTER_LEASE_TIMEOUT
            }
        });
    }

    pub(crate) fn extend_interaction(&self, duration: Duration) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let deadline = Instant::now() + duration;
        if inner
            .interaction_until
            .is_none_or(|current| current < deadline)
        {
            inner.interaction_until = Some(deadline);
        }
    }

    /// Whether any interaction lease (pointer / drag / popover-dismiss /
    /// autosend) is currently active. Recovery uses this to defer L2/L3 and to
    /// avoid running native snapshots mid-interaction.
    pub(crate) fn interaction_active(&self) -> bool {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        inner.interaction_active(Instant::now())
    }

    pub(crate) fn set_autosend_active(&self, active: bool) {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .autosend_active = active;
    }

    pub(crate) fn set_sleeping(&self, sleeping: bool) {
        self.set_sleeping_at(sleeping, Instant::now());
    }

    fn set_sleeping_at(&self, sleeping: bool, now: Instant) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.sleeping = sleeping;
        if sleeping {
            // Pause health deadlines; the system being asleep is NOT a failure,
            // so never declare recovery while suspended.
            inner.resume_probing_at = None;
        } else {
            // Wake: settle for a bounded controller deadline, then issue exactly
            // one probe. Drop any in-flight probe state so the single resumed
            // action is a fresh nonce probe rather than a reload/rebuild.
            inner.pending_probe = None;
            inner.last_visual_receipt = None;
            if matches!(inner.stage, VisualHealthStage::Probing) {
                inner.stage = VisualHealthStage::Healthy;
            }
            inner.resume_probing_at = Some(now + WAKE_PROBE_DELAY);
        }
    }

    /// System memory pressure: confirm the visual is still alive with a probe
    /// but declare no failure. Never wakes a sleeping renderer. Inactive decoded
    /// surfaces are released on the frontend; the active frame is retained.
    pub(crate) fn note_memory_pressure(&self) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if inner.sleeping {
            return;
        }
        // Force the next Healthy tick to confirm with a probe rather than treat
        // old evidence as fresh. This never escalates on its own: a live renderer
        // acks and stays Healthy.
        inner.last_visual_receipt = None;
    }

    pub(crate) fn accept_probe_ack(
        &self,
        renderer_instance_id: u64,
        recovery_generation: u64,
        nonce: u64,
        visible_alpha_pixels: u64,
    ) -> VisualProbeAck {
        self.accept_probe_ack_at(
            renderer_instance_id,
            recovery_generation,
            nonce,
            visible_alpha_pixels,
            Instant::now(),
        )
    }

    fn accept_probe_ack_at(
        &self,
        renderer_instance_id: u64,
        recovery_generation: u64,
        nonce: u64,
        visible_alpha_pixels: u64,
        now: Instant,
    ) -> VisualProbeAck {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let pending_matches = inner
            .pending_probe
            .is_some_and(|(pending_nonce, deadline)| pending_nonce == nonce && now <= deadline);
        if inner.renderer_instance_id != renderer_instance_id
            || inner.recovery_generation != recovery_generation
            || !pending_matches
        {
            return VisualProbeAck {
                accepted: false,
                visually_alive: false,
            };
        }

        // The pending deadline is `issued_at + PROBE_ACK_TIMEOUT`, so the probe
        // round-trip duration is how much of that window has already elapsed.
        let probe_duration_ms = inner.pending_probe.map(|(_, deadline)| {
            let remaining = deadline.saturating_duration_since(now);
            PROBE_ACK_TIMEOUT.saturating_sub(remaining).as_millis()
        });
        inner.last_probe_duration_ms = probe_duration_ms;

        inner.pending_probe = None;
        let visually_alive = visible_alpha_pixels >= MIN_VISIBLE_ALPHA_PIXELS;
        if visually_alive {
            inner.stage = VisualHealthStage::Probing;
            inner.pending_probe = Some((nonce, now + PROBE_ACK_TIMEOUT));
        } else {
            inner.stage = VisualHealthStage::Probing;
            inner.pending_probe = Some((nonce, now));
        }
        VisualProbeAck {
            accepted: true,
            visually_alive,
        }
    }

    pub(crate) fn accept_snapshot_result(
        &self,
        renderer_instance_id: u64,
        recovery_generation: u64,
        nonce: u64,
        visible_alpha_pixels: u64,
    ) -> bool {
        self.accept_snapshot_result_at(
            renderer_instance_id,
            recovery_generation,
            nonce,
            visible_alpha_pixels,
            Instant::now(),
        )
    }

    fn accept_snapshot_result_at(
        &self,
        renderer_instance_id: u64,
        recovery_generation: u64,
        nonce: u64,
        visible_alpha_pixels: u64,
        now: Instant,
    ) -> bool {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let pending_matches = inner
            .pending_probe
            .is_some_and(|(pending_nonce, deadline)| pending_nonce == nonce && now <= deadline);
        if inner.renderer_instance_id != renderer_instance_id
            || inner.recovery_generation != recovery_generation
            || !pending_matches
        {
            return false;
        }
        if visible_alpha_pixels >= MIN_VISIBLE_ALPHA_PIXELS {
            inner.pending_probe = None;
            inner.stage = VisualHealthStage::Healthy;
            inner.last_visual_receipt = Some(now);
            inner.failure_count = 0;
            inner.next_retry_at = None;
        } else {
            inner.pending_probe = Some((nonce, now));
            inner.stage = VisualHealthStage::Probing;
        }
        true
    }

    pub(crate) fn plan_monitor(
        &self,
        desired_visible: bool,
        renderer_ready: bool,
        visual_present: bool,
        input_present: bool,
    ) -> VisualMonitorAction {
        self.plan_monitor_at(
            desired_visible,
            renderer_ready,
            visual_present,
            input_present,
            Instant::now(),
        )
    }

    fn plan_monitor_at(
        &self,
        desired_visible: bool,
        renderer_ready: bool,
        visual_present: bool,
        input_present: bool,
        now: Instant,
    ) -> VisualMonitorAction {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if !desired_visible || inner.sleeping {
            return VisualMonitorAction::None;
        }

        // Post-wake settle: hold all recovery for a bounded deadline so the
        // renderer's surfaces can come back before we probe it.
        if let Some(resume_at) = inner.resume_probing_at {
            if now < resume_at {
                return VisualMonitorAction::None;
            }
            inner.resume_probing_at = None;
        }

        if !visual_present || !input_present {
            if inner.interaction_active(now) {
                return VisualMonitorAction::None;
            }
            inner.stage = VisualHealthStage::Rebuilding;
            inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
            return VisualMonitorAction::Rebuild;
        }

        match inner.stage {
            VisualHealthStage::Initializing | VisualHealthStage::Reloading => {
                if renderer_ready && inner.pair_committed {
                    inner.stage = VisualHealthStage::Healthy;
                    inner.last_visual_receipt = Some(now);
                    return VisualMonitorAction::None;
                }
                if now.duration_since(inner.instance_started_at) < INITIALIZATION_TIMEOUT
                    || inner.interaction_active(now)
                {
                    return VisualMonitorAction::None;
                }
                if inner.reload_attempts < SAME_WEBVIEW_RELOAD_LIMIT {
                    inner.stage = VisualHealthStage::Reloading;
                    VisualMonitorAction::Reload
                } else {
                    inner.stage = VisualHealthStage::Rebuilding;
                    VisualMonitorAction::Rebuild
                }
            }
            VisualHealthStage::Healthy => {
                let stale = inner
                    .last_visual_receipt
                    .is_none_or(|last| now.duration_since(last) >= VISUAL_EVIDENCE_STALE);
                if !stale {
                    return VisualMonitorAction::None;
                }
                inner.next_probe_nonce = inner.next_probe_nonce.wrapping_add(1).max(1);
                let nonce = inner.next_probe_nonce;
                inner.pending_probe = Some((nonce, now + PROBE_ACK_TIMEOUT));
                inner.stage = VisualHealthStage::Probing;
                VisualMonitorAction::Probe(VisualProbeRequest {
                    renderer_instance_id: inner.renderer_instance_id,
                    recovery_generation: inner.recovery_generation,
                    nonce,
                })
            }
            VisualHealthStage::Probing => {
                let expired = inner
                    .pending_probe
                    .is_none_or(|(_, deadline)| deadline <= now);
                if !expired {
                    return VisualMonitorAction::None;
                }
                if inner.interaction_active(now) {
                    if let Some((nonce, _)) = inner.pending_probe {
                        inner.pending_probe = Some((nonce, now + Duration::from_secs(1)));
                    }
                    return VisualMonitorAction::None;
                }
                inner.pending_probe = None;
                inner.stage = VisualHealthStage::Reloading;
                VisualMonitorAction::Reload
            }
            VisualHealthStage::Backoff => {
                if inner.interaction_active(now)
                    || inner.next_retry_at.is_some_and(|deadline| deadline > now)
                {
                    return VisualMonitorAction::None;
                }
                inner.stage = VisualHealthStage::Rebuilding;
                VisualMonitorAction::Rebuild
            }
            VisualHealthStage::Rebuilding => VisualMonitorAction::None,
        }
    }

    pub(crate) fn begin_reload(&self, renderer_instance_id: u64) {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
        inner.renderer_instance_id = renderer_instance_id;
        inner.stage = VisualHealthStage::Reloading;
        inner.pending_probe = None;
        inner.instance_started_at = now;
        inner.last_visual_receipt = None;
        inner.renderer_ready = false;
        inner.pair_committed = true;
        inner.reload_attempts = inner.reload_attempts.saturating_add(1);
        inner.next_retry_at = None;
    }

    pub(crate) fn begin_rebuild_attempt(&self) -> u64 {
        self.begin_rebuild_attempt_at(Instant::now())
    }

    fn begin_rebuild_attempt_at(&self, now: Instant) -> u64 {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
        inner.stage = VisualHealthStage::Rebuilding;
        inner.pending_probe = None;
        inner.last_visual_receipt = None;
        inner.renderer_ready = false;
        inner.pair_committed = false;
        inner.next_retry_at = None;
        inner.ignore_termination_until = Some(now + Duration::from_secs(2));
        // Capture the position before any destruction so the replacement pair
        // appears where the old one was, even if the pointer moves mid-rebuild.
        inner.rebuild_position = inner.latest_position;
        inner.rebuild_phase = Some(RebuildPhase::HideOld);
        inner.rebuild_phase_deadline = None;
        inner.recovery_generation
    }

    /// True while a phased rebuild owns the monitor loop.
    pub(crate) fn rebuild_in_progress(&self) -> bool {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .rebuild_phase
            .is_some()
    }

    /// The phase the rebuild will execute next (for diagnostics/tests).
    pub(crate) fn rebuild_phase(&self) -> Option<RebuildPhase> {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .rebuild_phase
    }

    /// Position captured at rebuild start, used to place the replacement pair.
    pub(crate) fn rebuild_position(&self) -> Option<(f64, f64)> {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .rebuild_position
    }

    /// Stop an in-flight rebuild without recording a failure. Used when the user
    /// hides the cat mid-rebuild: the recovery mechanism must not keep going.
    pub(crate) fn abort_rebuild(&self) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.rebuild_phase = None;
        inner.rebuild_phase_deadline = None;
    }

    /// Advance the phased rebuild by at most one non-blocking transition.
    ///
    /// `visual_present` / `input_present` reflect the live window registry;
    /// `candidate_ready` reflects the freshly built renderer's readiness. Side
    /// effects are reported back via [`RebuildStep`] for the caller to execute;
    /// this method never touches windows itself and never blocks.
    pub(crate) fn advance_rebuild(
        &self,
        visual_present: bool,
        input_present: bool,
        candidate_ready: bool,
    ) -> RebuildStep {
        self.advance_rebuild_at(
            visual_present,
            input_present,
            candidate_ready,
            Instant::now(),
        )
    }

    fn advance_rebuild_at(
        &self,
        visual_present: bool,
        input_present: bool,
        candidate_ready: bool,
        now: Instant,
    ) -> RebuildStep {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let Some(phase) = inner.rebuild_phase else {
            return RebuildStep::Finished;
        };
        match phase {
            RebuildPhase::HideOld => {
                inner.rebuild_phase = Some(RebuildPhase::DestroyOld);
                RebuildStep::Run(RebuildPhase::HideOld)
            }
            RebuildPhase::DestroyOld => {
                inner.rebuild_phase = Some(RebuildPhase::WaitForRegistryRemoval);
                inner.rebuild_phase_deadline = Some(now + REGISTRY_REMOVAL_TIMEOUT);
                RebuildStep::Run(RebuildPhase::DestroyOld)
            }
            RebuildPhase::WaitForRegistryRemoval => {
                if !visual_present && !input_present {
                    // Registry cleared: advance to building, but do not build in
                    // the same tick (one transition per tick).
                    inner.rebuild_phase = Some(RebuildPhase::BuildCandidate);
                    inner.rebuild_phase_deadline = None;
                    RebuildStep::Wait
                } else if inner
                    .rebuild_phase_deadline
                    .is_some_and(|deadline| now >= deadline)
                {
                    inner.rebuild_phase = None;
                    inner.rebuild_phase_deadline = None;
                    RebuildStep::Failed(RebuildFailure::RegistryNotCleared)
                } else {
                    RebuildStep::Wait
                }
            }
            RebuildPhase::BuildCandidate => {
                inner.rebuild_phase = Some(RebuildPhase::AwaitReady);
                inner.rebuild_phase_deadline = Some(now + REBUILD_READY_TIMEOUT);
                RebuildStep::Run(RebuildPhase::BuildCandidate)
            }
            RebuildPhase::AwaitReady => {
                if candidate_ready {
                    inner.rebuild_phase = Some(RebuildPhase::Verify);
                    RebuildStep::Wait
                } else if inner
                    .rebuild_phase_deadline
                    .is_some_and(|deadline| now >= deadline)
                {
                    inner.rebuild_phase = None;
                    inner.rebuild_phase_deadline = None;
                    RebuildStep::Failed(RebuildFailure::CandidateNotReady)
                } else {
                    RebuildStep::Wait
                }
            }
            RebuildPhase::Verify => {
                if visual_present && input_present && candidate_ready {
                    inner.rebuild_phase = Some(RebuildPhase::Show);
                    RebuildStep::Wait
                } else if inner
                    .rebuild_phase_deadline
                    .is_some_and(|deadline| now >= deadline)
                {
                    inner.rebuild_phase = None;
                    inner.rebuild_phase_deadline = None;
                    RebuildStep::Failed(RebuildFailure::CandidateNotReady)
                } else {
                    RebuildStep::Wait
                }
            }
            RebuildPhase::Show => {
                inner.rebuild_phase = None;
                inner.rebuild_phase_deadline = None;
                RebuildStep::Run(RebuildPhase::Show)
            }
        }
    }

    pub(crate) fn generation_is_current(&self, generation: u64) -> bool {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .recovery_generation
            == generation
    }

    pub(crate) fn mark_unresponsive(&self) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.renderer_ready = false;
        inner.last_visual_receipt = None;
        inner.pending_probe = None;
        inner.stage = VisualHealthStage::Probing;
    }

    pub(crate) fn accepts_termination_event(&self) -> bool {
        self.accepts_termination_event_at(Instant::now())
    }

    fn accepts_termination_event_at(&self, now: Instant) -> bool {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        inner
            .ignore_termination_until
            .is_none_or(|deadline| deadline <= now)
    }

    pub(crate) fn record_recovery_failure(&self) {
        self.record_recovery_failure_at(Instant::now());
    }

    fn record_recovery_failure_at(&self, now: Instant) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let delay = BACKOFF_DELAYS[inner.failure_count.min(BACKOFF_DELAYS.len() - 1)];
        inner.failure_count = inner.failure_count.saturating_add(1);
        inner.stage = VisualHealthStage::Backoff;
        inner.next_retry_at = Some(now + delay);
        inner.pending_probe = None;
        // A failure ends any in-flight phased rebuild; backoff owns the retry.
        inner.rebuild_phase = None;
        inner.rebuild_phase_deadline = None;
    }

    pub(crate) fn interrupt_backoff(&self) {
        self.interrupt_backoff_at(Instant::now());
    }

    fn interrupt_backoff_at(&self, now: Instant) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if matches!(
            inner.stage,
            VisualHealthStage::Backoff | VisualHealthStage::Rebuilding
        ) {
            inner.stage = VisualHealthStage::Backoff;
            inner.next_retry_at = Some(now);
        }
    }

    pub(crate) fn next_monitor_delay(&self) -> Duration {
        let now = Instant::now();
        let inner = self.inner.lock().expect("visual health lock poisoned");
        // While a phased rebuild is in flight, poll quickly (bounded by any
        // phase deadline) so registry clearing and readiness are noticed fast.
        if inner.rebuild_phase.is_some() {
            return inner
                .rebuild_phase_deadline
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(now)
                        .max(Duration::from_millis(50))
                })
                .unwrap_or(REBUILD_POLL_INTERVAL)
                .min(REBUILD_POLL_INTERVAL)
                .max(Duration::from_millis(50));
        }
        let deadline = match inner.stage {
            VisualHealthStage::Probing => inner.pending_probe.map(|(_, deadline)| deadline),
            VisualHealthStage::Backoff => inner.next_retry_at,
            _ => None,
        };
        // Wake promptly at the post-wake settle deadline so the single resumed
        // probe fires on time instead of waiting out a full monitor tick.
        let deadline = match inner.resume_probing_at {
            Some(resume_at) => Some(match deadline {
                Some(existing) => existing.min(resume_at),
                None => resume_at,
            }),
            None => deadline,
        };
        deadline
            .map(|deadline| {
                deadline
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(50))
            })
            .unwrap_or(MONITOR_TICK)
            .min(MONITOR_TICK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_state(now: Instant) -> PromptButtonVisualHealthState {
        let state = PromptButtonVisualHealthState::new(now);
        state.register_instance_at(7, true, false, now);
        state.note_renderer_ready_at(7, true, now);
        state
    }

    #[test]
    fn healthy_static_renderer_is_probed_only_after_stale_deadline() {
        let now = Instant::now();
        let state = ready_state(now);

        assert_eq!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_secs(19)),
            VisualMonitorAction::None
        );
        assert!(matches!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_secs(20)),
            VisualMonitorAction::Probe(_)
        ));
    }

    #[test]
    fn probe_ack_requires_current_instance_generation_nonce_and_pixels() {
        let now = Instant::now();
        let state = ready_state(now);
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, now + VISUAL_EVIDENCE_STALE)
        else {
            panic!("expected probe");
        };

        assert!(
            !state
                .accept_probe_ack_at(
                    probe.renderer_instance_id,
                    probe.recovery_generation,
                    probe.nonce + 1,
                    MIN_VISIBLE_ALPHA_PIXELS,
                    now + VISUAL_EVIDENCE_STALE
                )
                .accepted
        );
        let ack = state.accept_probe_ack_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            now + VISUAL_EVIDENCE_STALE,
        );
        assert_eq!(
            ack,
            VisualProbeAck {
                accepted: true,
                visually_alive: true
            }
        );
        assert!(state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            now + VISUAL_EVIDENCE_STALE,
        ));
        assert_eq!(state.stage(), VisualHealthStage::Healthy);
    }

    #[test]
    fn blank_or_missing_probe_escalates_without_reloading_during_interaction() {
        let now = Instant::now();
        let state = ready_state(now);
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, now + VISUAL_EVIDENCE_STALE)
        else {
            panic!("expected probe");
        };
        let ack = state.accept_probe_ack_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            0,
            now + VISUAL_EVIDENCE_STALE,
        );
        assert!(ack.accepted);
        assert!(!ack.visually_alive);

        state.set_interaction_active_at(true, false, now + VISUAL_EVIDENCE_STALE);
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                true,
                now + VISUAL_EVIDENCE_STALE + PROBE_ACK_TIMEOUT
            ),
            VisualMonitorAction::None
        );
        state.set_interaction_active_at(false, false, now);
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                true,
                now + VISUAL_EVIDENCE_STALE + PROBE_ACK_TIMEOUT + Duration::from_secs(2)
            ),
            VisualMonitorAction::Reload
        );
    }

    #[test]
    fn recovery_never_revives_disabled_or_sleeping_renderer() {
        let now = Instant::now();
        let state = ready_state(now);
        assert_eq!(
            state.plan_monitor_at(false, true, false, false, now + Duration::from_secs(60)),
            VisualMonitorAction::None
        );
        state.set_sleeping(true);
        assert_eq!(
            state.plan_monitor_at(true, true, false, false, now + Duration::from_secs(60)),
            VisualMonitorAction::None
        );
    }

    #[test]
    fn missing_window_pair_rebuilds_as_one_unit() {
        let now = Instant::now();
        let state = ready_state(now);
        assert_eq!(
            state.plan_monitor_at(true, true, true, false, now),
            VisualMonitorAction::Rebuild
        );
        assert_eq!(state.stage(), VisualHealthStage::Rebuilding);
    }

    #[test]
    fn recovery_stages_gate_show_but_keep_latest_position() {
        let now = Instant::now();
        let state = ready_state(now);
        assert!(state.allow_show_or_record_position(10.0, 20.0));
        state.begin_reload(8);
        assert!(!state.allow_show_or_record_position(30.0, 40.0));
        assert_eq!(state.latest_position(), Some((30.0, 40.0)));
    }

    #[test]
    fn partial_pair_is_not_presentable_until_committed_and_ready() {
        let now = Instant::now();
        let state = PromptButtonVisualHealthState::new(now);
        state.register_instance_at(9, false, false, now);
        assert!(!state.note_renderer_ready_at(9, true, now));
        assert!(!state.may_present(9));
        assert!(state.commit_pair(9));
        assert!(state.may_present(9));
    }

    #[test]
    fn repeated_recovery_failures_back_off_but_never_become_terminal() {
        let state = PromptButtonVisualHealthState::default();
        for expected_upper_bound in BACKOFF_DELAYS {
            state.record_recovery_failure();
            assert_eq!(state.stage(), VisualHealthStage::Backoff);
            assert!(state.next_monitor_delay() <= expected_upper_bound);
            state.interrupt_backoff();
            assert!(state.next_monitor_delay() <= Duration::from_millis(50));
        }
    }

    #[test]
    fn blank_canvas_probe_ack_is_accepted_but_not_visually_alive() {
        let now = Instant::now();
        let state = ready_state(now);
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, now + VISUAL_EVIDENCE_STALE)
        else {
            panic!("expected probe");
        };

        let ack = state.accept_probe_ack_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS - 1,
            now + VISUAL_EVIDENCE_STALE,
        );
        assert!(ack.accepted);
        assert!(!ack.visually_alive);
        // A blank ack stays in Probing (awaiting escalation); it must not mark Healthy.
        assert_eq!(state.stage(), VisualHealthStage::Probing);
    }

    #[test]
    fn snapshot_result_only_confirms_health_for_a_fresh_matching_probe() {
        let now = Instant::now();
        let probe_time = now + VISUAL_EVIDENCE_STALE;
        let state = ready_state(now);
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, probe_time)
        else {
            panic!("expected probe");
        };
        // Canvas ack reports alive and opens the snapshot confirmation window.
        assert!(
            state
                .accept_probe_ack_at(
                    probe.renderer_instance_id,
                    probe.recovery_generation,
                    probe.nonce,
                    MIN_VISIBLE_ALPHA_PIXELS,
                    probe_time,
                )
                .visually_alive
        );

        // Stale generation and mismatched nonce are rejected without touching health.
        assert!(!state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation + 1,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            probe_time,
        ));
        assert!(!state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce + 1,
            MIN_VISIBLE_ALPHA_PIXELS,
            probe_time,
        ));
        assert_eq!(state.stage(), VisualHealthStage::Probing);

        // A blank snapshot keeps the renderer in Probing rather than Healthy.
        assert!(state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            0,
            probe_time,
        ));
        assert_eq!(state.stage(), VisualHealthStage::Probing);

        // A valid snapshot within the confirmation window confirms health.
        assert!(state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            probe_time,
        ));
        assert_eq!(state.stage(), VisualHealthStage::Healthy);
    }

    #[test]
    fn snapshot_confirmation_times_out_after_the_probe_ack_window() {
        let now = Instant::now();
        let probe_time = now + VISUAL_EVIDENCE_STALE;
        let state = ready_state(now);
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, probe_time)
        else {
            panic!("expected probe");
        };
        assert!(
            state
                .accept_probe_ack_at(
                    probe.renderer_instance_id,
                    probe.recovery_generation,
                    probe.nonce,
                    MIN_VISIBLE_ALPHA_PIXELS,
                    probe_time,
                )
                .visually_alive
        );

        // The alive ack re-arms a PROBE_ACK_TIMEOUT window; a snapshot arriving after
        // it expires is rejected.
        let too_late = probe_time + PROBE_ACK_TIMEOUT + Duration::from_millis(1);
        assert!(!state.accept_snapshot_result_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            too_late,
        ));
        assert_eq!(state.stage(), VisualHealthStage::Probing);
    }

    #[test]
    fn interaction_lease_expires_and_then_allows_recovery() {
        let now = Instant::now();
        let state = ready_state(now);
        state.set_interaction_active_at(true, false, now);

        // While the pointer lease is live, a missing input window does not rebuild.
        assert_eq!(
            state.plan_monitor_at(true, true, true, false, now + Duration::from_secs(5)),
            VisualMonitorAction::None
        );
        // Once the lease expires, the same condition rebuilds the pair as one unit.
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                false,
                now + POINTER_LEASE_TIMEOUT + Duration::from_secs(1)
            ),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn explicit_user_reenable_interrupts_backoff_and_rebuilds_promptly() {
        let now = Instant::now();
        let state = ready_state(now);
        state.record_recovery_failure_at(now);
        assert_eq!(state.stage(), VisualHealthStage::Backoff);

        // Before the retry deadline, the monitor stays idle.
        assert_eq!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_millis(100)),
            VisualMonitorAction::None
        );

        // Explicit re-enable clears the wait; the next plan rebuilds immediately.
        state.interrupt_backoff_at(now);
        assert_eq!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_millis(100)),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn old_termination_callback_is_ignored_during_controlled_rebuild() {
        let now = Instant::now();
        let state = ready_state(now);
        let generation = state.begin_rebuild_attempt_at(now);
        assert_eq!(state.stage(), VisualHealthStage::Rebuilding);
        assert!(state.generation_is_current(generation));

        // Deliberately destroying the old WebView fires a stale termination callback;
        // it must be ignored inside the rebuild grace window.
        assert!(!state.accepts_termination_event_at(now + Duration::from_secs(1)));
        // After the window, genuine termination events are honoured again.
        assert!(state.accepts_termination_event_at(now + Duration::from_secs(2)));
    }

    #[test]
    fn show_gate_blocks_presentation_during_all_recovery_stages() {
        let now = Instant::now();
        let state = ready_state(now);
        // Healthy: show_prompt_button may proceed (and thus build the input window).
        assert!(state.allow_show_or_record_position(1.0, 2.0));

        // Reloading / Rebuilding / Backoff: the gate must refuse, so frontend polling
        // can never reach ensure_prompt_button_input_window mid-recovery.
        state.begin_reload(11);
        assert_eq!(state.stage(), VisualHealthStage::Reloading);
        assert!(!state.allow_show_or_record_position(1.0, 2.0));

        state.begin_rebuild_attempt_at(now);
        assert_eq!(state.stage(), VisualHealthStage::Rebuilding);
        assert!(!state.allow_show_or_record_position(1.0, 2.0));

        state.record_recovery_failure_at(now);
        assert_eq!(state.stage(), VisualHealthStage::Backoff);
        assert!(!state.allow_show_or_record_position(1.0, 2.0));
    }

    #[test]
    fn phased_rebuild_advances_one_step_per_tick_and_builds_exactly_once() {
        let now = Instant::now();
        let state = ready_state(now);
        state.begin_rebuild_attempt_at(now);
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::HideOld));

        // HideOld -> DestroyOld.
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now),
            RebuildStep::Run(RebuildPhase::HideOld)
        );
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::DestroyOld));

        // DestroyOld -> WaitForRegistryRemoval (arms the bounded registry wait).
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now),
            RebuildStep::Run(RebuildPhase::DestroyOld)
        );
        assert_eq!(
            state.rebuild_phase(),
            Some(RebuildPhase::WaitForRegistryRemoval)
        );

        // Labels still in the registry: wait, never build.
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now + Duration::from_secs(1)),
            RebuildStep::Wait
        );
        assert_eq!(
            state.rebuild_phase(),
            Some(RebuildPhase::WaitForRegistryRemoval)
        );

        // Registry cleared: advance to BuildCandidate, still without building.
        assert_eq!(
            state.advance_rebuild_at(false, false, false, now + Duration::from_secs(2)),
            RebuildStep::Wait
        );
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::BuildCandidate));

        // BuildCandidate executes exactly once, then awaits readiness.
        assert_eq!(
            state.advance_rebuild_at(false, false, false, now + Duration::from_secs(3)),
            RebuildStep::Run(RebuildPhase::BuildCandidate)
        );
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::AwaitReady));

        // Not ready yet: wait.
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now + Duration::from_secs(4)),
            RebuildStep::Wait
        );
        // Ready: advance to Verify.
        assert_eq!(
            state.advance_rebuild_at(true, true, true, now + Duration::from_secs(5)),
            RebuildStep::Wait
        );
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::Verify));
        // Verified pair present + ready: advance to Show.
        assert_eq!(
            state.advance_rebuild_at(true, true, true, now + Duration::from_secs(6)),
            RebuildStep::Wait
        );
        assert_eq!(state.rebuild_phase(), Some(RebuildPhase::Show));
        // Show runs and completes the rebuild.
        assert_eq!(
            state.advance_rebuild_at(true, true, true, now + Duration::from_secs(7)),
            RebuildStep::Run(RebuildPhase::Show)
        );
        assert!(!state.rebuild_in_progress());
        // A finished rebuild reports Finished and does nothing further.
        assert_eq!(
            state.advance_rebuild_at(true, true, true, now + Duration::from_secs(8)),
            RebuildStep::Finished
        );
    }

    #[test]
    fn rebuild_never_builds_while_old_labels_remain_in_registry() {
        let now = Instant::now();
        let state = ready_state(now);
        state.begin_rebuild_attempt_at(now);
        // Drive to WaitForRegistryRemoval.
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now),
            RebuildStep::Run(RebuildPhase::HideOld)
        );
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now),
            RebuildStep::Run(RebuildPhase::DestroyOld)
        );

        // The registry never clears. Keep ticking (with labels present) until the
        // bounded deadline: we must wait the whole way and then fail without ever
        // emitting a BuildCandidate.
        let mut saw_build = false;
        let mut outcome = None;
        for tick in 1..=12 {
            let t = now + Duration::from_secs(tick);
            match state.advance_rebuild_at(true, true, false, t) {
                RebuildStep::Run(RebuildPhase::BuildCandidate) => saw_build = true,
                RebuildStep::Failed(failure) => {
                    outcome = Some(failure);
                    break;
                }
                _ => {}
            }
        }
        assert!(!saw_build, "must not build while old labels remain");
        assert_eq!(outcome, Some(RebuildFailure::RegistryNotCleared));
        assert!(!state.rebuild_in_progress());

        // The failure is non-terminal: recording it enters backoff, and once the
        // backoff expires the monitor is allowed to try rebuilding again.
        state.record_recovery_failure_at(now + Duration::from_secs(12));
        assert_eq!(state.stage(), VisualHealthStage::Backoff);
        assert_eq!(
            state.plan_monitor_at(true, true, false, false, now + Duration::from_secs(80)),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn rebuild_candidate_ready_timeout_is_categorized_and_non_terminal() {
        let now = Instant::now();
        let state = ready_state(now);
        state.begin_rebuild_attempt_at(now);
        // Run through Hide/Destroy, clear the registry, and build the candidate.
        state.advance_rebuild_at(true, true, false, now);
        state.advance_rebuild_at(true, true, false, now);
        state.advance_rebuild_at(false, false, false, now + Duration::from_secs(1));
        assert_eq!(
            state.advance_rebuild_at(false, false, false, now + Duration::from_secs(2)),
            RebuildStep::Run(RebuildPhase::BuildCandidate)
        );

        // The candidate never becomes ready; after the bounded window we fail.
        let deadline = now + Duration::from_secs(2) + REBUILD_READY_TIMEOUT;
        assert_eq!(
            state.advance_rebuild_at(true, true, false, deadline + Duration::from_millis(1)),
            RebuildStep::Failed(RebuildFailure::CandidateNotReady)
        );
        assert!(!state.rebuild_in_progress());

        // Non-terminal: backoff then a fresh rebuild is still permitted.
        state.record_recovery_failure_at(deadline);
        assert_eq!(state.stage(), VisualHealthStage::Backoff);
        state.interrupt_backoff_at(deadline);
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                false,
                false,
                deadline + Duration::from_millis(10)
            ),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn rebuild_captures_position_before_destruction() {
        let now = Instant::now();
        let state = ready_state(now);
        assert!(state.allow_show_or_record_position(120.0, 240.0));
        state.begin_rebuild_attempt_at(now);
        // During the rebuild the show gate is closed, but the latest position is
        // still tracked; the rebuild must keep the pre-destruction position.
        assert!(!state.allow_show_or_record_position(999.0, 999.0));
        assert_eq!(state.latest_position(), Some((999.0, 999.0)));
        assert_eq!(state.rebuild_position(), Some((120.0, 240.0)));
    }

    #[test]
    fn user_disable_aborts_rebuild_without_recording_failure() {
        let now = Instant::now();
        let state = ready_state(now);
        state.begin_rebuild_attempt_at(now);
        assert_eq!(
            state.advance_rebuild_at(true, true, false, now),
            RebuildStep::Run(RebuildPhase::HideOld)
        );
        assert!(state.rebuild_in_progress());

        // User hides the cat mid-rebuild: the machine stops, without backoff.
        state.abort_rebuild();
        assert!(!state.rebuild_in_progress());
        assert_ne!(state.stage(), VisualHealthStage::Backoff);
    }

    #[test]
    fn rebuild_polls_quickly_but_never_busy_loops() {
        let now = Instant::now();
        let state = ready_state(now);
        state.begin_rebuild_attempt_at(now);
        let delay = state.next_monitor_delay();
        assert!(delay <= super::REBUILD_POLL_INTERVAL);
        assert!(delay >= Duration::from_millis(50));
    }

    #[test]
    fn rebuild_bumps_generation_so_stale_probe_acks_are_rejected() {
        let now = Instant::now();
        let state = ready_state(now);
        let probe_time = now + VISUAL_EVIDENCE_STALE;
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, probe_time)
        else {
            panic!("expected probe");
        };
        let stale_generation = probe.recovery_generation;

        // Starting a rebuild bumps the generation; the in-flight probe's ack,
        // carrying the old generation, must no longer be accepted.
        state.begin_rebuild_attempt_at(probe_time);
        assert!(
            !state
                .accept_probe_ack_at(
                    probe.renderer_instance_id,
                    stale_generation,
                    probe.nonce,
                    MIN_VISIBLE_ALPHA_PIXELS,
                    probe_time,
                )
                .accepted
        );
    }

    #[test]
    fn interaction_lease_covers_pointer_drag_autosend_and_expires_on_event_loss() {
        let now = Instant::now();
        let state = ready_state(now);
        assert!(!state.interaction_active());

        // Pointer lease is active immediately...
        state.set_interaction_active_at(true, false, now);
        assert!(state.interaction_active());
        // ...and, if the pointerup event is lost, it still expires safely.
        let before_expiry = now + POINTER_LEASE_TIMEOUT - Duration::from_millis(1);
        assert!(state
            .inner
            .lock()
            .unwrap()
            .interaction_active(before_expiry));
        let after_expiry = now + POINTER_LEASE_TIMEOUT + Duration::from_millis(1);
        assert!(!state.inner.lock().unwrap().interaction_active(after_expiry));

        // Drag uses a longer safety deadline than a plain pointer lease.
        state.set_interaction_active_at(true, true, now);
        assert!(state
            .inner
            .lock()
            .unwrap()
            .interaction_active(now + POINTER_LEASE_TIMEOUT + Duration::from_secs(1)));
        assert!(!state
            .inner
            .lock()
            .unwrap()
            .interaction_active(now + DRAG_LEASE_TIMEOUT + Duration::from_secs(1)));

        // Autosend is an independent lease source.
        state.set_interaction_active_at(false, false, now);
        assert!(!state.interaction_active());
        state.set_autosend_active(true);
        assert!(state.interaction_active());
        state.set_autosend_active(false);
        assert!(!state.interaction_active());
    }

    #[test]
    fn drag_lease_defers_both_reload_and_rebuild_until_it_expires() {
        let now = Instant::now();
        let state = ready_state(now);
        state.set_interaction_active_at(true, true, now);
        let mid_drag = now + Duration::from_secs(5);

        // L3: a missing input window must not rebuild mid-drag.
        assert_eq!(
            state.plan_monitor_at(true, true, true, false, mid_drag),
            VisualMonitorAction::None
        );
        // L2: an expired probe must not reload mid-drag (it re-arms and waits).
        let probe_time = now + VISUAL_EVIDENCE_STALE;
        let VisualMonitorAction::Probe(probe) =
            state.plan_monitor_at(true, true, true, true, probe_time)
        else {
            panic!("expected probe");
        };
        assert!(
            state
                .accept_probe_ack_at(
                    probe.renderer_instance_id,
                    probe.recovery_generation,
                    probe.nonce,
                    0,
                    probe_time,
                )
                .accepted
        );
        assert_eq!(
            state.plan_monitor_at(true, true, true, true, probe_time + PROBE_ACK_TIMEOUT),
            VisualMonitorAction::None
        );

        // Once the drag lease expires, the same missing pair rebuilds.
        let after_drag = now + DRAG_LEASE_TIMEOUT + Duration::from_secs(1);
        assert_eq!(
            state.plan_monitor_at(true, true, true, false, after_drag),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn stable_popover_grace_blocks_recovery_only_briefly() {
        let now = Instant::now();
        let state = ready_state(now);
        // Dismissing the popover grants only a short hand-off grace.
        state.extend_interaction(POPOVER_DISMISS_GRACE);

        // During the grace, a missing pair does not rebuild...
        assert_eq!(
            state.plan_monitor_at(true, true, true, false, now + Duration::from_millis(100)),
            VisualMonitorAction::None
        );
        // ...but a stably visible popover never holds an unlimited lease: once the
        // grace lapses, recovery proceeds on its own.
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                false,
                now + POPOVER_DISMISS_GRACE + Duration::from_millis(100)
            ),
            VisualMonitorAction::Rebuild
        );
    }

    #[test]
    fn sleep_pauses_recovery_without_declaring_failure() {
        let now = Instant::now();
        let state = ready_state(now);
        state.set_sleeping_at(true, now);
        // Far past the stale deadline AND with a missing pair, nothing recovers
        // while the system is asleep, and no failure stage is entered.
        let far = now + VISUAL_EVIDENCE_STALE + Duration::from_secs(300);
        assert_eq!(
            state.plan_monitor_at(true, true, true, true, far),
            VisualMonitorAction::None
        );
        assert_eq!(
            state.plan_monitor_at(true, true, false, false, far),
            VisualMonitorAction::None
        );
        assert_eq!(state.stage(), VisualHealthStage::Healthy);
    }

    #[test]
    fn wake_waits_then_issues_exactly_one_probe() {
        let now = Instant::now();
        let state = ready_state(now);
        state.set_sleeping_at(true, now);
        let wake = now + Duration::from_secs(600);
        state.set_sleeping_at(false, wake);
        // During the bounded settle window no recovery fires...
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                true,
                wake + WAKE_PROBE_DELAY - Duration::from_millis(1)
            ),
            VisualMonitorAction::None
        );
        // ...and immediately after it, exactly one probe (not a reload/rebuild).
        assert!(matches!(
            state.plan_monitor_at(true, true, true, true, wake + WAKE_PROBE_DELAY),
            VisualMonitorAction::Probe(_)
        ));
        assert_eq!(state.stage(), VisualHealthStage::Probing);
        // While that probe is pending, no second probe/reload is issued.
        assert_eq!(
            state.plan_monitor_at(
                true,
                true,
                true,
                true,
                wake + WAKE_PROBE_DELAY + Duration::from_millis(100)
            ),
            VisualMonitorAction::None
        );
    }

    #[test]
    fn wake_from_pending_probe_probes_again_not_reloads() {
        let now = Instant::now();
        let state = ready_state(now);
        // Drive into Probing with a pending probe.
        assert!(matches!(
            state.plan_monitor_at(true, true, true, true, now + VISUAL_EVIDENCE_STALE),
            VisualMonitorAction::Probe(_)
        ));
        assert_eq!(state.stage(), VisualHealthStage::Probing);
        // Sleep, then wake: the single resumed action must be a probe, never a reload.
        state.set_sleeping_at(true, now + Duration::from_secs(1));
        let wake = now + Duration::from_secs(100);
        state.set_sleeping_at(false, wake);
        assert!(matches!(
            state.plan_monitor_at(true, true, true, true, wake + WAKE_PROBE_DELAY),
            VisualMonitorAction::Probe(_)
        ));
    }

    #[test]
    fn next_monitor_delay_wakes_at_settle_deadline_not_full_tick() {
        let state = ready_state(Instant::now());
        state.set_sleeping(false); // schedules resume ~now + WAKE_PROBE_DELAY
        let delay = state.next_monitor_delay();
        assert!(
            delay <= WAKE_PROBE_DELAY,
            "delay {delay:?} should not exceed the settle window"
        );
        assert!(delay >= Duration::from_millis(50));
    }

    #[test]
    fn memory_pressure_confirms_with_probe_but_never_escalates_or_wakes() {
        let now = Instant::now();
        let state = ready_state(now);
        // Fresh evidence would normally mean "no action yet"...
        assert_eq!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_millis(1)),
            VisualMonitorAction::None
        );
        // ...but pressure forces a confirming probe on the next tick.
        state.note_memory_pressure();
        assert!(matches!(
            state.plan_monitor_at(true, true, true, true, now + Duration::from_millis(2)),
            VisualMonitorAction::Probe(_)
        ));

        // Pressure never wakes a sleeping renderer.
        let now2 = Instant::now();
        let sleeping = ready_state(now2);
        sleeping.set_sleeping_at(true, now2);
        sleeping.note_memory_pressure();
        assert_eq!(
            sleeping.plan_monitor_at(
                true,
                true,
                true,
                true,
                now2 + VISUAL_EVIDENCE_STALE + Duration::from_secs(60)
            ),
            VisualMonitorAction::None
        );
    }

    #[test]
    fn probe_ack_records_round_trip_duration() {
        let now = Instant::now();
        let state = ready_state(now);
        let probe = match state.plan_monitor_at(true, true, true, true, now + VISUAL_EVIDENCE_STALE)
        {
            VisualMonitorAction::Probe(probe) => probe,
            other => panic!("expected a probe, got {other:?}"),
        };
        assert_eq!(state.last_probe_duration_ms(), None);
        // Acknowledge one second after the probe was issued.
        let ack_time = now + VISUAL_EVIDENCE_STALE + Duration::from_secs(1);
        state.accept_probe_ack_at(
            probe.renderer_instance_id,
            probe.recovery_generation,
            probe.nonce,
            MIN_VISIBLE_ALPHA_PIXELS,
            ack_time,
        );
        assert_eq!(state.last_probe_duration_ms(), Some(1000));
    }
}
