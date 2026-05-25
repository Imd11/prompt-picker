import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useInputTargetPolling } from "./useInputTargetPolling";

vi.mock("../platform/platformApi", () => ({
  getFrontmostApp: vi.fn(),
  getCurrentInputTarget: vi.fn(),
  showPromptButton: vi.fn(),
  hidePromptButton: vi.fn(),
  showPromptPopover: vi.fn(),
  hidePromptPopover: vi.fn()
}));

import { getFrontmostApp, getCurrentInputTarget, showPromptButton, hidePromptButton } from "../platform/platformApi";

describe("useInputTargetPolling", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows attached button when target exists and app not blacklisted", async () => {
    vi.mocked(getFrontmostApp).mockResolvedValue({ name: "Codex", bundle_id: "com.codex.app" });
    vi.mocked(getCurrentInputTarget).mockResolvedValue({
      frame: { x: 100, y: 100, width: 300, height: 200 },
      button_position: [148, 268],
      app: { name: "Codex", bundle_id: "com.codex.app" }
    });

    renderHook(() => useInputTargetPolling([]));

    await act(async () => {
      await new Promise((r) => setTimeout(r, 600));
    });

    expect(showPromptButton).toHaveBeenCalledWith(148, 268);
  });

  it("hides button when app is blacklisted", async () => {
    vi.mocked(getFrontmostApp).mockResolvedValue({ name: "Codex", bundle_id: "com.codex.app" });

    renderHook(() => useInputTargetPolling(["com.codex.app"]));

    await act(async () => {
      await new Promise((r) => setTimeout(r, 600));
    });

    expect(hidePromptButton).toHaveBeenCalled();
  });

  it("shows fallback when no target found", async () => {
    vi.mocked(getFrontmostApp).mockResolvedValue({ name: "Finder", bundle_id: "com.apple.finder" });
    vi.mocked(getCurrentInputTarget).mockResolvedValue(null);

    renderHook(() => useInputTargetPolling([]));

    await act(async () => {
      await new Promise((r) => setTimeout(r, 600));
    });

    expect(hidePromptButton).toHaveBeenCalled();
  });
});