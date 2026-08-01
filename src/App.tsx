import { useState, useEffect, useLayoutEffect, useRef, useCallback, useMemo } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import type { PromptCategory, PromptContainer } from "./shared/promptTypes";
import { getPromptContainerBodies } from "./shared/promptTypes";
import type { AppLanguage, PromptInsertionMode, Settings } from "./shared/settingsStore";
import { createSettingsStore } from "./shared/settingsStore";
import { getMessages, type Messages } from "./shared/i18n";
import {
  DEFAULT_CATEGORY_ID,
  createPromptStore,
  type PromptLibrarySnapshot,
} from "./shared/promptStore";
import { createPromptLibrarySyncStorage } from "./storage/promptLibrarySyncStorage";
import { createTauriPromptStorage } from "./storage/tauriPromptStorage";
import { createTauriSettingsStorage } from "./storage/tauriSettingsStorage";
import {
  acknowledgePromptPopoverMode,
  getPromptLibraryFileMetadata,
  hidePromptPopover,
  pastePromptAndSubmitToLastTarget,
  pastePromptSequenceAndSubmitToLastTarget,
  readPromptLibraryFile,
  setMenuLanguage,
  setPromptButtonVisibility,
  writePromptLibraryFile,
} from "./platform/platformApi";
import type { AutosendOutcome, AutosendSequenceOutcome, NativeSubmitKey } from "./platform/platformApi";
import { useInputTargetPolling } from "./overlay/useInputTargetPolling";
import { PromptQuickList } from "./ui/PromptQuickList";
import type { NativePopoverPointerPosition } from "./ui/PromptQuickList";
import { PromptManager } from "./ui/PromptManager";
import { SettingsPanel } from "./ui/SettingsPanel";
import type { AppMode } from "./app/AppMode";
import "./styles.css";

interface AppProps {
  settings?: Settings;
}

const DEFAULT_SETTINGS: Settings = {
  version: 1,
  blacklistedApps: [],
  overlayPlacement: { buttonOffset: null, buttonPosition: null },
  floatingButton: { visible: true },
  promptInsertion: { mode: "paste_enter" },
  permissions: { accessibilityPromptRequested: false },
  promptLibraryLink: {
    mode: "copy",
    path: null,
    lastKnownSignature: null,
    lastSyncedAt: null,
  },
  language: "zh-CN",
};

const waitForWindowHide = () => new Promise((resolve) => window.setTimeout(resolve, 260));
const LINKED_PROMPT_LIBRARY_REFRESH_MS = 5000;

type AutosendStatusKind = "sent" | "failed";

async function emitAutosendStatus(
  kind: AutosendStatusKind,
  message: string
) {
  try {
    await emit("prompt-autosend-status", { kind, message });
  } catch (error) {
    console.warn("Failed to emit autosend status:", error);
  }
}

async function emitAutosendActivity(active: boolean) {
  try {
    await emit("prompt-autosend-activity", { active });
  } catch (error) {
    console.warn("Failed to emit autosend activity:", error);
  }
}

type CalicoMotionState =
  | "thinking"
  | "working-typing"
  | "working-conducting"
  | "working-juggling"
  | "working-building"
  | "working-carrying"
  | "working-sweeping"
  | "notification"
  | "error"
  | "happy"
  | "react-poke";

type CalicoMotionPayload = {
  state: CalicoMotionState;
  reason: string;
  durationMs?: number;
};

type PendingPromptImport = {
  path: string;
  content: string;
  linkAndSync: boolean;
};

type PendingPopoverModeRequest = {
  requestId: number;
  mode: "popover" | "button-controls";
};

function emitCalicoMotion(state: CalicoMotionState, reason: string, durationMs?: number) {
  const payload: CalicoMotionPayload = { state, reason };
  if (durationMs !== undefined) {
    payload.durationMs = durationMs;
  }
  emit("calico-motion", payload).catch((error) => {
    console.warn("Failed to emit Calico motion:", error);
  });
}

