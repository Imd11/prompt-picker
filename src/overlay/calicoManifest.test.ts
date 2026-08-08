import { existsSync, readFileSync, readdirSync } from "fs";
import { describe, expect, it, vi } from "vitest";
import {
  decodeRgbaPng,
  measureMotionVisual,
  type SheetGeometry,
} from "./calicoVisualMetrics";

type CalicoState = {
  file?: string;
  priority: number;
  durationMs: number;
  minMs: number;
  replay: boolean;
  completeBeforeTransition?: boolean;
  scale: number;
  offsetX: number;
  offsetY: number;
};

type SheetManifest = { states: Record<string, SheetGeometry> };

type CalicoManifest = {
  schemaVersion: number;
  assetSource: string;
  defaultState: string;
  phase1States: string[];
  reservedStates: string[];
  states: Record<string, CalicoState>;
};

type IdleDirectorModule = {
  IDLE_MOTION_POOL: Array<{ state: string; weights: Record<string, number> }>;
};

type MotionRuntimeModule = {
  createCalicoMotionRuntime(options: Record<string, unknown>): {
    apply(payload: Record<string, unknown>): boolean;
    dispose(): void;
  };
};

const phase1States = [
  "idle-follow",
  "idle",
  "thinking",
  "working-typing",
  "working-conducting",
  "working-juggling",
  "working-building",
  "working-carrying",
  "working-sweeping",
  "notification",
  "error",
  "happy",
  "react-drag",
];

const reservedStates = [
  "yawning",
  "dozing",
  "collapsing",
  "sleeping",
  "waking",
  "react-poke",
  "react-left",
  "mini-enter",
  "mini-idle",
  "mini-peek",
  "mini-alert",
  "mini-happy",
  "mini-crabwalk",
  "mini-sleep",
];

const calicoNativeWindowSize = 288;
const calicoHitAreaSize = 132;
const calicoSpriteSize = 126;

function readManifest(): CalicoManifest {
  return JSON.parse(readFileSync("public/calico/manifest.json", "utf8"));
}

function readSheetManifest(): SheetManifest {
  return JSON.parse(readFileSync("public/calico/sheets/manifest.json", "utf8"));
}

async function loadIdleDirector() {
  // @ts-expect-error public overlay module is intentionally outside the src build graph.
  return (await import("../../public/calico/idle-director.js")) as IdleDirectorModule;
}

async function loadMotionRuntime() {
  // @ts-expect-error public overlay module is intentionally outside the src build graph.
  return (await import("../../public/calico/motion-runtime.js")) as MotionRuntimeModule;
}

