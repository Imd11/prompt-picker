# Sleepy Cat Visual Liveness Protocol

## Status

- Final consolidated execution plan for this task. It supersedes the solution drafts and risk supplements discussed in chat; no separate supplement plan is required.
- Plan only. Do not implement as part of this document update.
- Target branch: `main`.
- Scope: the floating Calico visual WebView, its input WebView, renderer health state, recovery, and diagnostics.
- Baseline: `cargo test prompt_button_renderer_tests --lib` passes 9 tests before implementation.

## Problem

The native application and menu bar item can remain alive while the floating Calico becomes permanently invisible. The visual WebView may still exist, and the separate input WebView may continue intercepting clicks.

The code currently treats historical renderer readiness as visual health:

- `PromptButtonRendererInner.ready` records initialization/readiness, not current pixels.
- `resumeAndReportReady()` returns success immediately when renderer diagnostics report `state === 'ready'`.
- the native monitor checks window existence, window visibility, and historical readiness, but not end-to-end visual liveness.
- WebContent termination recovery retries navigation three times and has no controlled rebuild escalation.
- page-load recovery hides only the visual window, not the input window.

The bounded Canvas renderer fixed the earlier unbounded decoded-surface growth. It reduced the frequency of failure but did not create a visual liveness protocol across Canvas, WKWebView composition, and the native overlay window.

## User-visible goal

- A healthy Calico behaves exactly as it does today.
- If rendering becomes blank or unresponsive, recovery is automatic.
- An invisible Calico never leaves an input window blocking applications underneath it.
- A user-hidden Calico stays hidden.
- Prompt selection, prompt autosend, dragging, popover behavior, target capture, animation sequencing, and focus preservation remain unchanged.

## Non-goals

- No periodic reload or age-based rebuild of a healthy renderer.
- No second fallback image layer and no overlapping Calico renderers.
- No changes to prompt storage, prompt selection, target detection, paste/submit behavior, or application-specific input profiles.
- No requirement for the user to restart the app or manually recover Calico.
- No continuous screenshots.

## Existing interaction contracts that must not regress

The current implementation contains deliberate latency and focus-preservation optimizations. They are acceptance requirements, not incidental implementation details.

### Instant prompt popover

- `prompt-popover` is prewarmed hidden during application setup.
- A normal click reuses that WebView, repositions it, and calls the non-activating show path.
- Recovery must never close, reload, rebuild, or wait for the prompt popover.
- The healthy click path must not add a liveness probe, snapshot, renderer wait, or new awaited IPC call before `toggle_prompt_popover_from_button`.

### Freeze target before showing the list

The click command must preserve the current order:

```text
prepare_prompt_pick_session_target
start non-blocking capture completion
calculate popover position
show the prewarmed popover
```

The fast synchronous freeze of app/PID/known identity must remain ahead of popover display. Window identity and browser URL enrichment must continue in the background. Renderer recovery must never clear or replace `PromptPickSessionState` or `LastInputTargetState`.

### Stable hit testing and drag behavior

- `prompt-button` remains a mouse-ignoring visual WebView.
- `prompt-button-input` remains the stable interaction WebView.
- Pointer capture, the drag threshold, asynchronous initial-position lookup, and requestAnimationFrame move coalescing remain unchanged.
- L2/L3 recovery must not close the input WebView during an active press, click command, context-menu command, or drag.

### Focus preservation

- All visual/input/popover shows continue through the non-activating NSPanel path.
- Recovery must never call generic activating `show`, make Sleepy Cat key, or activate the Sleepy Cat process.
- Hidden window construction must remain `.visible(false)` and `.focusable(false)` before native panel configuration.

### Prompt selection and autosend

- Prompt selection continues to hide the popover and wait for its existing hide-settle delay before autosend.
- Target activation, paste, submit, captured-window validation, and browser/app input profiles remain untouched.
- Recovery must not hold the macOS main thread while autosend needs it to reactivate the captured process.
- Move the existing `prompt-autosend-activity=true` notification to the beginning of `handleSelect`, before `hidePromptPopover()`. This changes only recovery/polling exclusion timing; the visible sequence remains hide, settle, paste, and optional submit.
- The activity remains true through popover hide, the existing settle delay, target activation, paste, optional submit, and status handling, and is cleared in the existing `finally` path.
- This existing activity period is a critical interaction lease for L2/L3 recovery. Do not introduce a second autosend event or a second source of truth.