function pasteOnlyBody(prompt: PromptContainer, bodies: string[]): string {
  if (prompt.type === "group") return bodies.join("\n\n");
  return bodies[0] ?? "";
}

function submitKeyForMode(mode: PromptInsertionMode): NativeSubmitKey {
  switch (mode) {
    case "paste_only":
      return "none";
    case "paste_enter":
    default:
      return "enter";
  }
}

function statusForAutosendOutcome(
  outcome: AutosendOutcome,
  t: Messages,
  successMessage: string = t.autosend.sent
): {
  kind: AutosendStatusKind;
  message: string;
  requiresAttention?: boolean;
} {
  if (outcome.completion === "pasted_only") {
    return { kind: "sent", message: t.autosend.insertedIntoInput };
  }
  if (outcome.completion === "submitted" || outcome.sent) {
    return { kind: "sent", message: successMessage };
  }

  switch (outcome.reason) {
    case "missing_accessibility_permission":
      return {
        kind: "failed",
        message: t.autosend.enableAccessibility,
        requiresAttention: true,
      };
    case "no_safe_target":
      return { kind: "failed", message: t.autosend.copiedNotSent };
    case "copy_failed":
      return { kind: "failed", message: t.autosend.copyFailed };
    case "paste_event_failed":
      return { kind: "failed", message: t.autosend.pasteFailed };
    case "return_event_failed":
      return { kind: "failed", message: t.autosend.pastedNotSent };
    case "target_focus_failed":
      return { kind: "failed", message: t.autosend.targetFocusFailed };
    case "target_changed":
      return { kind: "failed", message: t.autosend.targetChanged };
    case "composer_not_found":
      return { kind: "failed", message: t.autosend.composerNotFound };
    case "composer_ambiguous":
      return { kind: "failed", message: t.autosend.composerAmbiguous };
    case "focus_not_acquired":
      return { kind: "failed", message: t.autosend.focusNotAcquired };
    case "paste_not_confirmed":
      return { kind: "failed", message: t.autosend.pasteNotConfirmed };
    default:
      return {
        kind: "failed",
        message: outcome.copied ? t.autosend.automaticFailed : t.autosend.copyFailed,
      };
  }
}

function statusForAutosendSequenceOutcome(outcome: AutosendSequenceOutcome, t: Messages): {
  kind: AutosendStatusKind;
  message: string;
  requiresAttention?: boolean;
} {
  if (outcome.completion === "pasted_only") {
    return { kind: "sent", message: t.autosend.insertedIntoInput };
  }
  if (outcome.completion === "submitted" || outcome.sent) {
    return { kind: "sent", message: t.autosend.sent };
  }
  if (outcome.reason === "missing_accessibility_permission") {
    return {
      kind: "failed",
      message: t.autosend.enableAccessibility,
      requiresAttention: true,
    };
  }
  return {
    kind: "failed",
    message: t.autosend.sequenceFailed(outcome.failed_index ?? 1),
  };
}

function isAccessibilityPermissionError(error: unknown): boolean {
  const message = error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "";
  return message.toLowerCase().includes("accessibility permission");
}

interface InputTargetPollingControllerProps {
  settings: Settings;
  onButtonDragEnd: (
    position: { x: number; y: number },
    basePosition: [number, number] | null
  ) => void;
}

function InputTargetPollingController({
  settings,
  onButtonDragEnd,
}: InputTargetPollingControllerProps) {
  useInputTargetPolling(
    settings.blacklistedApps.map((app) => app.bundleId),
    settings.overlayPlacement,
    { onButtonDragEnd },
    settings.floatingButton.visible
  );

  return null;
}

function initialWindowLabel() {
  return new URLSearchParams(window.location.search).has("mode")
    ? "prompt-popover"
    : "main";
}

function currentWindowLabel() {
  try {
    return getCurrentWindow().label;
  } catch {
    return initialWindowLabel();
  }
}

