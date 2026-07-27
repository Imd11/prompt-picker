# Sleepy Cat Visual Liveness - Remaining Execution Plan

## Purpose

This is the executable handoff for the unfinished portion of
`2026-07-28-calico-visual-liveness-protocol.md`.

Work directly on local `main` in:

```text
/Users/yang/Desktop/GitHub-pre/sleepy-cat
```

Do not create a worktree or a second branch. Preserve unrelated local build artifacts and do not
commit `dist/`, `src-tauri/target/`, `release/`, or `调研结果/` unless a later user request explicitly
asks for release artifacts.

## Current Implemented Baseline

The following code is already present and must be reviewed rather than rewritten:

- `src-tauri/src/visual_liveness.rs`
  - additive visual-health state separate from renderer readiness;
  - instance/generation/nonce validation;
  - probe, reload, rebuild, and bounded non-terminal backoff decisions;
  - pointer, drag, popover-dismiss, and autosend leases;
  - latest requested position retention.
- `src-tauri/src/visual_liveness_log.rs`
  - asynchronous bounded queue;
  - 512 KiB rotating files with a fixed file count;
  - no prompt, clipboard, URL, target-window content, or screenshot logging.
- `public/overlay.html`
  - `state === 'ready'` no longer bypasses a real redraw;
  - independent `prompt-button-visual-probe` listener;
  - Canvas alpha-pixel acknowledgement separate from renderer readiness.
- `src-tauri/src/lib.rs`
  - native monitor owns probe/reload/rebuild/backoff actions;
  - per-attempt sleeping recovery threads removed;
  - WebContent termination enters the same health controller;
  - autosend activity is mirrored into a native lease;
  - macOS snapshot spike is wired behind `SLEEPY_CAT_VISUAL_SNAPSHOT_SPIKE=1`.
- `src-tauri/src/windows.rs`
  - health gate is before normal window lookup/build/show side effects;
  - visual/input hide order disables input first;
  - partial pair construction performs basic rollback;
  - prompt popover remains outside renderer rebuild ownership.
- `src/App.tsx`
  - existing autosend-active notification starts before popover hide;
  - paste/submit ordering is unchanged.
- `public/overlay-interaction.html`
  - pointer/drag lease reports are fire-and-forget and add no awaited healthy-click step.
- `src-tauri/src/macos_panels.rs`
  - asynchronous `WKWebView.takeSnapshot` spike compiles;
  - snapshot is not enabled by default until good/blank ROI behavior is validated.

Known passing checks at handoff:

```text
cargo check
cargo test visual_liveness --lib        # 9 passed
npm test -- --run src/overlay/overlayHtml.test.ts src/app/App.test.tsx  # 97 passed
```

These checks are not final acceptance.

## Non-Negotiable Existing UX Contracts

Do not regress any of these:

1. Clicking Calico reuses the prewarmed prompt popover and does not wait for a probe or snapshot.
2. The target app/PID is frozen before the popover is shown; browser/window enrichment remains async.
3. `prompt-button` ignores mouse input; `prompt-button-input` owns stable hit testing and dragging.
4. Pointer capture, 10 px drag threshold, requestAnimationFrame move coalescing, and saved position remain.
5. All overlays remain never-key, non-activating NSPanels; recovery must not activate Sleepy Cat.
6. Prompt selection still hides the popover, waits the existing settle delay, then pastes/submits.
7. Existing Codex, Cursor, CLI, Claude, browser, Gemini, ChatGPT, Manus, and WeChat delivery paths are
   outside this task and must not be altered.
8. User-hidden Calico must never be revived by monitoring or recovery.
9. No fallback image layer and no two-Calico overlap.

## Task 1 - Audit and Stabilize the Existing Partial Integration

Files:

- `src-tauri/src/visual_liveness.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/windows.rs`

Steps:

1. Read the complete protocol plan and current diff before editing.
2. Remove genuinely unused health APIs (`recovery_generation`, the obsolete `begin_rebuild` overload)
   only after confirming no planned call site needs them.
3. Add tests for:
   - blank Canvas acknowledgement;
   - snapshot success, blank result, stale generation, duplicate nonce, and timeout;
   - interaction lease expiry;
   - explicit user re-enable interrupting backoff;
   - old termination callback rejection during controlled rebuild.
4. Preserve the instantaneous readiness rule:

```text
prompt_button_ensure_action(true, true, false, false) == None
```

5. Do not turn readiness into health or delete that readiness test.

Exit criteria:

