import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { SettingsPanel } from "./SettingsPanel";
import type { Settings } from "../shared/settingsStore";

describe("settings panel", () => {
  const mockSettings: Settings = {
    version: 1,
    blacklistedApps: [
      { bundleId: "com.example.app", name: "Example App" }
    ]
  };

  it("renders blacklisted apps", () => {
    render(<SettingsPanel settings={mockSettings} onRemove={() => {}} />);
    expect(screen.getByText("Example App")).toBeTruthy();
  });

  it("remove button calls onRemove with bundle id", () => {
    let removedBundleId: string | null = null;
    render(
      <SettingsPanel
        settings={mockSettings}
        onRemove={(id) => { removedBundleId = id; }}
      />
    );

    const removeBtn = screen.getByRole("button", { name: "Remove" });
    removeBtn.click();
    expect(removedBundleId).toBe("com.example.app");
  });

  it("empty state renders when no blacklisted apps", () => {
    render(<SettingsPanel settings={{ version: 1, blacklistedApps: [] }} onRemove={() => {}} />);
    expect(screen.getByText("No blacklisted apps")).toBeTruthy();
  });
});