### Polling behavior

The main React window intentionally calls `show_prompt_button` repeatedly to retain the floating entry point. During healthy operation this behavior remains unchanged. During `Reloading`, `Rebuilding`, or `Backoff`, the call may update the latest desired button position but must not show a stale window, emit another resume request, or start a competing rebuild.

The health-stage gate must be at the start of `show_prompt_button`, after the user-visibility check but before any window lookup that can lead to size/position mutation, `ensure_prompt_button_input_window`, build, show, or resume side effects.

## Architecture constraints

### 1. Keep readiness and visual health separate

Do not reuse `set_prompt_button_renderer_ready` for liveness probes. That command owns initialization transitions and may hide or show native windows.

Keep the current readiness contract:

```text
ready = the current renderer instance completed initialization
```

Add an independent health controller:

```text
Healthy
Probing
Reloading
Rebuilding
Backoff
```

The health controller stores at least:

```text
renderer_instance_id
recovery_generation
last_frame_receipt: Instant
probe_nonce
probe_deadline: Instant
next_retry_at: Instant
retry_count
stage
```

Use Rust receipt times. Never trust a JavaScript wall-clock timestamp as freshness proof.

### 2. Use an independent nonce probe

Animation frame commits may be emitted as rate-limited supplementary evidence, but cannot be the sole heartbeat. The static baseline intentionally has no frame timer.

When evidence becomes stale, Rust emits a probe containing:

```text
rendererInstanceId + recoveryGeneration + nonce
```

The overlay must:

1. verify that the probe belongs to its renderer instance;
2. call the existing `calicoRenderer.redrawCurrentFrame()` when the renderer is ready;
3. resume and redraw when suspended;
4. reinitialize the Canvas lifecycle when lost;
5. inspect the Calico Canvas region after the draw;
6. acknowledge the same generation and nonce through a new liveness command.

Old instances, old generations, duplicate nonces, and replies received after the deadline must be ignored.

### 3. Do not duplicate the redraw implementation

`public/calico/frame-renderer.js` already exposes `redrawCurrentFrame()`. Reuse it.

Remove the `state === 'ready'` success short circuit in `public/overlay.html`. A recovery request must perform a real draw; historical state is not proof of current pixels.

### 4. Use one native recovery controller

Do not create a sleeping thread for every retry. Extend the existing single native monitor so it owns deadlines, backoff, and the next action.

The controller must be single-flight across:

- stale liveness probes;
- frontend fatal-render notifications;
- WebContent process termination;
- scale-factor changes;
- sleep/wake recovery;
- controlled WebView rebuild.

Mutate state under the renderer mutex, release the mutex, execute Tauri/AppKit side effects, then revalidate generation before applying results. Never hold the renderer mutex while emitting events, waiting for a snapshot, navigating, closing, or building a window.

`show_prompt_button` must consult this controller. `Healthy` and `Probing` keep the existing show behavior. `Reloading`, `Rebuilding`, and `Backoff` suppress native show/resume side effects and retain only the newest requested position.

When user visibility changes from disabled to enabled during `Backoff`, clear the backoff deadline and request one immediate probe/recovery attempt. This is an explicit user request to show Calico and must not wait up to 60 seconds. It still follows normal single-flight and generation checks.

### 5. Recovery levels

```text
L0 Healthy
  no behavior change

L1 Probe and force-redraw
  redraw the current frame and acknowledge the nonce

L2 Reload the current visual WebView
  allocate a new renderer instance and navigate the same WebView

L3 Controlled rebuild
  close the failed visual/input pair and build exactly one new pair

L4 Backoff
  retry after 1s, 5s, 15s, then at most once every 60s
```

L4 must not become a permanent terminal state. A menu-bar warning may expose repeated failure, but automatic low-frequency recovery continues.

### 6. Treat the two overlay windows as one transaction