describe("Calico manifest", () => {
  it("declares the Phase 1 motion states separately from later risky states", () => {
    const manifest = readManifest();

    expect(manifest.schemaVersion).toBe(1);
    expect(manifest.assetSource).toBe("authorized upstream");
    expect(manifest.defaultState).toBe("idle");
    expect(manifest.phase1States).toEqual(phase1States);
    expect(manifest.reservedStates).toEqual(reservedStates);
    expect(Object.keys(manifest.states).sort()).toEqual(
      [...phase1States, ...reservedStates].sort()
    );
  });

  it("ships every declared Calico state with rendering metadata", () => {
    const manifest = readManifest();
    const sheetManifest = readSheetManifest();

    for (const [stateName, state] of Object.entries(manifest.states)) {
      if (stateName === "idle-follow") {
        expect(state.file, stateName).toBe("/calico/calico-idle-follow.svg");
        expect(existsSync(`public${state.file}`), stateName).toBe(true);
      } else {
        expect(state.file, stateName).toBeUndefined();
        expect(sheetManifest.states[stateName], stateName).toBeDefined();
        expect(existsSync(`public${sheetManifest.states[stateName].file}`), stateName).toBe(true);
      }
      expect(typeof state.priority, stateName).toBe("number");
      expect(typeof state.durationMs, stateName).toBe("number");
      expect(typeof state.minMs, stateName).toBe("number");
      expect(typeof state.replay, stateName).toBe("boolean");
      expect(typeof state.scale, stateName).toBe("number");
      expect(typeof state.offsetX, stateName).toBe("number");
      expect(typeof state.offsetY, stateName).toBe("number");
    }
  });

  it("protects the waking transition until its full sequence completes", () => {
    const manifest = readManifest();

    expect(manifest.states.waking.completeBeforeTransition).toBe(true);
    for (const [stateName, state] of Object.entries(manifest.states)) {
      if (stateName !== "waking") {
        expect(state.completeBeforeTransition, stateName).not.toBe(true);
      }
    }
  });

  it("does not reintroduce paper-plane assets", () => {
    const manifest = readManifest();
    const files = Object.values(manifest.states).flatMap((state) => state.file ?? []);

    expect(files).not.toContain("/calico/paper-plane.svg");
    expect(existsSync("public/calico/paper-plane.svg")).toBe(false);
  });

  it("ships generated sheets for every idle director and hover response state", async () => {
    const manifest = readManifest();
    const sheetManifest = readSheetManifest();
    const { IDLE_MOTION_POOL } = await loadIdleDirector();

    for (const { state } of IDLE_MOTION_POOL) {
      expect(manifest.states[state], state).toBeDefined();
      expect(sheetManifest.states[state], state).toBeDefined();
      expect(existsSync(`public${sheetManifest.states[state].file}`), state).toBe(true);
    }
    expect(manifest.states.waking).toBeDefined();
    expect(existsSync(`public${sheetManifest.states.waking.file}`)).toBe(true);
  });

  it("does not ship APNG runtime assets", () => {
    const publicApng = readdirSync("public/calico").filter((file) => file.endsWith(".apng"));
    expect(publicApng).toEqual([]);
  });

  it("routes every generated Calico state to its sheet and preserves the SVG baseline", async () => {
    const manifest = readManifest();
    const sheetManifest = readSheetManifest();
    const { createCalicoMotionRuntime } = await loadMotionRuntime();
    const renderer = {
      play: vi.fn().mockResolvedValue(true),
      showBaseline: vi.fn().mockResolvedValue(true),
      setPresentation: vi.fn(),
      suspend: vi.fn(),
      resume: vi.fn(),
      dispose: vi.fn(),
    };
    const runtime = createCalicoMotionRuntime({
      renderer,
      host: document.createElement("button"),
      manifest,
      sheetManifest,
    });

    for (const state of Object.keys(manifest.states)) {
      expect(runtime.apply({ state, force: true }), state).toBe(true);
    }

    expect(new Set(renderer.play.mock.calls.map((call) => call[0]))).toEqual(
      new Set(Object.keys(manifest.states).filter((state) => state !== "idle-follow"))
    );
    expect(renderer.showBaseline).toHaveBeenCalledTimes(1);
    runtime.dispose();
  });

  it("keeps deep idle states on non-replay assets", async () => {
    const manifest = readManifest();
    const { IDLE_MOTION_POOL } = await loadIdleDirector();
    const deepIdleStates = IDLE_MOTION_POOL.filter((entry) => (entry.weights.deepIdle ?? 0) > 0)
      .map((entry) => entry.state);

    expect(deepIdleStates.length).toBeGreaterThan(0);
    for (const stateName of deepIdleStates) {
      expect(manifest.states[stateName].replay, stateName).toBe(false);
    }
  });

  it("keeps every rendered Calico motion inside the native transparent window", () => {
    const manifest = readManifest();
    const center = calicoNativeWindowSize / 2;

    for (const [stateName, state] of Object.entries(manifest.states)) {
      const renderedSize = calicoSpriteSize * state.scale;
      const left = center - renderedSize / 2 + state.offsetX;
      const top = center - renderedSize / 2 + state.offsetY;
      const right = left + renderedSize;
      const bottom = top + renderedSize;

      expect(left, `${stateName} left edge`).toBeGreaterThanOrEqual(0);
      expect(top, `${stateName} top edge`).toBeGreaterThanOrEqual(0);
      expect(right, `${stateName} right edge`).toBeLessThanOrEqual(calicoNativeWindowSize);
      expect(bottom, `${stateName} bottom edge`).toBeLessThanOrEqual(calicoNativeWindowSize);
    }

    expect(calicoHitAreaSize).toBe(132);
  });

  it("keeps every full-size motion optically aligned with idle", () => {
    const manifest = readManifest();
    const sheetManifest = readSheetManifest();
    // Mini motions intentionally use a separate scale and lower-screen position.
    const fullSizeStates = [...phase1States, ...reservedStates]
      .filter((stateName) => stateName !== "idle-follow" && !stateName.startsWith("mini-"));
    // These two sheets connect a wide desktop surface to the character. Limit
    // optical measurement to the character region while still testing the
    // complete sprite against the native window in the existing bounds test.
    const bodyRegionBottom: Record<string, number> = {
      "working-typing": 160,
      "working-sweeping": 170,
    };
    const metrics = new Map(fullSizeStates.map((stateName) => {
      const sheet = sheetManifest.states[stateName];
      const png = decodeRgbaPng(`public${sheet.file}`);
      return [stateName, measureMotionVisual(
        png,
        sheet,
        manifest.states[stateName],
        stateName === "react-drag" ? -2 : 0,
        bodyRegionBottom[stateName],
      )];
    }));
    const idleArea = metrics.get("idle")!.medianPrimaryArea;

    for (const stateName of fullSizeStates) {
      const stateMetrics = metrics.get(stateName)!;
      const ratio = stateMetrics.medianPrimaryArea / idleArea;
      expect(ratio, `${stateName} optical area`).toBeGreaterThanOrEqual(0.9);
      expect(ratio, `${stateName} optical area`).toBeLessThanOrEqual(1.1);
      expect(stateMetrics.nativeWindowContained, `${stateName} native bounds`).toBe(true);
      const minimumCoverage = stateName === "working-carrying" ? 0.8 : 0.95;
      expect(stateMetrics.minimumHitCoverage, `${stateName} hit coverage`)
        .toBeGreaterThanOrEqual(minimumCoverage);
    }
  }, 30_000);
});