export function App({
  settings = DEFAULT_SETTINGS,
}: AppProps) {
  const [mode, setMode] = useState<AppMode>(() => {
    const initialMode = new URLSearchParams(window.location.search).get("mode");
    if (initialMode === "manager") return "manager";
    if (initialMode === "settings") return "settings";
    if (initialMode === "button-controls") return "button-controls";
    return currentWindowLabel() === "main" ? "manager" : "popover";
  });
  const [windowLabel, setWindowLabel] = useState(currentWindowLabel);
  const [prompts, setPrompts] = useState<PromptContainer[]>([]);
  const [categories, setCategories] = useState<PromptCategory[]>([]);
  const [activeCategoryId, setActiveCategoryId] = useState<string | null>(null);
  const [categoryActionError, setCategoryActionError] = useState<string | null>(null);
  const [submittingPromptId, setSubmittingPromptId] = useState<string | null>(null);
  const [activeSettings, setActiveSettings] = useState<Settings>(settings);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [hoverResetKey, setHoverResetKey] = useState(0);
  const [nativePopoverPointer, setNativePopoverPointer] =
    useState<NativePopoverPointerPosition | null>(null);
  const [settingsReturnTarget, setSettingsReturnTarget] = useState<"manager" | null>(null);
  const [promptLibraryDraftActive, setPromptLibraryDraftActive] = useState(false);
  const [pendingPromptImport, setPendingPromptImport] = useState<PendingPromptImport | null>(null);
  const [pendingPopoverModeRequest, setPendingPopoverModeRequest] =
    useState<PendingPopoverModeRequest | null>(null);
  const activeSettingsRef = useRef<Settings>(settings);
  const appDataStorageRef = useRef(createTauriPromptStorage());
  const settingsStoreRef = useRef(createSettingsStore(createTauriSettingsStorage()));
  const applyActiveSettings = useCallback((nextSettings: Settings) => {
    activeSettingsRef.current = nextSettings;
    setActiveSettings(nextSettings);
  }, []);
  const appDataPromptStoreRef = useRef(createPromptStore(appDataStorageRef.current));
  const promptStorageRef = useRef(createPromptLibrarySyncStorage({
    appDataStorage: appDataStorageRef.current,
    getLink: async () => activeSettingsRef.current.promptLibraryLink,
    setLink: async (promptLibraryLink) => {
      await settingsStoreRef.current.setPromptLibraryLink(promptLibraryLink);
      applyActiveSettings(await settingsStoreRef.current.get());
    },
    readExternal: readPromptLibraryFile,
    writeExternal: writePromptLibraryFile,
    getExternalMetadata: getPromptLibraryFileMetadata,
  }));
  const storeRef = useRef(createPromptStore(promptStorageRef.current));
  const promptListRefreshingRef = useRef(false);
  const autosendInFlightRef = useRef(false);
  const popoverSessionOpenRef = useRef(false);
  const t = getMessages(activeSettings.language);
  const applyPromptData = useCallback((data: PromptLibrarySnapshot) => {
    setPrompts(data.containers);
    setCategories(data.categories);
    setActiveCategoryId(data.activeCategoryId);
  }, []);
  const reloadPromptData = useCallback(async () => {
    applyPromptData(await storeRef.current.getData());
  }, [applyPromptData]);
  const resetPromptHoverPreview = useCallback(() => {
    setHoverResetKey((key) => key + 1);
  }, []);

  const activeCategory = useMemo(() => (
    categories.find((category) => category.id === activeCategoryId) ?? categories[0] ?? null
  ), [activeCategoryId, categories]);

  const activePrompts = useMemo(() => (
    activeCategory
      ? prompts.filter((prompt) => prompt.categoryId === activeCategory.id)
      : prompts
  ), [activeCategory, prompts]);

  const categoryCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    categories.forEach((category) => {
      counts[category.id] = 0;
    });
    prompts.forEach((prompt) => {
      counts[prompt.categoryId] = (counts[prompt.categoryId] ?? 0) + 1;
    });
    return counts;
  }, [categories, prompts]);

  const getCategoryDisplayName = useCallback((category: PromptCategory) => {
    if (category.id === DEFAULT_CATEGORY_ID && category.name === "Default") {
      return t.manager.defaultCategoryName;
    }
    return category.name;
  }, [t.manager.defaultCategoryName]);

  useLayoutEffect(() => {
    const className = "popover-transparent-page";
    const enabled = windowLabel === "prompt-popover"
      && (mode === "popover" || mode === "button-controls");
    document.documentElement.classList.toggle(className, enabled);
    document.body.classList.toggle(className, enabled);
    return () => {
      document.documentElement.classList.remove(className);
      document.body.classList.remove(className);
    };
  }, [mode, windowLabel]);

  useLayoutEffect(() => {
    if (!pendingPopoverModeRequest || mode !== pendingPopoverModeRequest.mode) return;
    const frame = window.requestAnimationFrame(() => {
      acknowledgePromptPopoverMode(
        pendingPopoverModeRequest.requestId,
        pendingPopoverModeRequest.mode
      ).catch((error) => {
        console.warn("Failed to acknowledge prompt popover mode:", error);
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [mode, pendingPopoverModeRequest]);

  useEffect(() => {
    let active = true;
    const label = currentWindowLabel();
    setWindowLabel(label);
    const loadInitialData = async () => {
      try {
        await reloadPromptData();
      } catch (error) {
        console.warn("Failed to load prompt data:", error);
      }
      const loadedSettings = await settingsStoreRef.current.get();
      if (!active) return;
      applyActiveSettings(loadedSettings);
      setSettingsLoaded(true);
      setMenuLanguage(loadedSettings.language).catch((error) => {
        console.warn("Failed to update menu language:", error);
      });
      if (loadedSettings.promptLibraryLink.mode === "linked") {
        try {
          await reloadPromptData();
        } catch (error) {
          console.warn("Failed to load linked prompt data:", error);
        }
      }
    };
    loadInitialData().catch((error) => {
      console.warn("Failed to initialize app data:", error);
    });

    return () => {
      active = false;
    };
  }, [applyActiveSettings, reloadPromptData]);

  useEffect(() => {
    if (windowLabel !== "main" || mode !== "manager") return;
    if (activeSettings.promptLibraryLink.mode !== "linked") return;
    if (promptLibraryDraftActive) return;

    const intervalId = window.setInterval(() => {
      reloadPromptData().catch((error) => {
        console.warn("Failed to refresh linked prompt library:", error);
      });
    }, LINKED_PROMPT_LIBRARY_REFRESH_MS);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [
    activeSettings.promptLibraryLink.mode,
    activeSettings.promptLibraryLink.path,
    mode,
    promptLibraryDraftActive,
    reloadPromptData,
    windowLabel,
  ]);

  useEffect(() => {
    let active = true;
    const disposers: Array<() => void> = [];

    listen("open-manager-window", () => {
      if (!active || currentWindowLabel() !== "main") return;
      setSettingsReturnTarget(null);
      setMode("manager");
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for manager window requests:", error);
      });

    listen("open-settings-window", () => {
      if (!active || currentWindowLabel() !== "main") return;
      setSettingsReturnTarget(null);
      setMode("settings");
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for settings window requests:", error);
      });

    listen<boolean>("prompt-button-visibility-changed", (event) => {
      if (!active) return;
      const nextSettings = {
        ...activeSettingsRef.current,
        floatingButton: {
          ...activeSettingsRef.current.floatingButton,
          visible: event.payload,
        },
      };
      applyActiveSettings(nextSettings);
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for prompt button visibility:", error);
      });

    return () => {
      active = false;
      disposers.forEach((dispose) => dispose());
    };
  }, [applyActiveSettings]);

  useEffect(() => {
    if (windowLabel !== "prompt-popover") return;

    let active = true;
    const disposers: Array<() => void> = [];

    listen<PendingPopoverModeRequest>("prompt-popover-mode-requested", (event) => {
      if (!active) return;
      const requestedMode = event.payload.mode;
      if (requestedMode !== "popover" && requestedMode !== "button-controls") return;
      if (requestedMode !== "popover") {
        popoverSessionOpenRef.current = false;
      }
      setPendingPopoverModeRequest(event.payload);
      setMode(requestedMode);
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for prompt popover mode requests:", error);
      });

    listen<NativePopoverPointerPosition>("prompt-popover-pointer-position", (event) => {
      if (!active || !popoverSessionOpenRef.current) return;
      setNativePopoverPointer(event.payload);
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for prompt popover pointer position:", error);
      });

    listen<string>("prompt-popover-opened", async (event) => {
      if (!active) return;
      if (event.payload !== "popover") {
        popoverSessionOpenRef.current = false;
        return;
      }
      popoverSessionOpenRef.current = true;
      setNativePopoverPointer(null);
      resetPromptHoverPreview();
      promptListRefreshingRef.current = true;
      try {
        const refreshedSettings = await settingsStoreRef.current.get();
        if (!active) return;
        applyActiveSettings(refreshedSettings);
        await reloadPromptData();
      } finally {
        promptListRefreshingRef.current = false;
      }
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for prompt popover refresh:", error);
      });

    listen("prompt-popover-dismissed", () => {
      if (!active || currentWindowLabel() !== "prompt-popover") return;
      popoverSessionOpenRef.current = false;
      setNativePopoverPointer(null);
      resetPromptHoverPreview();
    })
      .then((unlisten) => {
        if (active) {
          disposers.push(unlisten);
          return;
        }
        unlisten();
      })
      .catch((error) => {
        console.warn("Failed to listen for prompt popover dismissal:", error);
      });

    return () => {
      active = false;
      popoverSessionOpenRef.current = false;
      disposers.forEach((dispose) => dispose());
    };
  }, [applyActiveSettings, reloadPromptData, resetPromptHoverPreview, windowLabel]);

  const handleSelect = async (prompt: PromptContainer) => {
    if (autosendInFlightRef.current || promptListRefreshingRef.current) return;
    autosendInFlightRef.current = true;
    setSubmittingPromptId(prompt.id);
    try {
      await emitAutosendActivity(true);
      await hidePromptPopover();
      await waitForWindowHide();
      setSubmittingPromptId(null);
      const bodies = getPromptContainerBodies(prompt);
      if (bodies.length === 0) {
        emitCalicoMotion("notification", "autosend-empty-prompt", 5200);
        await emitAutosendStatus("failed", t.autosend.genericFailed);
        return;
      }
      const submitKey = submitKeyForMode(activeSettingsRef.current.promptInsertion.mode);
      const status = submitKey === "none"
        ? statusForAutosendOutcome(
          await pastePromptAndSubmitToLastTarget(
            pasteOnlyBody(prompt, bodies),
            submitKey
          ),
          t,
          t.autosend.insertedIntoInput
        )
        : prompt.type === "group"
        ? statusForAutosendSequenceOutcome(
          await pastePromptSequenceAndSubmitToLastTarget(
            bodies,
            prompt.intervalMs,
            submitKey
          ),
          t
        )
        : statusForAutosendOutcome(
          await pastePromptAndSubmitToLastTarget(bodies[0], submitKey),
          t
        );
      if (status.kind === "sent") {
        emitCalicoMotion("happy", "autosend-success", 3000);
      } else {
        emitCalicoMotion(
          "notification",
          status.requiresAttention ? "autosend-action-required" : "autosend-failed",
          5200
        );
      }
      await emitAutosendStatus(status.kind, status.message);
    } catch (e) {
      console.warn("Prompt autosend failed without blocking the picker:", e);
      if (isAccessibilityPermissionError(e)) {
        emitCalicoMotion("notification", "autosend-accessibility-required", 5200);
        await emitAutosendStatus("failed", t.autosend.enableAccessibility);
      } else {
        emitCalicoMotion("notification", "autosend-exception", 5200);
        await emitAutosendStatus("failed", t.autosend.genericFailed);
      }
    } finally {
      autosendInFlightRef.current = false;
      setSubmittingPromptId(null);
      await emitAutosendActivity(false);
    }
  };

  const handleBackToPopover = () => setMode("popover");

  const handleSelectCategory = async (categoryId: string) => {
    setCategoryActionError(null);
    await storeRef.current.setActiveCategoryId(categoryId);
    setActiveCategoryId(categoryId);
  };

  const handleCreateCategory = async (name: string) => {
    setCategoryActionError(null);
    try {
      const category = await storeRef.current.createCategory(name);
      await storeRef.current.setActiveCategoryId(category.id);
      await reloadPromptData();
    } catch (error) {
      console.warn("Failed to create category:", error);
      setCategoryActionError(t.manager.categoryCreateFailed);
    }
  };

  const handleRenameCategory = async (categoryId: string, name: string) => {
    setCategoryActionError(null);
    try {
      await storeRef.current.renameCategory(categoryId, name);
      await reloadPromptData();
    } catch (error) {
      console.warn("Failed to rename category:", error);
      setCategoryActionError(t.manager.categoryRenameFailed);
    }
  };

  const handleDeleteCategory = async (categoryId: string) => {
    setCategoryActionError(null);
    try {
      await storeRef.current.removeCategory(categoryId);
      await reloadPromptData();
    } catch (error) {
      console.warn("Failed to delete category:", error);
      setCategoryActionError(t.manager.categoryDeleteFailed);
    }
  };

  const handleButtonDragEnd = useCallback(
    async (
      position: { x: number; y: number },
      _basePosition: [number, number] | null
    ) => {
      await settingsStoreRef.current.setOverlayButtonPosition(position);
      applyActiveSettings(await settingsStoreRef.current.get());
    },
    [applyActiveSettings]
  );

  const updatePromptInsertionMode = async (mode: PromptInsertionMode) => {
    await settingsStoreRef.current.setPromptInsertionMode(mode);
    applyActiveSettings(await settingsStoreRef.current.get());
  };

  const updateLanguage = async (language: AppLanguage) => {
    await settingsStoreRef.current.setLanguage(language);
    const nextSettings = await settingsStoreRef.current.get();
    applyActiveSettings(nextSettings);
    setMenuLanguage(language).catch((error) => {
      console.warn("Failed to update menu language:", error);
    });
  };

  const openPromptImportChoice = async () => {
    try {
      const file = await open({
        filters: [{ name: "JSON", extensions: ["json"] }],
        multiple: false,
      });
      if (!file || Array.isArray(file)) return;
      const content = await readTextFile(file);
      setPendingPromptImport({ path: file, content, linkAndSync: false });
    } catch (e) {
      console.error("Import failed:", e);
      emitCalicoMotion("notification", "import-prompts-failed", 5200);
      alert(t.manager.importFailed);
    }
  };

  const confirmPromptImport = async () => {
    if (!pendingPromptImport) return;
    try {
      await settingsStoreRef.current.clearPromptLibraryLink();
      applyActiveSettings(await settingsStoreRef.current.get());
      await appDataPromptStoreRef.current.importJson(pendingPromptImport.content);
      if (pendingPromptImport.linkAndSync) {
        const normalizedContent = await appDataPromptStoreRef.current.exportJson();
        const metadata = await writePromptLibraryFile(
          pendingPromptImport.path,
          normalizedContent
        );
        await settingsStoreRef.current.setPromptLibraryLink({
          mode: "linked",
          path: pendingPromptImport.path,
          lastKnownSignature: metadata.signature,
          lastSyncedAt: new Date().toISOString(),
        });
        applyActiveSettings(await settingsStoreRef.current.get());
      }
      setPendingPromptImport(null);
      await reloadPromptData();
      emitCalicoMotion("happy", "import-prompts-success", 3000);
    } catch (e) {
      console.error("Import failed:", e);
      emitCalicoMotion("notification", "import-prompts-failed", 5200);
      alert(t.manager.importFailed);
    }
  };

  const pollingController =
    windowLabel === "main" && settingsLoaded ? (
      <InputTargetPollingController
        settings={activeSettings}
        onButtonDragEnd={handleButtonDragEnd}
      />
    ) : null;

  // ── Manager ─────────────────────────────────────────────────────────
  if (mode === "manager") {
    return (
      <>
        {pollingController}
        <div className="app-window app-window-main app-window-manager">
          <PromptManager
            prompts={activePrompts}
            categories={categories}
            activeCategoryId={activeCategory?.id ?? null}
            categoryCounts={categoryCounts}
            totalPromptCount={prompts.length}
            messages={t}
            onSelectCategory={handleSelectCategory}
            onCreateCategory={handleCreateCategory}
            onRenameCategory={handleRenameCategory}
            onDeleteCategory={handleDeleteCategory}
            getCategoryDisplayName={getCategoryDisplayName}
            categoryActionError={categoryActionError}
            onDraftActivityChange={setPromptLibraryDraftActive}
            onOpenSettings={() => {
              setSettingsReturnTarget("manager");
              setMode("settings");
            }}
            onCreate={async (input) => {
              await storeRef.current.create({ ...input, categoryId: activeCategory?.id });
              await reloadPromptData();
              emitCalicoMotion("happy", "create-prompt-success", 3000);
            }}
            onCreateGroup={async (input) => {
              await storeRef.current.createGroup({ ...input, categoryId: activeCategory?.id });
              await reloadPromptData();
              emitCalicoMotion("happy", "create-group-success", 3000);
            }}
            onCombineSingles={async (input) => {
              const result = await storeRef.current.combineSingles(input);
              applyPromptData(result.snapshot);
              emitCalicoMotion("happy", "combine-prompts-success", 3000);
            }}
            onSplitGroup={async (id) => {
              const result = await storeRef.current.splitGroup(id);
              applyPromptData(result.snapshot);
              emitCalicoMotion("happy", "split-group-success", 3000);
            }}
            onUpdate={async (id, input) => {
              await storeRef.current.update(id, input);
              await reloadPromptData();
              emitCalicoMotion("happy", "update-prompt-success", 3000);
            }}
            onDelete={async (id) => {
              await storeRef.current.remove(id);
              await reloadPromptData();
              emitCalicoMotion("happy", "delete-prompt-success", 2200);
            }}
            onReorder={async (ids) => {
              await storeRef.current.reorder(ids, activeCategory?.id);
              await reloadPromptData();
              emitCalicoMotion("react-poke", "reorder-prompts", 2500);
            }}
            onImport={openPromptImportChoice}
            onExport={async () => {
              try {
                const path = await save({
                  filters: [{ name: "JSON", extensions: ["json"] }],
                  defaultPath: "prompts.json",
                });
                if (path) {
                  const json = await storeRef.current.exportJson();
                  await writeTextFile(path, json);
                  emitCalicoMotion("happy", "export-prompts-success", 3000);
                }
              } catch (e) {
                console.error("Export failed:", e);
                emitCalicoMotion("notification", "export-prompts-failed", 5200);
                alert(t.manager.exportFailed);
              }
            }}
          />
          {pendingPromptImport ? (
            <PromptImportChoiceDialog
              pendingImport={pendingPromptImport}
              messages={t}
              onLinkAndSyncChange={(linkAndSync) => {
                setPendingPromptImport({ ...pendingPromptImport, linkAndSync });
              }}
              onCancel={() => setPendingPromptImport(null)}
              onConfirm={confirmPromptImport}
            />
          ) : null}
          {windowLabel !== "main" ? (
            <div className="page-footer">
              <button className="button button-secondary" onClick={handleBackToPopover}>
                {t.common.back}
              </button>
            </div>
          ) : null}
        </div>
      </>
    );
  }

  // ── Settings ────────────────────────────────────────────────────────
  if (mode === "settings") {
    return (
      <>
        {pollingController}
        <div className="app-window app-window-main">
          <SettingsPanel
            settings={activeSettings}
            onLanguageChange={updateLanguage}
            onPromptInsertionModeChange={updatePromptInsertionMode}
            onBack={settingsReturnTarget === "manager" ? () => {
              setSettingsReturnTarget(null);
              setMode("manager");
            } : undefined}
          />
          {windowLabel !== "main" ? (
            <div className="page-footer">
              <button className="button button-secondary" onClick={handleBackToPopover}>
                {t.common.back}
              </button>
            </div>
          ) : null}
        </div>
      </>
    );
  }

  // ── Button controls mode ────────────────────────────────────────────
  if (mode === "button-controls") {
    return (
      <>
        {pollingController}
        <div className="button-controls">
          <button
            className="button-controls-close"
            onClick={async () => {
              const outcome = await setPromptButtonVisibility(false);
              applyActiveSettings({
                ...activeSettingsRef.current,
                floatingButton: {
                  ...activeSettingsRef.current.floatingButton,
                  visible: outcome.visible,
                },
              });
              if (!outcome.persisted) {
                console.warn("Failed to persist prompt button visibility:", outcome.error);
              }
            }}
          >
            {t.buttonControls.closePet}
          </button>
        </div>
      </>
    );
  }

  // ── Default: popover quick-list ─────────────────────────────────────
  return (
    <div className="popover-root">
      {pollingController}
      <div className="popover-window">
        <PromptQuickList
          prompts={activePrompts}
          categories={categories}
          activeCategoryId={activeCategory?.id ?? null}
          getCategoryDisplayName={getCategoryDisplayName}
          onSelectCategory={handleSelectCategory}
          messages={t.quickList}
          groupMeta={t.manager.groupMeta}
          onSelect={handleSelect}
          submittingPromptId={submittingPromptId}
          hoverResetKey={hoverResetKey}
          nativePointerPosition={nativePopoverPointer}
          onGroupPreview={() => {
            emitCalicoMotion("react-poke", "group-preview", 2500);
          }}
        />
      </div>
    </div>
  );
}

// ── Sub-components ────────────────────────────────────────────────────

interface PromptImportChoiceDialogProps {
  pendingImport: PendingPromptImport;
  messages: Messages;
  onLinkAndSyncChange: (linkAndSync: boolean) => void;
  onCancel: () => void;
  onConfirm: () => void | Promise<void>;
}

function promptFileName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function PromptImportChoiceDialog({
  pendingImport,
  messages,
  onLinkAndSyncChange,
  onCancel,
  onConfirm,
}: PromptImportChoiceDialogProps) {
  return (
    <div className="import-choice-backdrop" role="presentation">
      <section className="import-choice-dialog" role="dialog" aria-modal="true">
        <div>
          <h2>{messages.manager.importPromptLibraryTitle}</h2>
          <p>{promptFileName(pendingImport.path)}</p>
        </div>
        <label className="import-choice-option">
          <input
            type="checkbox"
            aria-label={messages.manager.linkAndSyncThisFile}
            checked={pendingImport.linkAndSync}
            onChange={(event) => onLinkAndSyncChange(event.target.checked)}
          />
          <span>
            <strong>{messages.manager.linkAndSyncThisFile}</strong>
            <small>{messages.manager.linkAndSyncDescription}</small>
          </span>
        </label>
        <div className="import-choice-actions">
          <button className="button button-secondary" type="button" onClick={onCancel}>
            {messages.manager.cancel}
          </button>
          <button className="button button-primary" type="button" onClick={onConfirm}>
            {messages.manager.importPromptLibrary}
          </button>
        </div>
      </section>
    </div>
  );
}