- state transitions are deterministic under injected `Instant` values;
- no healthy-path window operation is added;
- targeted Rust and frontend tests pass.

## Task 2 - Make Visual/Input Window Operations Truly Transactional

Files:

- `src-tauri/src/windows.rs`
- `src-tauri/src/macos_panels.rs`
- tests in both modules

Current gap:

- hide order is correct, but show still uses separate native calls;
- visual construction still begins before the input half is fully committed;
- rollback does not asynchronously wait for Tauri labels to disappear.

Steps:

1. Add one macOS main-thread helper that configures both panels, shows the visual panel, verifies it is
   visible, then shows/enables the input panel. No asynchronous gap may exist between the pair.
2. Add the symmetric hide transaction: disable/hide input first, then hide visual.
3. Recheck visibility generation and health generation immediately before showing the pair.
4. Treat pair construction as a candidate transaction:
   - allocate candidate generation;
   - build both hidden;
   - configure transparency and never-key behavior for both;
   - commit only after both succeed;
   - on failure, invalidate candidate and destroy every created label.
5. Ensure frontend polling cannot call `ensure_prompt_button_input_window` while stage is Reloading,
   Rebuilding, or Backoff.

Tests:

- failure after visual build leaves neither label;
- failure after input build leaves neither label;
- polling during hide/build cannot expose input without visual;
- pair show never calls activating `show` or makes Sleepy Cat key;
- user hide racing with pair show wins.

## Task 3 - Complete Registry-Safe Controlled Rebuild

Files:

- `src-tauri/src/lib.rs`
- `src-tauri/src/windows.rs`
- `src-tauri/src/visual_liveness.rs`

Current gap:

- current rebuild destroys both labels and checks the registry immediately;
- the final plan requires bounded asynchronous registry polling with no main-thread sleep or wait.

Steps:

1. Represent rebuild phases explicitly in the single health controller:

```text
HideOld -> DestroyOld -> WaitForRegistryRemoval -> BuildCandidate -> AwaitReady -> Verify -> Show
```

2. Each monitor tick performs at most one non-blocking phase transition.
3. Never sleep, busy-loop, wait for a channel, or wait for a snapshot on the main thread.
4. Capture current position before destruction and use it for the new pair.
5. Reject callbacks from the old instance/generation instead of relying only on a short time grace.
6. If labels do not leave the registry before the bounded deadline, record a categorized failure and
   enter normal backoff. Continue low-frequency retries; never become terminal.
7. Do not touch `prompt-popover`, `PromptPickSessionState`, `LastInputTargetState`, autosend state, or
   application-specific input profiles.
8. Replace the source-string “no rebuild helper” test with behavioral tests that forbid unconditional
   rebuilds and duplicate labels while allowing one fault-controlled rebuild.

Exit criteria:

- forced same-WebView navigation failure creates exactly one replacement pair;
- no duplicate Calico, no transparent click blocker, no key-window change;
- user disable during any phase cancels show and cannot be undone by a stale callback.

## Task 4 - Finish Interaction Lease Coverage

Files:

- `public/overlay-interaction.html`
- `src-tauri/src/windows.rs`
- `src-tauri/src/lib.rs`
- `src/App.tsx`
- associated tests

Steps:

1. Keep pointer-down reporting fire-and-forget; do not add an awaited IPC call before popover toggle.
2. Cover pointer up/cancel/lost capture, drag start/end, context-menu command, popover mode request and
   acknowledgement, prompt selection, popover-dismiss handoff, and autosend finally cleanup.
3. Use one native lease source of truth with safety deadlines.
4. L1 redraw remains allowed during a lease; L2/L3 is deferred.
5. Do not run native snapshots during click, drag, popover transition, or autosend leases.
6. A stably visible popover must not create an unlimited lease.

Tests:

- event loss expires safely;
- drag cannot be interrupted by L2/L3;
- autosend activity becomes true before popover hide and false in `finally`;
- stable popover does not block recovery forever;
- click-to-popover source order remains target freeze, async enrichment, position, prewarmed show.

## Task 5 - Validate and Integrate the macOS Snapshot Spike

Files:

- `src-tauri/src/macos_panels.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/visual_liveness.rs`

Steps:

1. Run a diagnostic build with:

```text
SLEEPY_CAT_VISUAL_SNAPSHOT_SPIKE=1
```

2. Capture only Sleepy Cat's 288 x 288 visual WebView after a nonce redraw.
3. Record counts for a known-good Calico and an injected blank Canvas.
4. Verify the central Calico ROI excludes the status bubble and distinguishes good from blank across
   Retina/non-Retina scale factors and all normal Calico actions.
