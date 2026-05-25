export type Settings = {
  version: 1;
  blacklistedApps: Array<{ bundleId: string; name: string }>;
};

interface SettingsAdapter {
  read(): Promise<string | null>;
  write(value: string): Promise<void>;
}

export function createSettingsStore(adapter: SettingsAdapter) {
  async function load(): Promise<Settings> {
    const data = await adapter.read();
    if (!data) return { version: 1, blacklistedApps: [] };
    try {
      return JSON.parse(data) as Settings;
    } catch {
      return { version: 1, blacklistedApps: [] };
    }
  }

  async function save(settings: Settings): Promise<void> {
    await adapter.write(JSON.stringify(settings, null, 2));
  }

  return {
    async get(): Promise<Settings> {
      return load();
    },

    async addBlacklistedApp(bundleId: string, name: string): Promise<void> {
      const settings = await load();
      if (settings.blacklistedApps.some(a => a.bundleId === bundleId)) {
        return; // duplicate
      }
      settings.blacklistedApps.push({ bundleId, name });
      await save(settings);
    },

    async removeBlacklistedApp(bundleId: string): Promise<void> {
      const settings = await load();
      settings.blacklistedApps = settings.blacklistedApps.filter(a => a.bundleId !== bundleId);
      await save(settings);
    },

    async isAppBlacklisted(bundleId: string): Promise<boolean> {
      const settings = await load();
      return settings.blacklistedApps.some(a => a.bundleId === bundleId);
    }
  };
}