import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, act, fireEvent } from "@testing-library/react";
import { App } from "../App";
import type { PromptCategory, PromptContainer, PromptItem } from "../shared/promptTypes";

const inputTargetPollingMock = vi.hoisted(() => vi.fn());
const emitMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const listenMock = vi.hoisted(() => vi.fn());
const eventHandlers = vi.hoisted(
  () => new Map<string, (event: { payload: unknown }) => unknown>()
);

vi.mock("../overlay/useInputTargetPolling", () => ({
  useInputTargetPolling: inputTargetPollingMock,
}));

// Mock Tauri core invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: emitMock,
  listen: listenMock,
}));

// Mock getCurrentWindow
let currentWindowLabel = "prompt-popover";
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: currentWindowLabel }),
}));

// Mock fs plugin
vi.mock("@tauri-apps/plugin-fs", () => ({
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
  BaseDirectory: { AppData: "AppData" },
}));

vi.mock("../storage/tauriSettingsStorage", () => ({
  createTauriSettingsStorage: () => ({
    read: async () => {
      const { readTextFile, BaseDirectory } = await import("@tauri-apps/plugin-fs");
      try {
        return await readTextFile("settings.json", { baseDir: BaseDirectory.AppData });
      } catch {
        return null;
      }
    },
    write: async (value: string) => {
      const { writeTextFile, BaseDirectory } = await import("@tauri-apps/plugin-fs");
      await writeTextFile("settings.json", value, { baseDir: BaseDirectory.AppData });
    },
  }),
}));

// Mock dialog plugin
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

const mockPrompts: PromptItem[] = [
  {
    id: "1",
    title: "Test Prompt",
    body: "Test body",
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
  },
];

const devCategory: PromptCategory = {
  id: "cat-dev",
  name: "开发代码",
  order: 0,
  createdAt: "2026-05-26T00:00:00.000Z",
  updatedAt: "2026-05-26T00:00:00.000Z",
};

const writingCategory: PromptCategory = {
  id: "cat-writing",
  name: "写作",
  order: 1,
  createdAt: "2026-05-26T00:00:00.000Z",
  updatedAt: "2026-05-26T00:00:00.000Z",
};

function makeContainer(overrides: Partial<PromptContainer>): PromptContainer {
  return {
    id: "container",
    categoryId: "cat-dev",
    title: "Prompt",
    type: "single",
    sendBehavior: "inherit",
    prompts: [{ id: "entry", body: "body", order: 0 }],
    intervalMs: 700,
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
    ...overrides,
  };
}