5. Verify callback ownership and timeout behavior under WebContent termination.
6. Keep the operation asynchronous; never synchronously receive on the main thread.
7. Only after good/blank separation is proven, replace the environment spike gate with the validated
   production path and centralize the measured threshold.
8. If separation cannot be proven, leave the gate disabled and report P4 as blocked. Do not guess.

Exit criteria:

- injected blank and known-good screenshots are reliably distinguishable;
- no snapshot runs during a critical lease;
- snapshot timeout leads to controller escalation without deadlock;
- no other application is captured.

## Task 6 - Add macOS Lifecycle Recovery

Files:

- a dedicated macOS liveness module or `src-tauri/src/macos_panels.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/visual_liveness.rs`

Steps:

1. Spike and then register application-lifetime observers for system sleep, system wake/display wake,
   and memory pressure.
2. Retain observer/source ownership and remove/cancel it on shutdown.
3. Before sleep, pause health deadlines without declaring failure.
4. After wake, wait a 3-second controller deadline, then issue one nonce probe.
5. After scale/DPR change, finish the existing prepared resize transaction, then probe.
6. Under memory pressure, release only inactive decoded surfaces; retain the active frame, then probe.
7. Keep all code under `#[cfg(target_os = "macos")]`; Windows must compile unchanged.

Exit criteria:

- sleep produces no false recovery;
- wake produces one probe, not multiple rebuilds;
- memory pressure does not remove the active image;
- observers do not accumulate across rebuilds.

## Task 7 - Complete Diagnostics

Files:

- `src-tauri/src/visual_liveness_log.rs`
- recovery call sites

Steps:

1. Keep the existing fixed queue, byte cap, and file-count cap.
2. Log every health transition, probe duration, window pair existence/visibility, recovery level,
   lifecycle event, result category, and timeout category.
3. Never include arbitrary error strings if they may contain paths or content; map them to categories.
4. Preserve the explicit privacy exclusions.
5. Add queue-full, rotation, and shutdown tests.

## Task 8 - Fault Injection and Regression Verification

Automate:

- clear Canvas while readiness remains true;
- Canvas context loss;
- missing/late/duplicate/stale nonce acknowledgements;
- snapshot good/blank/error/timeout;
- WebContent termination;
- same-WebView navigation failure;
- registry removal delay/failure;
- partial pair construction failures;
- user hide and re-enable racing with every recovery phase;
- repeated frontend `show_prompt_button` polling during recovery;
- pointer, drag, context menu, popover transition, and autosend races;
- repeated recovery failure and 60-second capped backoff;
- scale-factor change and sleep/wake.

Run before completion:

```bash
npm test -- --run
npm run build
cd src-tauri && cargo fmt --check
cd src-tauri && cargo test --lib
cd src-tauri && cargo check
```

Also compile the Windows target using the repository's established Windows build environment. Do not
claim Windows verification from a macOS-only `cargo check`.

## Task 9 - Runtime Acceptance

1. Record click-to-popover-open timing on the same Mac and build mode used for the baseline.
2. Verify p95 is no more than 20 ms slower and no liveness IPC is awaited before toggle.
3. Run a 2-hour automated fault/stress session.
4. Run or obtain an 8-hour user soak including normal prompts and sleep/wake.
5. Confirm bounded memory, bounded WebContent process count, no duplicate Calico, no transparent
   blocker, no focus stealing, and unchanged prompt fill/send behavior.

Do not claim the long-running disappearance closed until the silent-blank injection recovers and the
8-hour soak (or equivalent runtime evidence) passes.

## Task 10 - Final Review, Commit, and Push

1. Review the final diff only for this task.
2. Do not commit unrelated generated files already present in the dirty working tree.
3. Confirm `git diff --cached --name-only` contains only intended source, tests, Cargo metadata, and
   plan files.
4. Commit to local `main` with a focused message.
5. Push `main` to `origin/main` only after all automated acceptance checks pass.
6. Report manual/soak evidence separately and honestly.

## Definition of Done

This handoff is complete only when:

- healthy Calico has no new reload/rebuild or user-visible latency;
- silent blank is detected with validated visual evidence;
- the controller automatically recovers indefinitely with bounded resources;
- visual and input windows cannot diverge;
- user-hidden state always wins;
- popover, drag, focus preservation, target capture, and autosend all retain existing behavior;
- macOS and Windows tests/builds pass;
- runtime acceptance evidence is recorded;
- only intended files are committed and pushed to `main`.