Create helpers that own both `prompt-button-input` and `prompt-button`:

```text
hide transaction:
  hide/disable input window
  hide visual window

show transaction:
  verify current renderer generation and desired visibility
  show visual window
  enable/show input window
```

Use the transaction from page load, renderer failure, WebContent termination, controlled rebuild, startup, user hide, and recovery success.

The visual/input operations must execute inside one macOS main-thread transaction with no asynchronous wait between them. This prevents the existing 1-2 second frontend polling loop from observing an intermediate state and re-showing the input window while recovery is hiding the pair.

Window construction is also transactional. Building the visual window does not commit a renderer instance until the input window and all native panel configuration succeed. On any partial failure:

1. invalidate the candidate renderer/recovery generation;
2. hide and close every window created by that attempt;
3. wait asynchronously for any created labels to leave the Tauri registry;
4. retain no half-built visual or input window;
5. retry only through the recovery controller.

Every show boundary must recheck `PromptButtonVisibilityState::may_show`. A visibility-generation mismatch cancels the show. Recovery must never revive a user-hidden Calico.

### 7. Preserve current position and interaction state

Before a controlled rebuild, capture the current visual position. Rebuild at that position rather than the startup default.

Do not close or reset the prompt popover as part of renderer recovery. Prompt selection and target capture belong to separate windows and state.

Do not begin L3 while an active pointer drag is in progress. Defer until release or explicitly cancel the interaction and persist the last valid position before rebuilding.

Add an expiring native interaction lease covering:

- pointer down through pointer up/cancel/lost capture;
- `toggle_prompt_popover_from_button` until its command returns;
- `show_prompt_button_controls_from_button` until its command returns;
- drag start through drag end;
- a pending/opening popover command, popover-mode acknowledgement, prompt selection, and popover hide/settle transition;
- `prompt-autosend-activity` while active.

The provisional pointer lease must be reported without adding an awaited step to the healthy click path. It must have a safety deadline so a lost event cannot block recovery forever. L1 redraw is allowed during a lease. L2/L3 is deferred until the lease ends, except that an already invisible input window may still be disabled immediately to prevent click blocking.

A popover that is already stably visible is not an unlimited recovery lease. Renderer recovery may continue because it owns only the visual/input button pair, but it must not close, reload, move, resize, or reset the popover. After popover dismissal, retain a short recovery grace window until the early `prompt-autosend-activity=true` event is observed or the grace expires.

Recovery must not call `calicoIdleDirector.start()`, `resetToBaseline()`, or otherwise restart the current motion during an L1 probe. `redrawCurrentFrame()` redraws the current protected wake/sleep/drag frame in place. L2/L3 may reset to baseline only after a confirmed renderer failure because the previous renderer no longer exists.

### 8. Controlled rebuild discipline

Controlled rebuild must run on the macOS main thread and must not use `close()` followed immediately by a same-label `build()`.

"Run on the main thread" does not permit blocking the main thread. Registry removal must be checked in bounded asynchronous steps. Do not sleep, busy-loop, synchronously wait for a snapshot callback, or hold a mutex on the main thread. This preserves prompt popover display and autosend target activation latency.

Required sequence:

1. enter `Rebuilding` and allocate a new recovery generation;
2. recheck desired visibility;
3. hide the input and visual windows transactionally;
4. close the old input and visual windows;
5. wait with a bounded timeout until both Tauri labels are absent from the registry;
6. abort if another generation superseded this rebuild;
7. build one hidden visual window and one hidden input window;
8. configure transparency, non-activating NSPanel behavior, never-key WebView behavior, position, and event routing;
9. initialize and visually verify the new renderer;
10. show the visual window and then the input window only if visibility generation is still current.

Closing old windows may produce pagehide or termination callbacks. Those callbacks must be rejected as stale by the recovery generation and must not start a second rebuild.

The existing source-string test that forbids all runtime rebuilds must be replaced by behavioral tests that forbid unconditional/age-based rebuilds and duplicate windows while permitting this controlled fault-only path.

### 9. Native snapshot is corroborating evidence

Implement a macOS-only spike before relying on `WKWebView.takeSnapshot`.

