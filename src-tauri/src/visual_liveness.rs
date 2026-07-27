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
    latest_position: Option<(f64, f64)>,
    ignore_termination_until: Option<Instant>,
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
            latest_position: None,
            ignore_termination_until: None,
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

    pub(crate) fn recovery_generation(&self) -> u64 {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .recovery_generation
    }

    pub(crate) fn diagnostic_identity(&self) -> (u64, u64) {
        let inner = self.inner.lock().expect("visual health lock poisoned");
        (inner.renderer_instance_id, inner.recovery_generation)
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

    pub(crate) fn set_autosend_active(&self, active: bool) {
        self.inner
            .lock()
            .expect("visual health lock poisoned")
            .autosend_active = active;
    }

    pub(crate) fn set_sleeping(&self, sleeping: bool) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.sleeping = sleeping;
        if !sleeping {
            inner.last_visual_receipt = None;
        }
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

    pub(crate) fn begin_rebuild(&self, renderer_instance_id: u64) {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
        inner.renderer_instance_id = renderer_instance_id;
        inner.stage = VisualHealthStage::Rebuilding;
        inner.pending_probe = None;
        inner.instance_started_at = now;
        inner.last_visual_receipt = None;
        inner.renderer_ready = false;
        inner.pair_committed = false;
        inner.next_retry_at = None;
    }

    pub(crate) fn begin_rebuild_attempt(&self) -> u64 {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        inner.recovery_generation = inner.recovery_generation.wrapping_add(1).max(1);
        inner.stage = VisualHealthStage::Rebuilding;
        inner.pending_probe = None;
        inner.last_visual_receipt = None;
        inner.renderer_ready = false;
        inner.pair_committed = false;
        inner.next_retry_at = None;
        inner.ignore_termination_until = Some(now + Duration::from_secs(2));
        inner.recovery_generation
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
        let now = Instant::now();
        let inner = self.inner.lock().expect("visual health lock poisoned");
        inner
            .ignore_termination_until
            .is_none_or(|deadline| deadline <= now)
    }

    pub(crate) fn record_recovery_failure(&self) {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        let delay = BACKOFF_DELAYS[inner.failure_count.min(BACKOFF_DELAYS.len() - 1)];
        inner.failure_count = inner.failure_count.saturating_add(1);
        inner.stage = VisualHealthStage::Backoff;
        inner.next_retry_at = Some(now + delay);
        inner.pending_probe = None;
    }

    pub(crate) fn interrupt_backoff(&self) {
        let mut inner = self.inner.lock().expect("visual health lock poisoned");
        if matches!(
            inner.stage,
            VisualHealthStage::Backoff | VisualHealthStage::Rebuilding
        ) {
            inner.stage = VisualHealthStage::Backoff;
            inner.next_retry_at = Some(Instant::now());
        }
    }

    pub(crate) fn next_monitor_delay(&self) -> Duration {
        let now = Instant::now();
        let inner = self.inner.lock().expect("visual health lock poisoned");
        let deadline = match inner.stage {
            VisualHealthStage::Probing => inner.pending_probe.map(|(_, deadline)| deadline),
            VisualHealthStage::Backoff => inner.next_retry_at,
            _ => None,
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
}
