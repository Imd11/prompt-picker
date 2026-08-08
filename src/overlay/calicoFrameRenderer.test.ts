import { describe, expect, it, vi } from "vitest";
import {
  calculateContainRect,
  createCalicoFrameRenderer,
  frameGeometry,
  loadHtmlImageSurface,
  loadImageBitmapSurface,
  playbackFrameAt,
} from "../../public/calico/frame-renderer.js";

function fakeContext() {
  return {
    clearRect: vi.fn(),
    drawImage: vi.fn(),
    restore: vi.fn(),
    save: vi.fn(),
    setTransform: vi.fn(),
    globalCompositeOperation: "source-over",
  };
}

function fakeCanvas(context = fakeContext()) {
  const properties = new Map<string, string>();
  return {
    width: 126,
    height: 126,
    clientWidth: 126,
    clientHeight: 126,
    context,
    getContext: vi.fn(() => context),
    addEventListener: vi.fn(),
    style: {
      width: "126px",
      height: "126px",
      setProperty: vi.fn((name: string, value: string) => properties.set(name, value)),
      getPropertyValue: (name: string) => properties.get(name) ?? "",
    },
  };
}

function sheet(file: string, overrides = {}) {
  return {
    file,
    pixelFormat: "rgba",
    frameWidth: 266,
    frameHeight: 200,
    frameCount: 2,
    columns: 2,
    rows: 1,
    gutter: 2,
    strideX: 268,
    strideY: 202,
    sheetWidth: 534,
    sheetHeight: 200,
    frameDurationsMs: [100, 200],
    plays: 0,
    ...overrides,
  };
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("Calico frame surface ownership", () => {
  it("closes an ImageBitmap surface exactly once", async () => {
    const close = vi.fn();
    const surface = await loadImageBitmapSurface("/sheet.png", {
      fetchFn: vi.fn(async () => ({ ok: true, blob: async () => ({}) })),
      createImageBitmapFn: vi.fn(async () => ({ close })),
    });

    surface.release();
    surface.release();

    expect(close).toHaveBeenCalledTimes(1);
  });

  it("revokes an HTML image Object URL exactly once", async () => {
    const image = { src: "", decode: vi.fn().mockResolvedValue(undefined) };
    const urlApi = { createObjectURL: vi.fn(() => "blob:sheet"), revokeObjectURL: vi.fn() };
    const surface = await loadHtmlImageSurface("/sheet.png", {
      fetchFn: vi.fn(async () => ({ ok: true, blob: async () => ({}) })),
      imageFactory: () => image,
      urlApi,
    });

    surface.release();
    surface.release();

    expect(image.src).toBe("");
    expect(urlApi.revokeObjectURL).toHaveBeenCalledTimes(1);
  });

  it("releases a partially allocated Object URL when decode fails", async () => {
    const image = { src: "", decode: vi.fn().mockRejectedValue(new Error("decode")) };
    const urlApi = { createObjectURL: vi.fn(() => "blob:sheet"), revokeObjectURL: vi.fn() };

    await expect(loadHtmlImageSurface("/sheet.png", {
      fetchFn: vi.fn(async () => ({ ok: true, blob: async () => ({}) })),
      imageFactory: () => image,
      urlApi,
    })).rejects.toThrow("decode");
    expect(urlApi.revokeObjectURL).toHaveBeenCalledWith("blob:sheet");
  });
});

describe("Calico frame geometry and playback", () => {
  it("contain-fits wide and square sources without stretching", () => {
    expect(calculateContainRect(266, 200, 126, 126)).toEqual({
      x: 0,
      y: expect.closeTo(15.631578947, 6),
      width: 126,
      height: expect.closeTo(94.736842105, 6),
    });
    expect(calculateContainRect(355, 200, 126, 126).width).toBeCloseTo(126);
  });

  it("samples cells by generated stride without including the gutter", () => {
    const geometry = frameGeometry(sheet("/wide.png"), 1, 126, 126);
    expect(geometry.sourceX).toBe(268);
    expect(geometry.sourceY).toBe(0);
    expect(geometry.sourceWidth).toBe(266);
  });

  it("uses modulo only for infinite playback and holds finite final frames", () => {
    expect(playbackFrameAt(sheet("/loop.png"), 350)).toMatchObject({
      frameIndex: 0,
      done: false,
    });
    expect(playbackFrameAt(sheet("/once.png", { plays: 1 }), 300)).toEqual({
      frameIndex: 1,
      done: true,
      nextDelayMs: null,
    });
    expect(playbackFrameAt(sheet("/twice.png", { plays: 2 }), 601).done).toBe(true);
  });

  it("skips expired frames and schedules only the next future boundary", () => {
    expect(playbackFrameAt(sheet("/late.png"), 580)).toEqual({
      frameIndex: 1,
      done: false,
      nextDelayMs: 20,
    });
  });
});

describe("bounded Calico frame renderer", () => {
  it("shares the two-surface budget between baseline and motion sheets", async () => {
    let liveSurfaces = 0;
    let maximumLiveSurfaces = 0;
    const releases = new Map<string, ReturnType<typeof vi.fn<() => void>>>();
    const allocateSurface = (file: string, baseline = false) => {
      liveSurfaces += 1;
      maximumLiveSurfaces = Math.max(maximumLiveSurfaces, liveSurfaces);
      let released = false;
      const release = vi.fn(() => {
        if (released) return;
        released = true;
        liveSurfaces -= 1;
      });
      releases.set(file, release);
      return {
        source: baseline ? { naturalWidth: 266, naturalHeight: 200 } : {},
        backend: "fake",
        release,
      };
    };
    const renderer = createCalicoFrameRenderer({
      canvas: fakeCanvas(),
      createCanvas: () => fakeCanvas(),
      loadBaseline: vi.fn(async () => allocateSurface("baseline", true)),
      loadSurface: vi.fn(async (file: string) => allocateSurface(file)),
      drawFrame: vi.fn(),
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
      maxDecodedSheets: 2,
    });

    expect(await renderer.showBaseline()).toBe(true);
    expect(await renderer.play("a", sheet("/a.png"), { restart: true })).toBe(true);
    expect(releases.get("baseline")).toHaveBeenCalledTimes(1);
    expect(await renderer.play("b", sheet("/b.png"), { restart: true })).toBe(true);
    expect(renderer.diagnostics().liveSurfaceCount).toBeLessThanOrEqual(2);

    expect(await renderer.showBaseline()).toBe(true);
    expect(renderer.diagnostics().liveSurfaceCount).toBeLessThanOrEqual(2);
    expect(await renderer.play("c", sheet("/c.png"), { restart: true })).toBe(true);
    expect(renderer.diagnostics().liveSurfaceCount).toBeLessThanOrEqual(2);
    expect(maximumLiveSurfaces).toBeLessThanOrEqual(2);

    renderer.dispose();
    expect(liveSurfaces).toBe(0);
    releases.forEach((release) => expect(release).toHaveBeenCalledTimes(1));
  });

  it("keeps one decode in flight and only the latest of 2,000 queued requests", async () => {
    const interruptionSequence = [
      "idle",
      "react-left",
      "react-drag",
      "idle-follow",
      "working-typing",
      "error",
      "idle-follow",
    ];
    const requests: Array<{
      file: string;
      resolve: (surface: { source: object; backend: string; release: () => void }) => void;
      release: ReturnType<typeof vi.fn<() => void>>;
    }> = [];
    const loadSurface = vi.fn((file: string) => new Promise((resolve) => {
      const release = vi.fn<() => void>();
      requests.push({ file, release, resolve: resolve as typeof requests[number]["resolve"] });
    }));
    const renderer = createCalicoFrameRenderer({
      canvas: fakeCanvas(),
      createCanvas: () => fakeCanvas(),
      loadSurface,
      drawFrame: vi.fn(),
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
    });

    for (let index = 0; index < 2_000; index += 1) {
      const state = interruptionSequence[index % interruptionSequence.length];
      void renderer.play(state, sheet(`/sheet-${index % 3}.png`), { restart: true });
      expect(renderer.diagnostics().pendingDecodeCount).toBeLessThanOrEqual(1);
      expect(renderer.diagnostics().queuedRequestCount).toBeLessThanOrEqual(1);
      expect(renderer.diagnostics().decodedSheetCount).toBeLessThanOrEqual(2);
      expect(renderer.diagnostics().liveSurfaceCount).toBeLessThanOrEqual(2);
    }
    expect(requests).toHaveLength(1);
    requests[0].resolve({
      source: {},
      backend: "fake",
      release: () => requests[0].release(),
    });
    await settle();
    expect(requests[0].release).toHaveBeenCalledTimes(1);
    expect(requests).toHaveLength(2);
    requests[1].resolve({
      source: {},
      backend: "fake",
      release: () => requests[1].release(),
    });
    await settle();

    expect(renderer.diagnostics()).toMatchObject({
      decodedSheetCount: 1,
      liveSurfaceCount: 1,
      pendingDecodeCount: 0,
      queuedRequestCount: 0,
      state: "ready",
      visualReady: true,
    });
    renderer.dispose();
    expect(requests[1].release).toHaveBeenCalledTimes(1);
    expect(renderer.diagnostics()).toMatchObject({
      decodedSheetCount: 0,
      liveSurfaceCount: 0,
      pendingDecodeCount: 0,
      queuedRequestCount: 0,
      activeTimerCount: 0,
    });
  });

  it("owns presentation transforms on the existing canvas", () => {
    const canvas = fakeCanvas();
    const renderer = createCalicoFrameRenderer({ canvas, createCanvas: () => fakeCanvas() });

    renderer.setPresentation({ scale: 1.2, offsetX: -3, offsetY: 6 });

    expect(canvas.style.getPropertyValue("--calico-scale")).toBe("1.2");
    expect(canvas.style.getPropertyValue("--calico-offset-x")).toBe("-3px");
    expect(canvas.style.getPropertyValue("--calico-offset-y")).toBe("6px");
  });

  it("can apply presentation transforms to a stable host around the canvas", () => {
    const canvas = fakeCanvas();
    const presentationElement = document.createElement("div");
    const renderer = createCalicoFrameRenderer({
      canvas,
      presentationElement,
      createCanvas: () => fakeCanvas(),
    });

    renderer.setPresentation({ scale: 1.2, offsetX: -3, offsetY: 6 });

    expect(presentationElement.style.getPropertyValue("--calico-scale")).toBe("1.2");
    expect(presentationElement.style.getPropertyValue("--calico-offset-x")).toBe("-3px");
    expect(presentationElement.style.getPropertyValue("--calico-offset-y")).toBe("6px");
    expect(canvas.style.getPropertyValue("--calico-scale")).toBe("");
  });

  it("retains the previous visible frame when a source-to-scratch draw fails", async () => {
    const visible = fakeCanvas();
    const drawFrame = vi.fn()
      .mockImplementationOnce(() => undefined)
      .mockImplementationOnce(() => undefined)
      .mockImplementation(() => { throw new Error("scratch draw"); });
    const release = vi.fn();
    const renderer = createCalicoFrameRenderer({
      canvas: visible,
      createCanvas: () => fakeCanvas(),
      loadSurface: vi.fn(async () => ({ source: {}, backend: "fake", release })),
      drawFrame,
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
      onError: vi.fn(),
    });

    await renderer.play("first", sheet("/first.png"), { restart: true });
    const commitsBeforeFailure = visible.context.drawImage.mock.calls.length;
    await renderer.play("second", sheet("/second.png"), { restart: true });

    expect(visible.context.drawImage).toHaveBeenCalledTimes(commitsBeforeFailure);
    expect(renderer.diagnostics().visualReady).toBe(true);
  });

  it("enters lost once when scratch-to-visible commit fails", async () => {
    const visibleContext = fakeContext();
    visibleContext.drawImage.mockImplementation(() => { throw new Error("commit"); });
    const fatal = vi.fn();
    const renderer = createCalicoFrameRenderer({
      canvas: fakeCanvas(visibleContext),
      createCanvas: () => fakeCanvas(),
      loadSurface: vi.fn(async () => ({ source: {}, backend: "fake", release: vi.fn() })),
      drawFrame: vi.fn(),
      onError: vi.fn(),
      onFatalRender: fatal,
    });

    await renderer.play("first", sheet("/first.png"), { restart: true });
    await renderer.play("second", sheet("/second.png"), { restart: true });

    expect(renderer.diagnostics()).toMatchObject({
      state: "lost",
      visualReady: false,
      activeTimerCount: 0,
    });
    expect(fatal).toHaveBeenCalledTimes(1);
  });

  it("reports a later fatal commit after a successful in-place recovery", async () => {
    const visibleContext = fakeContext();
    let commitShouldFail = false;
    visibleContext.drawImage.mockImplementation(() => {
      if (commitShouldFail) throw new Error("commit");
    });
    const fatal = vi.fn();
    const renderer = createCalicoFrameRenderer({
      canvas: fakeCanvas(visibleContext),
      createCanvas: () => fakeCanvas(),
      loadSurface: vi.fn(async () => ({ source: {}, backend: "fake", release: vi.fn() })),
      drawFrame: vi.fn(),
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
      onError: vi.fn(),
      onFatalRender: fatal,
    });
    await renderer.play("first", sheet("/first.png"), { restart: true });

    commitShouldFail = true;
    expect(renderer.redrawCurrentFrame()).toBe(false);
    expect(renderer.redrawCurrentFrame()).toBe(false);
    expect(fatal).toHaveBeenCalledTimes(1);

    renderer.suspend({ retainFrame: true });
    commitShouldFail = false;
    expect(renderer.resume()).toBe(true);
    expect(renderer.diagnostics()).toMatchObject({ state: "ready", visualReady: true });

    commitShouldFail = true;
    expect(renderer.redrawCurrentFrame()).toBe(false);
    expect(fatal).toHaveBeenCalledTimes(2);
  });

  it("prepares DPR changes without touching visible backing size and rejects stale tokens", async () => {
    const visible = fakeCanvas();
    const renderer = createCalicoFrameRenderer({
      canvas: visible,
      createCanvas: () => fakeCanvas(),
      loadSurface: vi.fn(async () => ({ source: {}, backend: "fake", release: vi.fn() })),
      drawFrame: vi.fn(),
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
    });
    await renderer.play("first", sheet("/first.png"), { restart: true });

    const token = renderer.prepareBackingStoreResize(2);
    expect(visible.width).toBe(126);
    expect(visible.height).toBe(126);
    expect(renderer.commitPreparedResize(token)).toBe(true);
    expect(visible.width).toBe(252);
    expect(visible.height).toBe(252);

    const stale = renderer.prepareBackingStoreResize(1);
    await renderer.play("first", sheet("/first.png"), { restart: true });
    expect(renderer.commitPreparedResize(stale)).toBe(false);
  });

  it("suspends without clearing the retained frame and releases all ownership on dispose", async () => {
    const visible = fakeCanvas();
    const release = vi.fn();
    const renderer = createCalicoFrameRenderer({
      canvas: visible,
      createCanvas: () => fakeCanvas(),
      loadSurface: vi.fn(async () => ({ source: {}, backend: "fake", release })),
      drawFrame: vi.fn(),
      setTimer: vi.fn(() => 1),
      clearTimer: vi.fn(),
      now: () => 0,
    });
    await renderer.play("first", sheet("/first.png"), { restart: true });
    const visibleClears = visible.context.clearRect.mock.calls.length;

    renderer.suspend({ retainFrame: true });
    expect(visible.context.clearRect).toHaveBeenCalledTimes(visibleClears);
    expect(renderer.diagnostics().state).toBe("suspended");
    expect(renderer.resume()).toBe(true);
    renderer.dispose();

    expect(release).toHaveBeenCalledTimes(1);
    expect(renderer.diagnostics()).toMatchObject({
      state: "disposed",
      decodedSheetCount: 0,
      pendingDecodeCount: 0,
      queuedRequestCount: 0,
      activeTimerCount: 0,
      visualReady: false,
    });
  });
});