The spike must establish:

- whether a hidden overlay WebView produces a reliable snapshot;
- the correct asynchronous callback and ownership model with `objc2-web-kit`;
- that no synchronous wait occurs on the macOS main thread;
- a bounded timeout and safe cancellation behavior;
- the Calico region of interest and transparent-pixel threshold.

Snapshot only the app's own 288 x 288 visual WebView, and only after a suspicious probe or recovery attempt. Never capture other applications.

Do not start a snapshot while a click/context-menu command, popover mode transition, drag, or autosend interaction lease is active. Snapshot work is diagnostic/recovery work and never belongs to the user-input critical path.

A snapshot validates the WebKit composition result, not guaranteed final WindowServer presentation. A valid snapshot is corroborating evidence; timeout escalation remains authoritative.

P1-P3 are implementation milestones, not proof that the original silent-compositor failure is fixed. P4 visual verification is a mandatory final acceptance dependency. If the snapshot spike cannot reliably distinguish a known-good Calico ROI from an injected blank composition, stop final rollout and document the blocked verification gap; do not invent an unvalidated threshold or claim completion.

### 10. System lifecycle handling

macOS-only lifecycle hooks require a spike and explicit ownership cleanup:

- before sleep: pause liveness deadlines;
- after wake/display wake: wait for a short grace period, then probe;
- WebContent termination: enter L2 immediately;
- scale/DPR change: keep the existing prepared resize transaction, then probe;
- memory pressure: release only inactive decoded surfaces, retain the active frame, then probe.

Observers and dispatch sources must be retained for the application lifetime and removed/cancelled during shutdown. All code must remain under `#[cfg(target_os = "macos")]` so Windows builds continue to compile.

### 11. Persistent diagnostics

Write asynchronous rolling logs to the application log directory. Apply a strict file-count and byte cap.

Record only:

- renderer instance and recovery generation;
- probe nonce and elapsed durations;
- health-state transitions;
- window existence/visibility;
- recovery level, result, and error category;
- lifecycle event and memory-pressure level.

Never log prompt text, clipboard contents, user input, target-window content, browser URLs, or screenshots.

## Initial timing constants

Keep constants centralized and testable:

```text
monitor tick:              10 seconds
visual evidence stale:     20 seconds
probe acknowledgement:      3 seconds
initialization deadline:    30 seconds
same-WebView reload limit:   2 attempts
backoff:                     1s, 5s, 15s, then 60s
wake grace period:           3 seconds
pointer/click lease safety: 10 seconds
drag lease safety:          30 seconds
popover-dismiss grace:     500 milliseconds
```

These are initial values, not scattered magic numbers. Tune only from logs and soak results.

## Implementation phases

### P0 - Characterization tests and rolling logs

Files:

- `src-tauri/src/lib.rs`
- a small dedicated diagnostics module under `src-tauri/src/`
- `src-tauri/Cargo.toml` only if a bounded logging dependency is required

Tasks:

- Add pure decision tests for the existing readiness and visibility behavior before changing it.
- Record a debug-only baseline for click-to-popover-open latency and target-freeze ordering.
- Add characterization tests for prewarming/reuse, non-activating show, pointer capture/drag coalescing, polling pause during drag/autosend, and prompt-selection hide ordering.
- Add bounded rolling logs without changing recovery behavior.
- Preserve the current nine passing `prompt_button_renderer_tests` as the baseline.

Exit criteria:

- no behavior change;
- logs are bounded and contain no user content;
- no additional awaited operation exists on the healthy click path;
- macOS and Windows compile.

The latency comparison is a local macOS acceptance measurement over repeated runs, not a cross-machine CI hard threshold.

### P1 - Independent liveness probe

Files:

- `public/overlay.html`
- `public/calico/frame-renderer.js`
- `src-tauri/src/lib.rs`
- `src/overlay/overlayHtml.test.ts`
- `src/overlay/calicoFrameRenderer.test.ts`

Tasks:

- Reuse `redrawCurrentFrame()`.
- Remove the ready-state redraw short circuit.
- Add a new probe event and a new probe-ack command, separate from renderer readiness.
- Add nonce/generation/deadline validation.
- Rate-limit optional frame-commit evidence.

