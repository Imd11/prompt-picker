import type { Settings } from "../shared/settingsStore";

interface SettingsPanelProps {
  settings: Settings;
  onRemove: (bundleId: string) => void;
}

export function SettingsPanel({ settings, onRemove }: SettingsPanelProps) {
  return (
    <div className="settings-panel">
      <h2>Blacklisted Apps</h2>
      {settings.blacklistedApps.length === 0 ? (
        <p className="empty-state">No blacklisted apps</p>
      ) : (
        <ul className="blacklist">
          {settings.blacklistedApps.map((app) => (
            <li key={app.bundleId}>
              <span>{app.name}</span>
              <button onClick={() => onRemove(app.bundleId)}>Remove</button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}