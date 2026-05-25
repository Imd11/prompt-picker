import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PromptItem } from "./shared/promptTypes";
import type { Settings } from "./shared/settingsStore";
import { PromptPopover } from "./ui/PromptPopover";
import { PromptManager } from "./ui/PromptManager";
import { SettingsPanel } from "./ui/SettingsPanel";
import type { AppMode } from "./app/AppMode";
import "./styles.css";

interface AppProps {
  prompts: PromptItem[];
  settings?: Settings;
  onRemoveBlacklist?: (bundleId: string) => void;
}

export function App({ prompts, settings = { version: 1, blacklistedApps: [] }, onRemoveBlacklist }: AppProps) {
  const [mode, setMode] = useState<AppMode>("popover");

  const handleSelect = async (prompt: PromptItem) => {
    try {
      await invoke("paste_prompt", { body: prompt.body });
    } catch (e) {
      console.error("Failed to paste prompt:", e);
    }
  };

  const handleManage = () => {
    setMode("manager");
  };

  const handleBackToPopover = () => {
    setMode("popover");
  };

  if (mode === "manager") {
    return (
      <div className="app-container">
        <PromptManager
          prompts={prompts}
          onCreate={() => {}}
          onUpdate={() => {}}
          onDelete={() => {}}
          onReorder={() => {}}
          onImport={() => {}}
          onExport={() => {}}
        />
        <button className="back-btn" onClick={handleBackToPopover}>← Back</button>
      </div>
    );
  }

  if (mode === "settings") {
    return (
      <div className="app-container">
        <SettingsPanel settings={settings} onRemove={onRemoveBlacklist ?? (() => {})} />
        <button className="back-btn" onClick={handleBackToPopover}>← Back</button>
      </div>
    );
  }

  return (
    <div className="app-container">
      <PromptPopover
        prompts={prompts}
        onSelect={handleSelect}
        onManage={handleManage}
      />
    </div>
  );
}