Exit criteria:

- ready-but-blank fault injection causes a real redraw;
- static baseline is not falsely declared dead;
- stale and duplicate acknowledgements are rejected;
- readiness commands are not emitted by normal probes.

### P2 - Additive health controller and window transaction

Files:

- `src-tauri/src/lib.rs`
- `src-tauri/src/windows.rs`
- associated unit tests

Tasks:

- Add the health state alongside, not instead of, renderer readiness.
- Convert the monitor to a pure action-decision function plus side-effect executor.
- Add single-flight recovery and centralized retry deadlines.
- Gate `show_prompt_button` by health stage while retaining the latest requested position.
- Add the expiring interaction lease without adding a healthy-path awaited round trip.
- Establish the autosend lease by moving the existing activity-true notification before popover hide; keep the activity-false cleanup in `finally`.
- Introduce transactional hide/show helpers for visual and input windows.
- Add candidate-pair construction with rollback on visual, input, or panel-configuration failure.
- Migrate page-load and fatal-render paths to the transaction.

Exit criteria:

- `prompt_button_ensure_action(true, true, false, false) == None` remains valid as an instantaneous readiness rule;
- prolonged unready/stale health escalates separately;
- user-disabled visibility always wins;
- invisible visual window cannot leave a visible input window.
- repeated frontend polling cannot re-show either window during L2/L3;
- frontend polling cannot create or ensure an input window while L2/L3/Backoff is active;
- partial pair construction leaves no registered visual or input label;
- a click, drag, context menu, popover transition, or autosend lease defers L2/L3 without suppressing L1;
- a stably visible popover does not defer recovery indefinitely;
- explicit user re-enable interrupts Backoff and requests an immediate single-flight attempt;
- prewarmed popover reuse and target capture ordering remain unchanged.

### P3 - Same-WebView reload and controlled rebuild

Files:

- `src-tauri/src/lib.rs`
- `src-tauri/src/windows.rs`
- `src-tauri/src/macos_panels.rs` only for existing panel reconfiguration hooks

Tasks:

- Route WebContent termination through the health controller.
- Replace per-attempt sleeping threads with controller deadlines.
- Implement registry-safe close/wait/build for the visual/input pair.
- Preserve current position and non-activating panel configuration.
- Keep popover, prompt-pick target, recent target, autosend, and idle-motion state outside the rebuild transaction.
- Implement registry polling as non-blocking main-thread steps.
- Replace the source-string no-rebuild test with behavioral uniqueness tests.

Exit criteria:

- forced navigation failure escalates to exactly one rebuilt pair;
- no duplicate labels, duplicate Calico images, focus stealing, or transparent click blockers;
- no generic activating window show is introduced;
- popover and autosend commands remain responsive while recovery is pending;
- closing during rebuild cannot revive Calico;
- repeated failure enters bounded low-frequency backoff rather than stopping forever.

### P4 - Native snapshot and lifecycle hooks

Files:

- `src-tauri/src/macos_panels.rs` or a dedicated macOS liveness module
- `src-tauri/src/lib.rs`

Tasks:

- Complete snapshot, sleep/wake, display-wake, and memory-pressure spikes first.
- Integrate only mechanisms proven safe by those spikes.
- Keep snapshot corroborating and keep recovery timeout authoritative.
- Defer snapshot and L2/L3 lifecycle recovery while a critical interaction lease is active.

Exit criteria:

- no main-thread deadlock;
- snapshot does not add latency to click, popover, drag, or autosend paths;
- sleep does not cause false recovery;
- wake triggers one probe after grace;
- memory pressure never removes the active frame;
- Windows remains unaffected.
- an injected blank composition is distinguishable from the known-good Calico ROI; otherwise final rollout remains blocked.

### P5 - Fault injection and soak verification

Automated cases:

