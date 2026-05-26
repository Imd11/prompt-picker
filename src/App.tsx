import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import type { PromptItem } from "./shared/promptTypes";
import type { Settings } from "./shared/settingsStore";
import { createPromptStore } from "./shared/promptStore";
import { createTauriPromptStorage } from "./storage/tauriPromptStorage";
import { PromptPopover } from "./ui/PromptPopover";
import { PromptManager } from "./ui/PromptManager";
import { SettingsPanel } from "./ui/SettingsPanel";
import type { AppMode } from "./app/AppMode";
import "./styles.css";

interface AppProps {
  settings?: Settings;
  onRemoveBlacklist?: (bundleId: string) => void;
}

export function App({ settings = { version: 1, blacklistedApps: [] }, onRemoveBlacklist }: AppProps) {
  const [mode, setMode] = useState<AppMode>("popover");
  const [prompts, setPrompts] = useState<PromptItem[]>([]);
  const storeRef = useRef(createPromptStore(createTauriPromptStorage()));

  useEffect(() => {
    storeRef.current.list().then(setPrompts);
  }, []);

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
          onCreate={async (input) => {
            await storeRef.current.create(input);
            setPrompts(await storeRef.current.list());
          }}
          onUpdate={async (id, input) => {
            await storeRef.current.update(id, input);
            setPrompts(await storeRef.current.list());
          }}
          onDelete={async (id) => {
            await storeRef.current.remove(id);
            setPrompts(await storeRef.current.list());
          }}
          onReorder={async (ids) => {
            await storeRef.current.reorder(ids);
            setPrompts(await storeRef.current.list());
          }}
          onImport={async () => {
            const file = await open({
              filters: [{ name: "JSON", extensions: ["json"] }],
              multiple: false,
            });
            if (file) {
              const content = await readTextFile(file as string);
              await storeRef.current.importJson(content);
              setPrompts(await storeRef.current.list());
            }
          }}
          onExport={async () => {
            const path = await save({
              filters: [{ name: "JSON", extensions: ["json"] }],
              defaultPath: "prompts.json",
            });
            if (path) {
              const json = await storeRef.current.exportJson();
              await writeTextFile(path, json);
            }
          }}
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