describe("app", () => {
  beforeEach(async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/");
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    vi.mocked(readTextFile).mockReset();
    vi.mocked(writeTextFile).mockReset();
    inputTargetPollingMock.mockClear();
    emitMock.mockReset();
    emitMock.mockResolvedValue(undefined);
    eventHandlers.clear();
    listenMock.mockReset();
    listenMock.mockImplementation(
      async (event: string, handler: (event: { payload: unknown }) => unknown) => {
        eventHandlers.set(event, handler);
        return () => eventHandlers.delete(event);
      }
    );
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  async function renderPromptPopover() {
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) {
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        }
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            language: "zh-CN",
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null, buttonPosition: null },
            floatingButton: { visible: true },
            promptInsertion: { mode: "paste_and_submit" },
          });
        }
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });
    await screen.findByText("Test Prompt");
  }

  function revealPromptPopoverTooltip() {
    vi.useFakeTimers();
    fireEvent.pointerMove(screen.getByRole("option", { name: /Test Prompt/i }));
    act(() => {
      vi.advanceTimersByTime(1500);
    });
    expect(screen.getByRole("tooltip")).toBeTruthy();
  }

  function calicoMotionStates() {
    return emitMock.mock.calls
      .filter(([event]) => event === "calico-motion")
      .map(([, payload]) => (payload as { state: string }).state);
  }

  function expectCalicoMotion(state: string) {
    expect(emitMock).toHaveBeenCalledWith(
      "calico-motion",
      expect.objectContaining({ state })
    );
  }

  async function renderMainPromptManager(initialContainers: PromptContainer[] = []) {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/");
    const files = new Map<string, string>([
      [
        "prompts.json",
        JSON.stringify({ version: 2, containers: initialContainers }),
      ],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
        }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        const value = files.get(path);
        if (!value) throw new Error("missing file: " + path);
        return value;
      }
    );
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string, value: string) => {
        files.set(path, value);
      }
    );

    await act(async () => {
      render(<App />);
    });
    await screen.findByRole("heading", { name: "管理提示词" });
    return files;
  }

  async function renderMainPromptManagerWithStore(promptData: unknown) {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/");
    const files = new Map<string, string>([
      ["prompts.json", JSON.stringify(promptData)],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
        }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        const value = files.get(path);
        if (!value) throw new Error("missing file: " + path);
        return value;
      }
    );
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string, value: string) => {
        files.set(path, value);
      }
    );

    await act(async () => {
      render(<App />);
    });
    await screen.findByRole("heading", { name: "管理提示词" });
    return files;
  }

  async function renderLinkedPromptManager({
    localContainers = [],
    externalContent = JSON.stringify({ version: 2, containers: [] }),
    externalSignature = "20:2000",
    linkPath = "/Users/example/Desktop/prompts.json",
    linkSignature = "10:1000",
    readExternal,
    writeExternal,
    getExternalMetadata,
  }: {
    localContainers?: PromptContainer[];
    externalContent?: string;
    externalSignature?: string;
    linkPath?: string;
    linkSignature?: string;
    readExternal?: () => Promise<{ content: string; signature: string }>;
    writeExternal?: (content: string) => Promise<{ signature: string }>;
    getExternalMetadata?: () => Promise<{ signature: string }>;
  } = {}) {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/");
    let currentExternalContent = externalContent;
    let currentExternalSignature = externalSignature;
    let currentMetadataSignature = externalSignature;
    let externalReadCount = 0;
    const files = new Map<string, string>([
      [
        "prompts.json",
        JSON.stringify({ version: 2, containers: localContainers }),
      ],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
          permissions: { accessibilityPromptRequested: false },
          promptLibraryLink: {
            mode: "linked",
            path: linkPath,
            lastKnownSignature: linkSignature,
            lastSyncedAt: "2026-07-06T00:00:00.000Z",
          },
        }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      const value = files.get(path);
      if (!value) throw new Error("missing file: " + path);
      return value;
    });
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string, value: string) => {
        files.set(path, value);
      }
    );
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
      if (command === "read_prompt_library_file") {
        externalReadCount += 1;
        if (readExternal) return readExternal();
        return {
          content: currentExternalContent,
          signature: currentExternalSignature,
        };
      }
      if (command === "prompt_library_file_metadata") {
        if (getExternalMetadata) return getExternalMetadata();
        return { signature: currentMetadataSignature };
      }
      if (command === "write_prompt_library_file") {
        const content = (args as { content?: string } | undefined)?.content ?? "";
        if (writeExternal) return writeExternal(content);
        currentExternalContent = content;
        currentExternalSignature = "30:3000";
        currentMetadataSignature = currentExternalSignature;
        return { signature: currentExternalSignature };
      }
      return undefined;
    });

    await act(async () => {
      render(<App />);
    });
    await screen.findByRole("heading", { name: "管理提示词" });
    return {
      files,
      setExternalContent: (content: string) => {
        currentExternalContent = content;
      },
      setExternalSignature: (signature: string) => {
        currentExternalSignature = signature;
      },
      setExternalMetadataSignature: (signature: string) => {
        currentMetadataSignature = signature;
      },
      getExternalReadCount: () => externalReadCount,
    };
  }

  async function renderPromptPopoverWithStore(promptData: unknown) {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) return JSON.stringify(promptData);
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            language: "zh-CN",
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null, buttonPosition: null },
            floatingButton: { visible: true },
            promptInsertion: { mode: "paste_and_submit" },
          });
        }
        throw new Error("missing file: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });
  }

  it("shows prompt list in popover mode by default", async () => {
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    await waitFor(() => {
      expect(screen.getByText("Test Prompt")).toBeTruthy();
    });
  });

  it("uses a transparent page shell only for prompt-list popovers", async () => {
    await renderPromptPopover();

    expect(document.querySelector(".popover-root")).toBeTruthy();
    expect(document.documentElement.classList.contains("popover-transparent-page")).toBe(true);
    expect(document.body.classList.contains("popover-transparent-page")).toBe(true);
  });

  it("uses the transparent page shell for button controls", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");

    await act(async () => {
      render(<App />);
    });

    expect(document.querySelector(".button-controls")).toBeTruthy();
    expect(document.querySelector(".popover-root")).toBeNull();
    expect(document.documentElement.classList.contains("popover-transparent-page")).toBe(true);
    expect(document.body.classList.contains("popover-transparent-page")).toBe(true);
  });

  it("refreshes prompt data when a reused popover is opened", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    const initialPrompts = JSON.stringify({ version: 1, prompts: mockPrompts });
    const refreshedPrompts = JSON.stringify({
      version: 1,
      prompts: [
        {
          ...mockPrompts[0],
          id: "2",
          title: "Fresh Prompt",
          body: "Fresh body",
        },
      ],
    });
    let promptData = initialPrompts;
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) return promptData;
        throw new Error("missing file");
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByText("Test Prompt")).toBeTruthy();
    await waitFor(() => {
      expect(listenMock).toHaveBeenCalledWith(
        "prompt-popover-opened",
        expect.any(Function)
      );
    });

    promptData = refreshedPrompts;
    const handler = eventHandlers.get("prompt-popover-opened");
    expect(handler).toBeTruthy();
    await act(async () => {
      await handler?.({ payload: "popover" });
    });

    expect(await screen.findByText("Fresh Prompt")).toBeTruthy();
    expect(screen.queryByText("Test Prompt")).toBeNull();
  });

  it("refreshes prompt insertion mode when a reused popover is opened", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    let insertionMode = "paste_and_submit";
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: true,
          sent: false,
          completion: "pasted_only",
          error: null,
          reason: null,
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) {
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        }
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            language: "zh-CN",
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null, buttonPosition: null },
            floatingButton: { visible: true },
            promptInsertion: { mode: insertionMode },
          });
        }
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByText("Test Prompt")).toBeTruthy();
    insertionMode = "paste_only";
    await act(async () => {
      await eventHandlers.get("prompt-popover-opened")?.({ payload: "popover" });
    });
    fireEvent.click(screen.getByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "none" }
      );
    });
  });

  it("clears visible prompt hover preview when a reused popover is opened", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();
    revealPromptPopoverTooltip();

    const handler = eventHandlers.get("prompt-popover-opened");
    expect(handler).toBeTruthy();
    await act(async () => {
      await handler?.({ payload: "popover" });
    });

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("does not emit Calico motion when a reused prompt popover opens", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();
    emitMock.mockClear();

    await act(async () => {
      await eventHandlers.get("prompt-popover-opened")?.({ payload: "popover" });
    });

    expect(emitMock).not.toHaveBeenCalledWith(
      "calico-motion",
      expect.objectContaining({ state: "thinking" })
    );
    expect(calicoMotionStates()).toEqual([]);
  });

  it("clears visible prompt hover preview when the popover is dismissed", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();
    revealPromptPopoverTooltip();

    await act(async () => {
      await eventHandlers.get("prompt-popover-dismissed")?.({ payload: undefined });
    });

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("applies native pointer tracking without activating the prompt popover", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();
    const option = screen.getByRole("option", { name: /Test Prompt/i });
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: vi.fn().mockReturnValue(option),
    });

    try {
      await act(async () => {
        await eventHandlers.get("prompt-popover-opened")?.({ payload: "popover" });
      });
      await act(async () => {
        await eventHandlers.get("prompt-popover-pointer-position")?.({
          payload: { x: 40, y: 80, inside: true },
        });
      });

      expect(option.classList.contains("is-hovered")).toBe(true);
    } finally {
      Reflect.deleteProperty(document, "elementFromPoint");
    }
  });

  it("ignores late native pointer events after the prompt popover is dismissed", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();
    const option = screen.getByRole("option", { name: /Test Prompt/i });
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: vi.fn().mockReturnValue(option),
    });

    try {
      await act(async () => {
        await eventHandlers.get("prompt-popover-opened")?.({ payload: "popover" });
        await eventHandlers.get("prompt-popover-dismissed")?.({ payload: undefined });
        await eventHandlers.get("prompt-popover-pointer-position")?.({
          payload: { x: 40, y: 80, inside: true },
        });
      });

      expect(option.classList.contains("is-hovered")).toBe(false);
    } finally {
      Reflect.deleteProperty(document, "elementFromPoint");
    }
  });

  it("does not select stale prompt rows while a reused popover is refreshing", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockResolvedValue(undefined);
    const initialPrompts = JSON.stringify({ version: 1, prompts: mockPrompts });
    const refreshedPrompts = JSON.stringify({
      version: 1,
      prompts: [
        {
          ...mockPrompts[0],
          id: "2",
          title: "Fresh Prompt",
          body: "Fresh body",
        },
      ],
    });
    let resolveRefresh: (value: string) => void = () => undefined;
    const refreshRead = new Promise<string>((resolve) => {
      resolveRefresh = resolve;
    });
    let promptReadCount = 0;
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (!path.includes("prompts")) throw new Error("missing file");
        promptReadCount += 1;
        return promptReadCount === 1 ? initialPrompts : refreshRead;
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByText("Test Prompt")).toBeTruthy();
    const handler = eventHandlers.get("prompt-popover-opened");
    let refreshResult: unknown;
    act(() => {
      refreshResult = handler?.({ payload: "popover" });
    });

    fireEvent.click(screen.getByText("Test Prompt"));
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 320));
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "paste_prompt_and_submit_to_last_target",
      expect.anything()
    );

    await act(async () => {
      resolveRefresh(refreshedPrompts);
      await refreshResult;
    });

    expect(await screen.findByText("Fresh Prompt")).toBeTruthy();
  });

  it("shows only prompt choices in popover mode", async () => {
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    await waitFor(() => {
      expect(screen.getByText("Test Prompt")).toBeTruthy();
    });

    expect(screen.queryByText("管理提示词")).toBeNull();
    expect(screen.queryByText("设置")).toBeNull();
    expect(screen.queryByText("导入")).toBeNull();
    expect(screen.queryByText("导出")).toBeNull();
  });

  it("does not start input target polling in prompt popover windows", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    await screen.findByText("Test Prompt");
    expect(inputTargetPollingMock).not.toHaveBeenCalled();
  });

  it("does not start input target polling in button controls windows", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) {
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        }
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null },
            floatingButton: { visible: true },
          });
        }
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });

    await screen.findByRole("button", { name: "关闭小猫" });
    expect(inputTargetPollingMock).not.toHaveBeenCalled();
  });

  it("starts input target polling in the main window", async () => {
    currentWindowLabel = "main";
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValue(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    await screen.findByRole("heading", { name: "管理提示词" });
    expect(inputTargetPollingMock).toHaveBeenCalled();
  });

  it("switches the main window to settings when the menu bar requests settings", async () => {
    currentWindowLabel = "main";
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(JSON.stringify({ version: 1, prompts: mockPrompts }))
      .mockResolvedValueOnce(JSON.stringify({
        version: 1,
        blacklistedApps: [],
        overlayPlacement: { buttonOffset: null, buttonPosition: null },
        floatingButton: { visible: true },
        promptInsertion: { mode: "paste_and_submit" },
      }));

    await act(async () => {
      render(<App />);
    });

    await screen.findByRole("heading", { name: "管理提示词" });
    await act(async () => {
      eventHandlers.get("open-settings-window")?.({ payload: null });
    });

    expect(screen.getByRole("heading", { name: "设置" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "填入 + Enter" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "返回管理提示词" })).toBeNull();
  });

  it("shows a settings back arrow when settings is opened from manager", async () => {
    await renderMainPromptManager();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    expect(await screen.findByRole("heading", { name: "设置" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "返回管理提示词" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "返回管理提示词" }));

    expect(await screen.findByRole("heading", { name: "管理提示词" })).toBeTruthy();
  });

  it("renders settings mode with the desktop settings panel shell", async () => {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/?mode=settings");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) {
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        }
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null, buttonPosition: null },
            floatingButton: { visible: true },
            promptInsertion: { mode: "paste_and_submit" },
            language: "zh-CN",
          });
        }
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByRole("heading", { name: "设置" })).toBeTruthy();
    expect(document.querySelector(".app-window-main")).toBeTruthy();
    expect(document.querySelector(".settings-panel")).toBeTruthy();
    expect(document.querySelector(".settings-card")).toBeTruthy();
  });

  it("renders manager mode with the polished prompt manager shell", async () => {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/?mode=manager");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts")) {
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        }
        if (path.includes("settings")) {
          return JSON.stringify({
            version: 1,
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null, buttonPosition: null },
            floatingButton: { visible: true },
            promptInsertion: { mode: "paste_and_submit" },
            language: "zh-CN",
          });
        }
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByRole("heading", { name: "管理提示词" })).toBeTruthy();
    expect(document.querySelector(".app-window-main")).toBeTruthy();
    expect(document.querySelector(".prompt-manager")).toBeTruthy();
    expect(document.querySelector(".panel-heading-with-actions")).toBeTruthy();
  });

  it("renders manager categories and filters prompts by active category", async () => {
    await renderMainPromptManagerWithStore({
      version: 3,
      categories: [devCategory, writingCategory],
      activeCategoryId: "cat-dev",
      containers: [
        makeContainer({ id: "dev-1", categoryId: "cat-dev", title: "Code Review" }),
        makeContainer({ id: "writing-1", categoryId: "cat-writing", title: "Blog Draft" }),
      ],
    });

    expect(screen.getByRole("button", { name: /^开发代码1$/ })).toBeTruthy();
    expect(screen.getByText("Code Review")).toBeTruthy();
    expect(screen.queryByText("Blog Draft")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /^写作1$/ }));

    expect(await screen.findByText("Blog Draft")).toBeTruthy();
    expect(screen.queryByText("Code Review")).toBeNull();
  });

  it("creates a category from the manager rail and selects it", async () => {
    const files = await renderMainPromptManager();

    fireEvent.click(screen.getByRole("button", { name: "新分类" }));
    fireEvent.change(screen.getByRole("textbox", { name: /分类名称/ }), {
      target: { value: "写作" },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: /分类名称/ }), { key: "Enter" });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /^写作0$/ }).getAttribute("aria-current"))
        .toBe("true");
    });

    const saved = JSON.parse(files.get("prompts.json") ?? "{}");
    expect(saved.categories.some((category: { name: string }) => category.name === "写作"))
      .toBe(true);
  });

  it("shows a visible error when deleting a non-empty category", async () => {
    await renderMainPromptManagerWithStore({
      version: 3,
      categories: [devCategory],
      activeCategoryId: "cat-dev",
      containers: [makeContainer({ id: "dev-1", categoryId: "cat-dev", title: "Code Review" })],
    });

    fireEvent.click(screen.getByRole("button", { name: /开发代码 的更多操作/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: "删除分类" }));
    fireEvent.click(screen.getByRole("button", { name: "删除分类" }));

    expect(await screen.findByRole("status")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("未能删除分类");
  });

  it("shows the localized default category name without changing stored data", async () => {
    const files = await renderMainPromptManagerWithStore({
      version: 3,
      categories: [{
        id: "category-default",
        name: "Default",
        order: 0,
        createdAt: "2026-05-26T00:00:00.000Z",
        updatedAt: "2026-05-26T00:00:00.000Z",
      }],
      activeCategoryId: "category-default",
      containers: [],
    });

    expect(screen.getByRole("button", { name: /^默认0$/ })).toBeTruthy();
    expect(files.get("prompts.json")).toContain("\"name\":\"Default\"");
  });

  it("quick picker switches prompt categories with tabs", async () => {
    await renderPromptPopoverWithStore({
      version: 3,
      categories: [devCategory, writingCategory],
      activeCategoryId: "cat-dev",
      containers: [
        makeContainer({ id: "dev-1", categoryId: "cat-dev", title: "Code Review" }),
        makeContainer({ id: "writing-1", categoryId: "cat-writing", title: "Blog Draft" }),
      ],
    });

    expect(await screen.findByText("Code Review")).toBeTruthy();
    expect(screen.queryByText("Blog Draft")).toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: "写作" }));

    expect(await screen.findByText("Blog Draft")).toBeTruthy();
    expect(screen.queryByText("Code Review")).toBeNull();
  });

  it("autosends selected prompt into the backend last input target", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "enter" }
      );
    });
  });

  it("migrates the removed command-enter global setting to Enter", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null, reason: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      if (path.includes("prompts")) {
        return JSON.stringify({
          version: 3,
          categories: [devCategory],
          containers: [
            makeContainer({
              id: "command-enter",
              title: "Command Enter",
              prompts: [{ id: "entry", body: "Command body", order: 0 }],
            }),
          ],
          activeCategoryId: devCategory.id,
        });
      }
      return JSON.stringify({
        version: 1,
        blacklistedApps: [],
        promptInsertion: { mode: "paste_command_enter" },
      });
    });

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Command Enter"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        {
          body: "Command body",
          submitKey: "enter",
        }
      );
    });
  });

  it("ignores a legacy prompt send behavior and uses the global setting", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: true,
          sent: false,
          completion: "pasted_only",
          error: null,
          reason: null,
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({
        version: 3,
        categories: [devCategory],
        containers: [
          makeContainer({
            id: "paste-only",
            title: "Paste Only",
            prompts: [{ id: "entry", body: "Paste body", order: 0 }],
            sendBehavior: "paste_only",
          }),
        ],
        activeCategoryId: devCategory.id,
      })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Paste Only"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Paste body", submitKey: "enter" }
      );
    });
    expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
      kind: "sent",
      message: "已填入输入框",
    });
  });

  it("emits autosend activity around single prompt sending", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const calls: string[] = [];
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      calls.push(`invoke:${command}`);
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    emitMock.mockImplementation(async (event: string, payload?: unknown) => {
      calls.push(`emit:${event}:${JSON.stringify(payload)}`);
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(calls).toContain("invoke:paste_prompt_and_submit_to_last_target");
      expect(calls).toContain('emit:prompt-autosend-activity:{"active":false}');
    });
    expect(calls).toContain('emit:prompt-autosend-activity:{"active":true}');
    expect(calls.indexOf('emit:prompt-autosend-activity:{"active":true}')).toBeLessThan(
      calls.indexOf("invoke:paste_prompt_and_submit_to_last_target")
    );
    expect(calls.indexOf("invoke:paste_prompt_and_submit_to_last_target")).toBeLessThan(
      calls.indexOf('emit:prompt-autosend-activity:{"active":false}')
    );
  });

  it("emits happy Calico motion for single prompt autosend success", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expectCalicoMotion("happy");
    });
    expect(calicoMotionStates()).not.toContain("working-typing");
  });

  it("pastes without pressing return when prompt insertion mode is paste only", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null, reason: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(JSON.stringify({ version: 1, prompts: mockPrompts }))
      .mockResolvedValueOnce(JSON.stringify({
        version: 1,
        blacklistedApps: [],
        overlayPlacement: { buttonOffset: null, buttonPosition: null },
        floatingButton: { visible: true },
        promptInsertion: { mode: "paste_only" },
      }));

    await act(async () => {
      render(<App />);
    });

    await screen.findByText("Test Prompt");
    await waitFor(() => {
      expect(readTextFile).toHaveBeenCalledWith(
        "settings.json",
        expect.objectContaining({ baseDir: "AppData" })
      );
    });
    fireEvent.click(screen.getByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "none" }
      );
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "paste_prompt_sequence_and_submit_to_last_target",
      expect.anything()
    );
    expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
      kind: "sent",
      message: "已填入输入框",
    });
    expectCalicoMotion("happy");
  });

  it("emits autosend activity around paste-only prompt insertion", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const calls: string[] = [];
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      calls.push(`invoke:${command}`);
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null, reason: null };
      }
      return undefined;
    });
    emitMock.mockImplementation(async (event: string, payload?: unknown) => {
      calls.push(`emit:${event}:${JSON.stringify(payload)}`);
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(JSON.stringify({ version: 1, prompts: mockPrompts }))
      .mockResolvedValueOnce(JSON.stringify({
        version: 1,
        blacklistedApps: [],
        overlayPlacement: { buttonOffset: null, buttonPosition: null },
        floatingButton: { visible: true },
        promptInsertion: { mode: "paste_only" },
      }));

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(calls).toContain("invoke:paste_prompt_and_submit_to_last_target");
      expect(calls).toContain('emit:prompt-autosend-activity:{"active":false}');
    });
    expect(calls.indexOf('emit:prompt-autosend-activity:{"active":true}')).toBeLessThan(
      calls.indexOf("invoke:paste_prompt_and_submit_to_last_target")
    );
    expect(calls.indexOf("invoke:paste_prompt_and_submit_to_last_target")).toBeLessThan(
      calls.indexOf('emit:prompt-autosend-activity:{"active":false}')
    );
  });

  it("emits a permission status when paste-only insertion lacks accessibility permission", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: false,
          sent: false,
          error: "Accessibility permission required for prompt insertion.",
          reason: "missing_accessibility_permission",
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(JSON.stringify({ version: 1, prompts: mockPrompts }))
      .mockResolvedValueOnce(JSON.stringify({
        version: 1,
        blacklistedApps: [],
        overlayPlacement: { buttonOffset: null, buttonPosition: null },
        floatingButton: { visible: true },
        promptInsertion: { mode: "paste_only" },
      }));

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
        kind: "failed",
        message: "请启用辅助功能权限",
      });
    });
    expectCalicoMotion("notification");
  });

  it("hides the prompt list before autosending a selected single prompt without throw animation", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const callOrder: string[] = [];
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      callOrder.push(`invoke:${command}`);
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    emitMock.mockImplementation(async (event: string) => {
      callOrder.push(`emit:${event}`);
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "enter" }
      );
    });
    expect(callOrder.indexOf("invoke:hide_prompt_popover")).toBeLessThan(
      callOrder.indexOf("invoke:paste_prompt_and_submit_to_last_target")
    );
    expect(emitMock).not.toHaveBeenCalledWith(
      "prompt-throw-send",
      expect.anything()
    );
  });

  it("autosends grouped prompts through the sequence backend command", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_sequence_and_submit_to_last_target") {
        return {
          copied: true,
          sent: true,
          sent_count: 2,
          failed_index: null,
          error: null,
          reason: null,
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({
        version: 2,
        containers: [
          {
            id: "group-1",
            title: "Repair Group",
            type: "group",
            prompts: [
              { id: "entry-1", body: "First prompt", order: 0 },
              { id: "entry-2", body: "Second prompt", order: 1 },
            ],
            intervalMs: 700,
            order: 0,
            createdAt: "2026-07-03T00:00:00.000Z",
            updatedAt: "2026-07-03T00:00:00.000Z",
          },
        ],
      })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Repair Group"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_sequence_and_submit_to_last_target",
        {
          bodies: ["First prompt", "Second prompt"],
          intervalMs: 700,
          submitKey: "enter",
        }
      );
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "paste_prompt_and_submit_to_last_target",
      expect.anything()
    );
    expectCalicoMotion("happy");
  });

  it("autosends grouped prompts without emitting a paper-plane throw event", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_sequence_and_submit_to_last_target") {
        return {
          copied: true,
          sent: true,
          sent_count: 2,
          failed_index: null,
          error: null,
          reason: null,
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({
        version: 2,
        containers: [
          {
            id: "group-1",
            title: "Repair Group",
            type: "group",
            prompts: [
              { id: "entry-1", body: "First prompt", order: 0 },
              { id: "entry-2", body: "Second prompt", order: 1 },
            ],
            intervalMs: 700,
            order: 0,
            createdAt: "2026-07-03T00:00:00.000Z",
            updatedAt: "2026-07-03T00:00:00.000Z",
          },
        ],
      })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Repair Group"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_sequence_and_submit_to_last_target",
        {
          bodies: ["First prompt", "Second prompt"],
          intervalMs: 700,
          submitKey: "enter",
        }
      );
    });
    expect(emitMock).not.toHaveBeenCalledWith(
      "prompt-throw-send",
      expect.anything()
    );
  });

  it("emits a sent status when autosend reports keyboard success", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
        kind: "sent",
        message: "已发送",
      });
    });
    expect(emitMock).not.toHaveBeenCalledWith(
      "prompt-throw-send",
      expect.anything()
    );
  });

  it("emits a permission status when autosend lacks accessibility permission", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: false,
          sent: false,
          error: "Accessibility permission required for prompt insertion.",
          reason: "missing_accessibility_permission",
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
        kind: "failed",
        message: "请启用辅助功能权限",
      });
    });
    expectCalicoMotion("notification");
    expect(emitMock).not.toHaveBeenCalledWith(
      "prompt-throw-send",
      expect.anything()
    );
    expect(emitMock).not.toHaveBeenCalledWith(
      "prompt-autosend-status",
      expect.objectContaining({ message: "已复制，可手动 Cmd+V" })
    );
  });

  it("emits a distinct status when autosend pastes but cannot press return", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: true,
          sent: false,
          error: "Native return event failed",
          reason: "return_event_failed",
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
        kind: "failed",
        message: "已填入输入框，未发送",
      });
    });
    expectCalicoMotion("notification");
  });

  it("emits a manual paste status when guarded autosend only copied", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return {
          copied: true,
          sent: false,
          error: "Target app changed before paste; prompt was copied instead.",
          reason: "no_safe_target",
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(emitMock).toHaveBeenCalledWith("prompt-autosend-status", {
        kind: "failed",
        message: "已复制，请手动粘贴",
      });
    });
  });

  it("hides the prompt popover before autosending the selected prompt", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      const calls = vi.mocked(invoke).mock.calls.map((call) => call[0]);
      expect(calls).toContain("hide_prompt_popover");
      expect(calls).toContain("paste_prompt_and_submit_to_last_target");
      expect(calls.indexOf("hide_prompt_popover")).toBeLessThan(
        calls.indexOf("paste_prompt_and_submit_to_last_target")
      );
    });
  });

  it("waits long enough for the popover to hide before autosend", async () => {
    const setTimeoutSpy = vi.spyOn(window, "setTimeout");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      vi.mocked(invoke).mockClear();
      vi.mocked(invoke).mockImplementation(async (command: string) => {
        if (command === "paste_prompt_and_submit_to_last_target") {
          return { copied: true, sent: true, error: null };
        }
        return undefined;
      });
      const { readTextFile } = await import("@tauri-apps/plugin-fs");
      (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
        JSON.stringify({ version: 1, prompts: mockPrompts })
      );

      await act(async () => {
        render(<App />);
      });

      fireEvent.click(await screen.findByText("Test Prompt"));

      await waitFor(() => {
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("hide_prompt_popover");
      });
      await waitFor(() => {
        expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 260);
      });
      await waitFor(() => {
        expect(vi.mocked(invoke)).toHaveBeenCalledWith(
          "paste_prompt_and_submit_to_last_target",
          { body: "Test body", submitKey: "enter" }
        );
      });
    } finally {
      setTimeoutSpy.mockRestore();
    }
  });

  it("does not run a frontend accessibility preflight before autosend", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("hide_prompt_popover");
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "enter" }
      );
    });

    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("accessibility_status_cmd");
  });

  it("hides the prompt popover and logs autosend failures without a blocking dialog", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const calls: string[] = [];
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "hide_prompt_popover") {
        calls.push("hide");
        return undefined;
      }
      if (command === "paste_prompt_and_submit_to_last_target") {
        calls.push("autosend");
        throw new Error("Unexpected autosend failure.");
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(calls).toEqual(["hide", "autosend"]);
      expect(warn).toHaveBeenCalledWith(
        "Prompt autosend failed without blocking the picker:",
        expect.any(Error)
      );
    });
    expectCalicoMotion("notification");
    warn.mockRestore();
  });

  it("does not move the floating button when selecting a prompt from the popover", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        return { copied: true, sent: true, error: null };
      }
      return undefined;
    });
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "paste_prompt_and_submit_to_last_target",
        { body: "Test body", submitKey: "enter" }
      );
    });

    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "show_prompt_button",
      expect.anything()
    );
    expect(inputTargetPollingMock).not.toHaveBeenCalled();
  });

  it("does not fall back to blind paste when no input target is recorded", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        throw new Error("Click into a text field first, then choose a prompt.");
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(warn).toHaveBeenCalledWith(
        "Prompt autosend failed without blocking the picker:",
        expect.any(Error)
      );
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("paste_prompt", {
      body: "Test body",
    });
    warn.mockRestore();
  });

  it("does not show a blocking permission dialog when backend autosend reports accessibility failure", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "paste_prompt_and_submit_to_last_target") {
        throw new Error(
          "Accessibility permission required for autosend. Enable Sleepy Cat in System Settings > Privacy & Security > Accessibility, then try again."
        );
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByText("Test Prompt"));

    await waitFor(() => {
      expect(warn).toHaveBeenCalledWith(
        "Prompt autosend failed without blocking the picker:",
        expect.any(Error)
      );
    });
    warn.mockRestore();
  });

  it("opens the main app window directly on prompt management", async () => {
    currentWindowLabel = "main";
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValue(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "管理提示词" })
      ).toBeTruthy();
    });

    expect(screen.queryByText("Floating Button")).toBeNull();
    expect(screen.queryByText("Hide Floating Button")).toBeNull();
    expect(screen.queryByRole("heading", { name: "设置" })).toBeNull();
    expect(screen.queryByPlaceholderText("标题")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "+ 添加提示词" }));
    expect(screen.getByPlaceholderText("标题")).toBeTruthy();
  });

  it("shows a newly saved prompt in the main window prompt list", async () => {
    currentWindowLabel = "main";
    const files = new Map<string, string>();
    const { readTextFile, writeTextFile } = await import(
      "@tauri-apps/plugin-fs"
    );
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        const value = files.get(path);
        if (!value) throw new Error("missing file");
        return value;
      }
    );
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string, value: string) => {
        files.set(path, value);
      }
    );

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(screen.getByRole("button", { name: "+ 添加提示词" }));
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "My Prompt" },
    });
    fireEvent.change(screen.getByPlaceholderText("提示词内容..."), {
      target: { value: "My saved prompt body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加提示词" }));

    await waitFor(() => {
      expect(screen.getByText("My Prompt")).toBeTruthy();
    });
    expectCalicoMotion("happy");
  });

  it("emits happy after creating a prompt group", async () => {
    await renderMainPromptManager();

    fireEvent.click(screen.getByRole("button", { name: "+ 添加提示词" }));
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "Grouped Work" },
    });
    fireEvent.change(screen.getAllByLabelText(/提示词 \d+ 内容/i)[0], {
      target: { value: "First body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加群组" }));

    await waitFor(() => {
      expect(screen.getByText("Grouped Work")).toBeTruthy();
    });
    expectCalicoMotion("happy");
  });

  it("applies committed combine and split snapshots without a post-commit read", async () => {
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    await renderMainPromptManager([
      makeContainer({
        id: "first",
        title: "First",
        prompts: [{ id: "first-entry", body: "First body", order: 0 }],
        order: 0,
      }),
      makeContainer({
        id: "second",
        title: "Second",
        prompts: [{ id: "second-entry", body: "Second body", order: 0 }],
        order: 1,
      }),
    ]);
    const promptReadCount = () => vi.mocked(readTextFile).mock.calls.filter(
      ([path]) => String(path).endsWith("prompts.json")
    ).length;

    const beforeCombine = promptReadCount();
    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 First" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Second" }));
    fireEvent.click(screen.getByRole("button", { name: "合并为群组" }));
    fireEvent.change(screen.getByDisplayValue("新提示词组"), {
      target: { value: "Combined" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建群组并删除原提示词" }));

    await screen.findByText("Combined");
    expect(promptReadCount() - beforeCombine).toBe(1);

    const beforeSplit = promptReadCount();
    fireEvent.click(screen.getByRole("button", { name: "Combined 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "拆分为单条" }));
    fireEvent.click(screen.getByRole("button", { name: "确认拆分" }));

    await screen.findByText("First");
    expect(screen.getByText("Second")).toBeTruthy();
    expect(promptReadCount() - beforeSplit).toBe(1);
  });

  it("emits happy motion when deleting a prompt", async () => {
    await renderMainPromptManager([
      {
        id: "delete-1",
        categoryId: "category-default",
        title: "Delete Me",
        type: "single",
        sendBehavior: "inherit",
        prompts: [{ id: "delete-1-entry", body: "Body", order: 0 }],
        intervalMs: 700,
        order: 0,
        createdAt: "2026-07-03T00:00:00.000Z",
        updatedAt: "2026-07-03T00:00:00.000Z",
      },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Delete Me 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "删除" }));
    fireEvent.click(screen.getByRole("button", { name: "确认" }));

    await waitFor(() => {
      expect(screen.queryByText("Delete Me")).toBeNull();
    });
    expectCalicoMotion("happy");
  });

  it("uses neutral feedback for reorder, import, and export", async () => {
    const files = await renderMainPromptManager([
      {
        id: "first",
        categoryId: "category-default",
        title: "First",
        type: "single",
        sendBehavior: "inherit",
        prompts: [{ id: "first-entry", body: "First body", order: 0 }],
        intervalMs: 700,
        order: 0,
        createdAt: "2026-07-03T00:00:00.000Z",
        updatedAt: "2026-07-03T00:00:00.000Z",
      },
      {
        id: "second",
        categoryId: "category-default",
        title: "Second",
        type: "single",
        sendBehavior: "inherit",
        prompts: [{ id: "second-entry", body: "Second body", order: 0 }],
        intervalMs: 700,
        order: 1,
        createdAt: "2026-07-03T00:00:00.000Z",
        updatedAt: "2026-07-03T00:00:00.000Z",
      },
    ]);
    files.set("import.json", JSON.stringify({ version: 2, containers: [] }));
    const { open, save } = await import("@tauri-apps/plugin-dialog");
    (open as ReturnType<typeof vi.fn>).mockResolvedValue("import.json");
    (save as ReturnType<typeof vi.fn>).mockResolvedValue("export.json");

    fireEvent.click(screen.getAllByText("↓")[0]);
    await waitFor(() => {
      expectCalicoMotion("react-poke");
    });

    fireEvent.click(screen.getByRole("button", { name: "导入" }));
    fireEvent.click(await screen.findByRole("button", { name: "导入提示词库" }));
    await waitFor(() => {
      expectCalicoMotion("happy");
    });

    fireEvent.click(screen.getByRole("button", { name: "导出" }));
    await waitFor(() => {
      expect(files.has("export.json")).toBe(true);
    });

    expect(calicoMotionStates()).not.toContain("working-carrying");
  });

  it("loads prompts from a linked prompt library file after settings load", async () => {
    currentWindowLabel = "main";
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "read_prompt_library_file") {
        return {
          content: JSON.stringify({
            version: 2,
            containers: [makeContainer({ id: "linked", title: "Linked Prompt" })],
          }),
          signature: "20:2000",
        };
      }
      return undefined;
    });
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      if (path === "prompts.json") {
        return JSON.stringify({
          version: 2,
          containers: [makeContainer({ id: "local", title: "Local Prompt" })],
        });
      }
      if (path === "settings.json") {
        return JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
          permissions: { accessibilityPromptRequested: false },
          promptLibraryLink: {
            mode: "linked",
            path: "/Users/example/Desktop/prompts.json",
            lastKnownSignature: "10:1000",
            lastSyncedAt: "2026-07-06T00:00:00.000Z",
          },
        });
      }
      throw new Error("missing file: " + path);
    });

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByText("Linked Prompt")).toBeTruthy();
  });

  it("keeps import as copy by default and does not write the old linked file", async () => {
    currentWindowLabel = "main";
    const files = new Map<string, string>([
      [
        "prompts.json",
        JSON.stringify({ version: 2, containers: [makeContainer({ title: "Old Local" })] }),
      ],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
          permissions: { accessibilityPromptRequested: false },
          promptLibraryLink: {
            mode: "linked",
            path: "/Users/example/Desktop/old.json",
            lastKnownSignature: "10:1000",
            lastSyncedAt: "2026-07-06T00:00:00.000Z",
          },
        }),
      ],
      [
        "/Users/example/Desktop/new.json",
        JSON.stringify({ version: 2, containers: [makeContainer({ title: "Imported Copy" })] }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      const value = files.get(path);
      if (!value) throw new Error("missing file: " + path);
      return value;
    });
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string, value: string) => {
      files.set(path, value);
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    (open as ReturnType<typeof vi.fn>).mockResolvedValue("/Users/example/Desktop/new.json");
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "read_prompt_library_file") {
        return {
          content: files.get("prompts.json") ?? "",
          signature: "10:1000",
        };
      }
      return undefined;
    });

    await act(async () => {
      render(<App />);
    });
    fireEvent.click(await screen.findByRole("button", { name: "导入" }));
    fireEvent.click(await screen.findByRole("button", { name: "导入提示词库" }));

    await waitFor(() => {
      expect(screen.getByText("Imported Copy")).toBeTruthy();
    });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "write_prompt_library_file",
      expect.objectContaining({ path: "/Users/example/Desktop/old.json" })
    );
  });

  it("links and syncs an imported prompt library file when selected", async () => {
    currentWindowLabel = "main";
    const files = new Map<string, string>([
      ["prompts.json", JSON.stringify({ version: 2, containers: [] })],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
          permissions: { accessibilityPromptRequested: false },
        }),
      ],
      [
        "/Users/example/Desktop/link.json",
        JSON.stringify({ version: 2, containers: [makeContainer({ title: "Linked Import" })] }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      const value = files.get(path);
      if (!value) throw new Error("missing file: " + path);
      return value;
    });
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string, value: string) => {
      files.set(path, value);
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    (open as ReturnType<typeof vi.fn>).mockResolvedValue("/Users/example/Desktop/link.json");
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "write_prompt_library_file") return { signature: "30:3000" };
      return undefined;
    });

    await act(async () => {
      render(<App />);
    });
    fireEvent.click(await screen.findByRole("button", { name: "导入" }));
    fireEvent.click(await screen.findByLabelText("链接并同步这个文件"));
    fireEvent.click(screen.getByRole("button", { name: "导入提示词库" }));

    await waitFor(() => {
      expect(screen.getByText("Linked Import")).toBeTruthy();
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      "write_prompt_library_file",
      expect.objectContaining({ path: "/Users/example/Desktop/link.json" })
    );
    expect(JSON.parse(files.get("settings.json") ?? "{}").promptLibraryLink).toMatchObject({
      mode: "linked",
      path: "/Users/example/Desktop/link.json",
      lastKnownSignature: "30:3000",
    });
  });

  it("keeps the edit form open when a linked-file conflict blocks update", async () => {
    currentWindowLabel = "main";
    const prompt = makeContainer({ id: "conflict", title: "Conflict Prompt" });
    const files = new Map<string, string>([
      ["prompts.json", JSON.stringify({ version: 2, containers: [prompt] })],
      [
        "settings.json",
        JSON.stringify({
          version: 1,
          language: "zh-CN",
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
          permissions: { accessibilityPromptRequested: false },
          promptLibraryLink: {
            mode: "linked",
            path: "/Users/example/Desktop/prompts.json",
            lastKnownSignature: "10:1000",
            lastSyncedAt: "2026-07-06T00:00:00.000Z",
          },
        }),
      ],
    ]);
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      const value = files.get(path);
      if (!value) throw new Error("missing file: " + path);
      return value;
    });
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string, value: string) => {
      files.set(path, value);
    });
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "read_prompt_library_file") {
        return { content: files.get("prompts.json"), signature: "10:1000" };
      }
      if (command === "prompt_library_file_metadata") return { signature: "changed" };
      return undefined;
    });

    await act(async () => {
      render(<App />);
    });
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByDisplayValue("Conflict Prompt"), {
      target: { value: "Edited Conflict Prompt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(screen.getByDisplayValue("Edited Conflict Prompt")).toBeTruthy();
    });
  });

  it("falls back to AppData without showing linked prompt library controls when linked read fails", async () => {
    const localPrompt = makeContainer({ id: "local-fallback", title: "Local Fallback" });

    await renderLinkedPromptManager({
      localContainers: [localPrompt],
      readExternal: async () => { throw new Error("missing linked file"); },
    });

    expect(await screen.findByText("Local Fallback")).toBeTruthy();
    expect(screen.queryByText(/Prompt library:/)).toBeNull();
    expect(screen.queryByRole("button", { name: "立即同步" })).toBeNull();
    expect(screen.queryByRole("button", { name: "取消同步" })).toBeNull();
    expect(screen.queryByText("外部提示词文件同步失败，请检查文件或取消同步。")).toBeNull();
  });

  it("syncs linked prompt library changes in the background", async () => {
    const initialPrompt = makeContainer({ id: "linked-initial", title: "Linked Initial" });
    const updatedPrompt = makeContainer({ id: "linked-updated", title: "Linked Updated" });
    const linked = await renderLinkedPromptManager({
      externalContent: JSON.stringify({ version: 2, containers: [initialPrompt] }),
    });

    expect(await screen.findByText("Linked Initial")).toBeTruthy();

    linked.setExternalContent(JSON.stringify({ version: 2, containers: [updatedPrompt] }));
    linked.setExternalSignature("30:3000");

    await waitFor(() => {
      expect(linked.getExternalReadCount()).toBeGreaterThan(1);
      expect(screen.getByText("Linked Updated")).toBeTruthy();
    }, { timeout: 7000 });
  }, 10000);

  it("keeps the current prompts when background linked sync reads invalid prompt data", async () => {
    const initialPrompt = makeContainer({ id: "linked-valid", title: "Linked Valid" });
    const linked = await renderLinkedPromptManager({
      externalContent: JSON.stringify({ version: 2, containers: [initialPrompt] }),
    });

    expect(await screen.findByText("Linked Valid")).toBeTruthy();

    linked.setExternalContent(JSON.stringify({ foo: true }));
    linked.setExternalSignature("30:3000");

    await waitFor(() => {
      expect(linked.getExternalReadCount()).toBeGreaterThan(1);
      expect(screen.getByText("Linked Valid")).toBeTruthy();
      expect(screen.queryByText("外部提示词文件同步失败，请检查文件或取消同步。")).toBeNull();
    }, { timeout: 7000 });
  }, 10000);

  it("keeps local edits visible when linked external write fails", async () => {
    const prompt = makeContainer({ id: "write-failure", title: "Write Failure Prompt" });
    await renderLinkedPromptManager({
      externalContent: JSON.stringify({ version: 2, containers: [prompt] }),
      writeExternal: async () => { throw new Error("denied"); },
    });

    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByDisplayValue("Write Failure Prompt"), {
      target: { value: "Edited Write Failure Prompt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(screen.getByText("Edited Write Failure Prompt")).toBeTruthy();
    });
    await waitFor(() => {
      expect(screen.getByText("Edited Write Failure Prompt")).toBeTruthy();
    }, { timeout: 7000 });
    expect(screen.queryByRole("button", { name: "立即同步" })).toBeNull();
    expect(screen.queryByRole("button", { name: "取消同步" })).toBeNull();
  });

  it("does not show floating button controls on the main prompt management page", async () => {
    currentWindowLabel = "main";
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockResolvedValue(
      JSON.stringify({ version: 1, prompts: mockPrompts })
    );

    await act(async () => {
      render(<App />);
    });

    expect(await screen.findByRole("heading", { name: "管理提示词" })).toBeTruthy();
    expect(screen.queryByText("Status: Visible")).toBeNull();
    expect(screen.queryByText("Autosend: Ready")).toBeNull();
    expect(screen.queryByRole("button", { name: "Hide Floating Button" })).toBeNull();
  });

  it("renders button controls mode without prompt management UI", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(
      async (path: string) => {
        if (path.includes("prompts"))
          return JSON.stringify({ version: 1, prompts: mockPrompts });
        if (path.includes("settings"))
          return JSON.stringify({
            version: 1,
            blacklistedApps: [],
            overlayPlacement: { buttonOffset: null },
            floatingButton: { visible: true },
          });
        throw new Error("unexpected path: " + path);
      }
    );

    await act(async () => {
      render(<App />);
    });

    expect(
      await screen.findByRole("button", { name: "关闭小猫" })
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "管理提示词..." })).toBeNull();
    expect(screen.queryByRole("button", { name: "打开辅助功能设置" })).toBeNull();
    expect(screen.queryByRole("button", { name: "退出 Sleepy Cat" })).toBeNull();
    expect(screen.queryByText("导入")).toBeNull();
    expect(screen.queryByText("导出")).toBeNull();
  });

  it("button controls hide persists state and hides the floating button", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "set_prompt_button_visibility") {
        return { visible: false, applied: true, persisted: true, error: null };
      }
      return undefined;
    });
    vi.mocked(invoke).mockClear();
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");
    const files = new Map<string, string>();
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      if (path.includes("prompts")) return JSON.stringify({ version: 1, prompts: mockPrompts });
      if (path.includes("settings")) return JSON.stringify({ version: 1, blacklistedApps: [], overlayPlacement: { buttonOffset: null }, floatingButton: { visible: true } });
      throw new Error("missing file");
    });
    (writeTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string, value: string) => {
      files.set(path, value);
    });

    await act(async () => {
      render(<App />);
    });

    await screen.findByRole("button", { name: "关闭小猫" });
    fireEvent.click(screen.getByRole("button", { name: "关闭小猫" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_prompt_button_visibility", {
        visible: false,
      });
    });
    const allCalls = (vi.mocked(invoke) as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0]);
    expect(allCalls).not.toContain("hide_prompt_button");
    expect(allCalls).not.toContain("hide_prompt_popover");
    expect(allCalls).not.toContain("open_main_window");
    expect(allCalls).not.toContain("open_accessibility_settings");
    expect(allCalls).not.toContain("quit_prompt_picker");
  });

  it("does not manually emit prompt-popover-dismissed when hiding Calico from button controls", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "set_prompt_button_visibility") {
        return { visible: false, applied: true, persisted: true, error: null };
      }
      return undefined;
    });
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      if (path.includes("prompts")) return JSON.stringify({ version: 1, prompts: mockPrompts });
      if (path.includes("settings")) {
        return JSON.stringify({
          version: 1,
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null },
          floatingButton: { visible: true },
        });
      }
      throw new Error("unexpected path: " + path);
    });

    await act(async () => {
      render(<App />);
    });

    fireEvent.click(await screen.findByRole("button", { name: "关闭小猫" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_prompt_button_visibility", {
        visible: false,
      });
    });
    expect(emitMock).not.toHaveBeenCalledWith("prompt-popover-dismissed");
  });

  it("applies native prompt button visibility events without writing settings", async () => {
    currentWindowLabel = "main";
    window.history.pushState({}, "", "/?mode=manager");
    const { readTextFile, writeTextFile } = await import("@tauri-apps/plugin-fs");
    vi.mocked(readTextFile).mockImplementation(async (path) => {
      const filePath = String(path);
      if (filePath.includes("prompts")) {
        return JSON.stringify({ version: 1, prompts: mockPrompts });
      }
      if (filePath.includes("settings")) {
        return JSON.stringify({
          version: 1,
          blacklistedApps: [],
          overlayPlacement: { buttonOffset: null, buttonPosition: null },
          floatingButton: { visible: true },
          promptInsertion: { mode: "paste_and_submit" },
        });
      }
      throw new Error("missing file");
    });
    await act(async () => {
      render(<App />);
    });
    await screen.findByRole("heading", { name: "管理提示词" });
    vi.mocked(writeTextFile).mockClear();

    await act(async () => {
      await eventHandlers.get("prompt-button-visibility-changed")?.({ payload: false });
    });

    await waitFor(() => {
      expect(inputTargetPollingMock).toHaveBeenLastCalledWith(
        expect.any(Array),
        expect.any(Object),
        expect.any(Object),
        false
      );
    });
    expect(writeTextFile).not.toHaveBeenCalled();
  });

  it("acknowledges a reused popover mode after React commits it", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=popover");
    await renderPromptPopover();

    await act(async () => {
      await eventHandlers.get("prompt-popover-mode-requested")?.({
        payload: { requestId: 19, mode: "button-controls" },
      });
    });

    expect(await screen.findByRole("button", { name: "关闭小猫" })).toBeTruthy();
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("acknowledge_prompt_popover_mode", {
        requestId: 19,
        mode: "button-controls",
      });
    });
  });

  it("button controls mode only renders the close pet action", async () => {
    currentWindowLabel = "prompt-popover";
    window.history.pushState({}, "", "/?mode=button-controls");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    (readTextFile as ReturnType<typeof vi.fn>).mockImplementation(async (path: string) => {
      if (path.includes("prompts")) return JSON.stringify({ version: 1, prompts: mockPrompts });
      if (path.includes("settings")) return JSON.stringify({ version: 1, blacklistedApps: [], overlayPlacement: { buttonOffset: null }, floatingButton: { visible: true } });
      throw new Error("missing file");
    });

    await act(async () => { render(<App />); });

    expect(screen.queryByRole("button", { name: "关闭小猫" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "管理提示词..." })).toBeNull();
    expect(screen.queryByRole("button", { name: "打开辅助功能设置" })).toBeNull();
    expect(screen.queryByRole("button", { name: "退出 Sleepy Cat" })).toBeNull();
    expect(screen.queryByText("设置")).toBeNull();
  });
});