- ready state with cleared Canvas;
- context loss;
- JavaScript that does not acknowledge a probe;
- WebContent termination;
- same-WebView navigation failure;
- stale acknowledgement after timeout;
- user hide racing with probe, reload, and rebuild;
- repeated `show_prompt_button` polling racing with transactional hide/show;
- repeated `show_prompt_button` polling racing with partial pair construction failure;
- pointer down, click command, context menu, and drag racing with L2/L3;
- popover mode acknowledgement racing with recovery;
- a stably visible popover during visual/input recovery;
- the popover-dismiss-to-autosend handoff window;
- autosend target activation racing with recovery;
- drag and popover use during recovery;
- scale-factor change;
- repeated failures and backoff;
- macOS and Windows build/test suites.

Manual/soak cases:

- 2-hour automated stress run;
- 8-hour user soak including sleep/wake and normal prompt use;
- memory and WebContent process count remain bounded;
- no invisible click-blocking region;
- no duplicate Calico and no focus regression;
- prompt fill/send behavior remains unchanged.
- click-to-popover-open p95 is no more than 20 ms slower than the P0 baseline;
- target app/PID is frozen before popover display in every tested click path;

The p95 latency check is performed on the same Mac, app build mode, and test procedure as the P0 baseline. It is a release acceptance check, not a portable CI assertion.

Do not claim the long-running disappearance is closed until the 8-hour soak passes or equivalent runtime evidence demonstrates recovery from an injected silent-blank condition.

## Test migration rules

- Do not delete the test that prevents an unready transparent window from being shown. Keep its instantaneous contract.
- Replace the test that forbids all health polling with a test that forbids frontend-owned autonomous recovery while permitting native nonce probes.
- Replace the test that forbids all rebuilds with tests that prohibit healthy/age-based rebuilds and assert one fault-controlled visual/input pair.
- Preserve focus, popover, drag, prompt selection, autosend, settings visibility, and cross-platform compilation tests.
- Add a regression test that recovery never closes or rebuilds `prompt-popover`.
- Add a regression test that health probing never calls renderer readiness or restarts the idle director.
- Add a regression test that `show_prompt_button` cannot undo a recovery transaction.
- Add a regression test that the health gate runs before `ensure_prompt_button_input_window` and all build/show/resume side effects.
- Add a regression test that partial pair creation rolls back every created label and invalidates its candidate instance.
- Add a regression test that autosend activity becomes true before popover hide while paste/submit ordering remains unchanged.
- Add a regression test that a stable visible popover does not create an unlimited recovery lease.
- Add a regression test that explicit re-enable interrupts Backoff without bypassing single-flight recovery.
- Add a regression test that no wait/sleep/snapshot completion occurs on the main thread.

## Pareto guardrails

The change is accepted only if all are true:

- healthy path performs no reload or rebuild;
- no additional user action is required;
- existing prompt and autosend code is untouched except for regression tests;
- a hidden Calico is never revived;
- recovery never steals key-window focus;
- healthy click-to-popover behavior adds no awaited liveness operation;
- target freezing and background identity enrichment retain their current ordering;
- prompt popover prewarming/reuse is untouched;
- pointer capture, drag throttling, polling pause, and autosend focus recovery are untouched;
- pair construction is all-or-nothing;
- a stable visible popover never creates a permanent recovery terminal state;
- visual and input windows cannot diverge;
- retries and logs are bounded;
- WebContent/window counts remain bounded;
- Windows behavior and builds are unchanged.

## Final execution gate

Implementation may begin only after:

1. this plan is reviewed against the current `main` branch;
2. the snapshot and lifecycle spikes have explicit pass/fail criteria;
3. readiness and health are represented as separate APIs;
4. controlled rebuild owns both overlay windows and has a registry-removal timeout;
5. the baseline targeted Rust tests remain green.
6. `show_prompt_button` has an explicit health-stage gate;
7. click/drag/popover/autosend critical paths have a bounded interaction lease;
8. no recovery operation can synchronously wait on the macOS main thread;
9. P0 records the latency and ordering baseline used by final acceptance.
10. candidate visual/input construction has explicit rollback and stale-instance invalidation;
11. autosend activity begins before popover hide, with no paste/submit ordering change;
12. a stable visible popover is excluded from unlimited leases;
13. successful P4 blank-vs-good visual verification is mandatory before claiming completion.
