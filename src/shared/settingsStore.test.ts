import { describe, expect, it } from "vitest";
import { createSettingsStore } from "./settingsStore";

function createTestSettingsStore(initial?: string | null) {
  let state = initial ?? null;
  return createSettingsStore({
    read: async () => state,
    write: async (value) => { state = value; }
  });
}

describe("settings store", () => {
  it("default blacklist is empty", async () => {
    const store = createTestSettingsStore();
    const settings = await store.get();
    expect(settings.blacklistedApps).toEqual([]);
  });

  it("add bundle id to blacklist", async () => {
    const store = createTestSettingsStore();
    await store.addBlacklistedApp("com.example.app", "Example App");
    const settings = await store.get();
    expect(settings.blacklistedApps).toContainEqual({ bundleId: "com.example.app", name: "Example App" });
  });

  it("remove bundle id from blacklist", async () => {
    const store = createTestSettingsStore('{"version":1,"blacklistedApps":[{"bundleId":"com.example.app","name":"Example App"}]}');
    await store.removeBlacklistedApp("com.example.app");
    const settings = await store.get();
    expect(settings.blacklistedApps.find(a => a.bundleId === "com.example.app")).toBeUndefined();
  });

  it("duplicate adds are ignored", async () => {
    const store = createTestSettingsStore();
    await store.addBlacklistedApp("com.example.app", "Example App");
    await store.addBlacklistedApp("com.example.app", "Example App");
    const settings = await store.get();
    const matching = settings.blacklistedApps.filter(a => a.bundleId === "com.example.app");
    expect(matching).toHaveLength(1);
  });
});