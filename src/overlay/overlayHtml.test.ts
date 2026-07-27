import { existsSync, readFileSync } from "fs";
import { describe, expect, it } from "vitest";

function readOverlayHtml(): string {
  return readFileSync("public/overlay.html", "utf8");
}

function readOverlayInteractionHtml(): string {
  return readFileSync("public/overlay-interaction.html", "utf8");
}

describe("overlay button html", () => {
  it("enables global Tauri for the vanilla overlay html", () => {
    const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));

    expect(config.app?.withGlobalTauri).toBe(true);
  });

  it("opens Tauri button controls on right click instead of an inline menu", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("window.__TAURI__");
    expect(html).toContain("contextmenu");
    expect(html).toContain("Tauri invoke API is unavailable");
    expect(html).toContain("Tauri command failed");
    expect(html).toContain("toggle_prompt_popover_from_button");
    expect(html).toContain("show_prompt_button_controls_from_button");
    expect(html).toContain("prompt-button-drag-started");
    expect(html).toContain("prompt-button-drag-ended");
    expect(html).toContain("event.button === 2");
    expect(html).toContain("event.ctrlKey");
    expect(html).toContain("start.positionReady = false");
    expect(html).not.toContain('id="menu"');
    expect(html).not.toContain("hide_prompt_button");
  });

  it("renders Calico through exactly one visible canvas", () => {
    const html = readOverlayHtml();
    const interactionHtml = readOverlayInteractionHtml();

    expect(html).toContain("calico-visual");
    expect(html).toContain('id="calicoCanvas"');
    expect(html).toContain('class="calico-sprite"');
    expect(html).toContain('width="252" height="252"');
    expect(html).not.toContain('id="calicoSprite"');
    expect(html).not.toContain("calico-action-sprite");
    expect(html).not.toContain("<img");
    expect(html).toContain('data-motion-state="idle-follow"');
    expect(html).not.toContain("calico-projectile");
    expect(html).not.toContain("promptProjectile");
    expect(interactionHtml).toContain('aria-label="Open Sleepy Cat"');
    expect(html).not.toContain("calico-rig");
    expect(html).not.toContain("calico-body");
    expect(html).not.toContain("calico-head");
    expect(html).not.toContain('class="calico-svg"');
    expect(html).not.toContain("<span>Prompts</span>");
  });

  it("loads the renderer, runtime, and both manifests with one asset version", () => {
    const html = readOverlayHtml();

    expect(html).toContain("const CALICO_ASSET_VERSION");
    expect(html).toContain("versionedCalicoAsset('/calico/frame-renderer.js')");
    expect(html).toContain("versionedCalicoAsset('/calico/motion-runtime.js')");
    expect(html).toContain("/calico/motion-runtime.js");
    expect(html).toContain("createCalicoMotionRuntime");
    expect(html).toContain("versionedCalicoAsset('/calico/idle-director.js')");
    expect(html).toContain("/calico/idle-director.js");
    expect(html).toContain("createCalicoIdleDirector");
    expect(html).toContain("initializeCalicoMotion");
    expect(html).toContain("fetch(versionedCalicoAsset('/calico/manifest.json'), { cache: 'no-store' })");
    expect(html).toContain("fetch(versionedCalicoAsset('/calico/sheets/manifest.json'), { cache: 'no-store' })");
    expect(html).toContain("createCalicoFrameRenderer");
    expect(html).toContain("calico-motion");
    expect(html).not.toContain("from '/calico/motion-runtime.js'");
    expect(html).not.toContain("from '/calico/idle-director.js'");
    expect(html).not.toContain("fetch('/calico/manifest.json')");
  });

  it("loads and starts the Calico idle director after the motion runtime is ready", () => {
    const html = readOverlayHtml();

    expect(html).toContain("/calico/idle-director.js");
    expect(html).toContain("createCalicoIdleDirector");
    expect(html).toContain("await loadCalicoModules();");
    expect(html).toContain("let calicoIdleDirector = null;");
    expect(html).toContain("calicoIdleDirector = createCalicoIdleDirector");
    expect(html).toContain("applyMotion: applyCalicoMotion");
    expect(html).toContain("resetMotion: resetCalicoMotion");
    expect(html).toContain("getCurrentState: () => visual.dataset.motionState || calicoManifest.defaultState");
    expect(html).toContain("isUserActive: () => interactionActive");
    expect(html).toContain("calicoIdleDirector.start();");
    expect(html.indexOf("const baselineReady = await calicoRenderer.showBaseline()")).toBeLessThan(
      html.indexOf("calicoMotion = createCalicoMotionRuntime")
    );
  });

  it("pauses idle motions around deliberate pointer and semantic actions", () => {
    const html = readOverlayHtml();

    expect(html).toContain("pauseIdleForExternalMotion(event.payload);");
    expect(html).toContain("pauseIdleForPointerInteraction(5_000);");
    expect(html).toContain("pauseIdleForPointerInteraction(6_000);");
    expect(html).toContain("calicoIdleDirector?.resetIdleClock();");
    expect(html).toContain("pauseIdleForPointerInteraction(4_000);");
    expect(html).toContain("calicoIdleDirector.resetToBaseline();");
  });

  it("wakes Calico as soon as the pointer press begins", () => {
    const html = readOverlayHtml();

    const pressBlock = html.slice(
      html.indexOf("case 'press':"),
      html.indexOf("case 'drag-start':")
    );

    expect(pressBlock).toContain("calicoIdleDirector?.handleClickAttention();");
    expect(pressBlock.indexOf("handleClickAttention"))
      .toBeLessThan(pressBlock.indexOf("interactionActive = true"));
  });

  it("opens the prompt list through one atomic backend command", () => {
    const html = readOverlayInteractionHtml();
    const clickBlock = html.slice(
      html.indexOf("const permission = await invoke('prompt_interaction_permission_status');"),
      html.indexOf("btn.addEventListener('pointerenter'")
    );

    expect(clickBlock).toContain("prompt_interaction_permission_status");
    expect(clickBlock).toContain("handleMissingPromptInteractionPermission(permission)");
    expect(clickBlock).toContain("toggle_prompt_popover_from_button");
    expect(clickBlock).not.toContain("begin_prompt_pick_session");
    expect(clickBlock).not.toContain("sessionPromise");
  });

  it("clears only the completed pointer interaction before awaiting the click action", () => {
    const html = readOverlayInteractionHtml();
    const pointerUpStart = html.indexOf("btn.addEventListener('pointerup'");
    const pointerEnterStart = html.indexOf("btn.addEventListener('pointerenter'");
    const pointerUpBlock = html.slice(pointerUpStart, pointerEnterStart);

    expect(pointerUpBlock).toContain("const interaction = start;");
    expect(pointerUpBlock).toContain("clearPointerInteraction(interaction.sequence)");
    expect(pointerUpBlock.indexOf("clearPointerInteraction(interaction.sequence)")).toBeLessThan(
      pointerUpBlock.indexOf("prompt_interaction_permission_status")
    );
    expect(html).toContain("if (start?.sequence !== sequence) return false;");
  });

  it("coalesces repeated primary clicks while the first toggle is still running", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("let primaryActionInFlight = false;");
    expect(html).toContain("if (primaryActionInFlight) {");
    expect(html).toContain("primaryActionInFlight = true;");
    expect(html).toContain("primaryActionInFlight = false;");
  });

  it("cleans up cancelled pointer interactions without changing the drag threshold", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("btn.addEventListener('pointercancel'");
    expect(html).toContain("btn.addEventListener('lostpointercapture'");
    expect(html).toContain("const DRAG_START_DISTANCE_PX = 10;");
  });

  it("keeps existing drag and click commands for the Calico entry", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("prompt_button_position_cmd");
    expect(html).toContain("move_prompt_button_to");
    expect(html).toContain("toggle_prompt_popover_from_button");
    expect(html).toContain("prompt-button-drag-started");
    expect(html).toContain("prompt-button-drag-ended");
    expect(html).toContain("setPointerCapture");
    expect(html).toContain("releasePointerCapture");
  });

  it("separates the mouse-transparent visual canvas from the centered interaction hit area", () => {
    const html = readOverlayHtml();
    const interactionHtml = readOverlayInteractionHtml();

    expect(html).toContain("--calico-hit-area-size: 132px");
    expect(html).toContain("--calico-sprite-size: 126px");
    expect(html).toContain("width: var(--calico-sprite-size);");
    expect(html).toContain("height: var(--calico-sprite-size);");
    expect(html).toContain(".calico-sprite[hidden]");
    expect(html).toContain("calc(-50% + var(--calico-offset-x))");
    expect(html).not.toContain("btn.addEventListener('pointerdown'");
    expect(html).not.toContain("btn.addEventListener('pointerup'");
    expect(html).not.toContain("btn.addEventListener('pointermove'");
    expect(interactionHtml).toContain("--calico-hit-area-size: 132px");
    expect(interactionHtml).toContain("btn.addEventListener('pointerdown'");
    expect(interactionHtml).toContain("btn.addEventListener('pointerup'");
    expect(interactionHtml).toContain("btn.addEventListener('pointermove'");
    expect(interactionHtml).toContain("btn.addEventListener('pointerenter'");
    expect(interactionHtml).toContain("setPointerCapture");
    expect(interactionHtml).toContain("releasePointerCapture");
    expect(interactionHtml).toContain("calico-interaction");
  });

  it("starts and pauses the Calico idle director from overlay events", () => {
    const html = readFileSync("public/overlay.html", "utf8");

    expect(html).toContain("let calicoIdleDirector = null;");
    expect(html).toContain("calicoIdleDirector = createCalicoIdleDirector");
    expect(html).toContain("calicoIdleDirector.start();");
    expect(html).toContain("calicoIdleDirector?.pause");
    expect(html).toContain("calicoIdleDirector?.resetIdleClock");
    expect(html).toContain("calicoIdleDirector.resetToBaseline();");
    expect(html).toContain("applyCalicoMotion(event.payload)");
  });

  it("uses native renderer readiness instead of health polling", () => {
    const html = readOverlayHtml();

    expect(html).toContain("set_prompt_button_renderer_ready");
    expect(html).toContain("readinessTransitionApplied");
    expect(html).toContain("outcome.applied === true");
    expect(html).toContain("rendererInstanceId");
    expect(html).toContain("prompt-button-renderer-resume-requested");
    expect(html).not.toContain("startCalicoSpriteHealthWatchdog");
    expect(html).not.toContain("startOverlayHealthHeartbeat");
    expect(html).not.toContain("prompt-button-health");
    expect(html).not.toContain("sprite.naturalWidth");
  });

  it("keeps the renderer readiness command on a throwing invoke path", () => {
    const html = readOverlayHtml();

    expect(html).toContain("async function invokeOrThrow(command, args)");
    expect(html).toContain("return invokeOrThrow('set_prompt_button_renderer_ready'");
  });

  it("suspends reversibly and recovers the same canvas lifecycle", () => {
    const html = readOverlayHtml();

    expect(html).toContain("window.addEventListener('pagehide'");
    expect(html).toContain("window.addEventListener('pageshow'");
    expect(html).toContain("calicoCanvas.addEventListener('contextlost'");
    expect(html).toContain("calicoCanvas.addEventListener('contextrestored'");
    expect(html).toContain("window.addEventListener('visibilitychange'");
    expect(html).toContain("calicoMotion?.suspend({ retainFrame: true })");
    expect(html).toContain("calicoMotion?.dispose()");
    expect(html).toContain("prepareBackingStoreResize");
    expect(html).toContain("commitPreparedResize");
    expect(html).toContain("queuedDpr = nextDpr");
  });

  it("runs one bounded in-place recovery after a fatal renderer error", () => {
    const html = readOverlayHtml();
    const recoveryBlock = html.slice(
      html.indexOf("async function recoverFatalRenderer"),
      html.indexOf("async function resizeCalicoBackingStore")
    );

    expect(html).toContain("let fatalRecoveryPromise = null;");
    expect(recoveryBlock).toContain("for (const delay of [0, 100, 400])");
    expect(recoveryBlock).toContain("const hidden = await reportRendererReady(false)");
    expect(recoveryBlock).toContain("readinessTransitionApplied(hidden)");
    expect(recoveryBlock).toContain("await resumeAndReportReady()");
    expect(recoveryBlock).toContain("Failed to recover Calico renderer in place");
    expect(recoveryBlock).toContain("if (fatalRecoveryPromise) return fatalRecoveryPromise;");
    expect(recoveryBlock).toContain("calicoMotion?.suspend({ retainFrame: true });");
    expect(recoveryBlock).not.toContain("createCalicoFrameRenderer");
    expect(recoveryBlock).not.toContain("show_prompt_button");
  });

  it("forces a real redraw and acknowledges nonce probes without changing readiness", () => {
    const html = readOverlayHtml();
    const probeBlock = html.slice(
      html.indexOf("async function acknowledgeVisualProbe"),
      html.indexOf("async function recoverFatalRenderer")
    );

    expect(html).toContain("prompt-button-visual-probe");
    expect(probeBlock).toContain("calicoRenderer.redrawCurrentFrame()");
    expect(probeBlock).toContain("visibleCalicoAlphaPixels()");
    expect(probeBlock).toContain("ack_prompt_button_visual_probe");
    expect(probeBlock).not.toContain("reportRendererReady");
    expect(probeBlock).not.toContain("calicoIdleDirector");
    expect(html).not.toContain("const resumed = state === 'ready' ||");
  });

  it("keeps click-to-open separate from hover attention", () => {
    const html = readOverlayInteractionHtml();
    const clickBlock = html.slice(
      html.indexOf("const permission = await invoke('prompt_interaction_permission_status');"),
      html.indexOf("btn.addEventListener('pointerenter'")
    );

    expect(clickBlock).not.toContain("handleAttention");
    expect(clickBlock).not.toContain("hover-attention");
    expect(clickBlock).not.toContain("applyCalicoMotion");
  });

  it("hides the prompt popover when Calico dragging starts", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("hidePromptPopoverForDrag");
    expect(html).toContain("await invoke('hide_prompt_popover')");
    expect(html.indexOf("hidePromptPopoverForDrag().catch")).toBeLessThan(
      html.indexOf("emit('prompt-button-drag-started')")
    );
  });

  it("delegates session-safe capture and opening to one backend command", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("let promptPickSessionId = Math.floor(performance.timeOrigin) * 1000;");
    expect(html).toContain("const sessionId = ++promptPickSessionId;");
    expect(html).toContain("const permission = await invoke('prompt_interaction_permission_status');");
    expect(html).toContain("if (permission?.required && !permission.trusted)");
    expect(html).toContain("await handleMissingPromptInteractionPermission(permission);");
    expect(html).toContain("const toggleResult = await invoke('toggle_prompt_popover_from_button', { sessionId });");
    expect(html).not.toContain("invoke('begin_prompt_pick_session'");
    expect(html).not.toContain("sessionPromise");
    expect(html).toContain("await notifyVisual('reset');");
    expect(html.indexOf("prompt_interaction_permission_status")).toBeLessThan(
      html.indexOf("toggle_prompt_popover_from_button")
    );
  });

  it("requires deliberate pointer movement before treating a click as drag", () => {
    const html = readOverlayInteractionHtml();
    const visualHtml = readOverlayHtml();

    expect(html).toContain("const DRAG_START_DISTANCE_PX = 10;");
    expect(html).toContain("distance(start, current) < DRAG_START_DISTANCE_PX");
    expect(visualHtml).toContain("applyCalicoMotion({ state: 'react-drag'");
    expect(html).not.toContain("distance(start, current) < 4");
  });

  it("listens for prompt autosend status and renders a Calico status bubble", () => {
    const html = readOverlayHtml();

    expect(html).toContain("prompt-autosend-status");
    expect(html).toContain("calico-status-bubble");
    expect(html).toContain("showStatusBubble");
    expect(html).toContain("hideStatusBubble");
    expect(html).toContain("statusBubble.textContent");
  });

  it("does not use manual paste as the default failure copy", () => {
    const html = readOverlayHtml();

    expect(html).not.toContain("可手动 Cmd+V");
  });

  it("checks Accessibility permission from the Calico click before opening prompts", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("prompt_interaction_permission_status");
    expect(html).toContain("request_prompt_interaction_permission");
    expect(html).toContain("open_accessibility_settings");
    expect(html).toContain("permissionMessages");
    expect(html).toContain("nativePromptFallback");
    expect(html).toContain("lastAccessibilitySettingsOpenAt");
    expect(html).toContain("invokeOrThrow('open_accessibility_settings')");
    expect(html).not.toContain("request_accessibility_permission_cmd");
    expect(html).not.toContain("statusBubble.dataset.action");
    expect(html).not.toContain("is-action");
    expect(html).not.toContain("Open Accessibility Permission Help");
    expect(html).not.toContain("payload.kind || 'copied'");
  });

  it("only debounces Accessibility settings after the settings open command succeeds", () => {
    const html = readOverlayInteractionHtml();
    const settingsBlock = html.slice(
      html.indexOf("const now = Date.now();"),
      html.indexOf("} catch (error)", html.indexOf("const now = Date.now();"))
    );

    expect(settingsBlock.indexOf("await invokeOrThrow('open_accessibility_settings');"))
      .toBeLessThan(settingsBlock.indexOf("lastAccessibilitySettingsOpenAt = now;"));
  });

  it("opens prompts without switching Calico into a throw-ready pose", () => {
    const html = readOverlayInteractionHtml();

    expect(html).toContain("toggle_prompt_popover_from_button");
    expect(html).not.toContain("setMotionState('ready'");
    expect(html).not.toContain('[data-motion-state="ready"]');
    expect(html).not.toContain("setSpriteSource(sprites.ready)");
    expect(html).not.toContain("calico-ready-windup");
    expect(html).not.toContain("calico-ready-projectile");
  });

  it("does not listen for paper-plane throw events", () => {
    const html = readOverlayHtml();

    expect(html).not.toContain("prompt-throw-send");
    expect(html).not.toContain("playCalicoThrow");
    expect(html).not.toContain("show_paper_plane_flight_from_button");
    expect(html).not.toContain("setMotionState('throwing'");
    expect(html).not.toContain("setMotionState('recovering'");
    expect(html).not.toContain("calico-throw-snap");
    expect(html).not.toContain("calico-throw-projectile-release");
    expect(html).not.toContain("THROW_RELEASE_MS");
  });

  it("requests a baseline return when the popover is dismissed", () => {
    const html = readOverlayHtml();

    expect(html).toContain("prompt-popover-dismissed");
    expect(html).toContain("requestReset");
  });

  it("does not ship the removed paper-plane flight window", () => {
    expect(existsSync("public/paper-flight.html")).toBe(false);
    expect(existsSync("public/calico/paper-plane.svg")).toBe(false);
    expect(existsSync("src-tauri/capabilities/paper-flight.json")).toBe(false);
  });
});
