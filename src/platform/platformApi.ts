import { invoke } from "@tauri-apps/api/core";
import type { AppLanguage } from "../shared/settingsStore";

export interface FrontmostApp {
  name: string;
  bundle_id: string;
}

export interface AccessibilityStatus {
  trusted: boolean;
}

export interface PromptButtonPosition {
  x: number;
  y: number;
}

export interface PromptButtonVisibilityOutcome {
  visible: boolean;
  applied: boolean;
  persisted: boolean;
  error: string | null;
}

export interface PromptLibraryFile {
  content: string;
  signature: string;
}

export interface PromptLibraryFileMetadata {
  signature: string;
}

export type AutosendFailureReason =
  | "copy_failed"
  | "missing_accessibility_permission"
  | "no_safe_target"
  | "paste_event_failed"
  | "return_event_failed"
  | "target_focus_failed"
  | "target_changed"
  | "composer_not_found"
  | "composer_ambiguous"
  | "focus_not_acquired"
  | "paste_not_confirmed";

export type NativeSubmitKey = "none" | "enter" | "command_enter";
export type AutosendCompletion = "pasted_only" | "submitted";

export interface AutosendOutcome {
  copied: boolean;
  sent: boolean;
  completion?: AutosendCompletion | null;
  error: string | null;
  reason: AutosendFailureReason | null;
}

export interface AutosendSequenceOutcome extends AutosendOutcome {
  sent_count: number;
  processed_count?: number;
  failed_index: number | null;
}

export async function getAccessibilityStatus(): Promise<AccessibilityStatus> {
  return invoke<AccessibilityStatus>("accessibility_status_cmd");
}

export async function requestAccessibilityPermission(): Promise<AccessibilityStatus> {
  return invoke<AccessibilityStatus>("request_accessibility_permission_cmd");
}

export async function openAccessibilitySettings(): Promise<void> {
  return invoke("open_accessibility_settings");
}

export async function getFrontmostApp(): Promise<FrontmostApp | null> {
  return invoke<FrontmostApp | null>("frontmost_app_cmd");
}

export async function pastePrompt(body: string): Promise<void> {
  return invoke("paste_prompt", { body });
}

export async function pastePromptAndSubmitToLastTarget(
  body: string,
  submitKey: NativeSubmitKey = "enter",
  containerId?: string
): Promise<AutosendOutcome> {
  return invoke<AutosendOutcome>("paste_prompt_and_submit_to_last_target", {
    body,
    submitKey,
    containerId,
  });
}

export async function pastePromptSequenceAndSubmitToLastTarget(
  bodies: string[],
  intervalMs: number,
  submitKey: NativeSubmitKey = "enter",
  containerId?: string
): Promise<AutosendSequenceOutcome> {
  return invoke<AutosendSequenceOutcome>(
    "paste_prompt_sequence_and_submit_to_last_target",
    { bodies, intervalMs, submitKey, containerId }
  );
}

export async function getCurrentInputTarget(): Promise<unknown> {
  return invoke("current_input_target");
}

export async function showPromptButton(x: number, y: number): Promise<void> {
  return invoke("show_prompt_button", { x, y });
}

export async function hidePromptButton(): Promise<void> {
  return invoke("hide_prompt_button");
}

export async function setPromptButtonVisibility(
  visible: boolean
): Promise<PromptButtonVisibilityOutcome> {
  return invoke<PromptButtonVisibilityOutcome>("set_prompt_button_visibility", { visible });
}

export async function showPromptPopover(x: number, y: number): Promise<void> {
  return invoke("show_prompt_popover", { x, y });
}

export async function hidePromptPopover(): Promise<void> {
  return invoke("hide_prompt_popover");
}

export async function getPromptButtonPosition(): Promise<PromptButtonPosition | null> {
  return invoke<PromptButtonPosition | null>("prompt_button_position_cmd");
}

export async function movePromptButtonTo(x: number, y: number): Promise<void> {
  return invoke("move_prompt_button_to", { x, y });
}

export async function showPromptPopoverFromButton(sessionId: number): Promise<void> {
  return invoke("show_prompt_popover_from_button", { sessionId });
}

export async function acknowledgePromptPopoverMode(
  requestId: number,
  mode: "popover" | "button-controls"
): Promise<void> {
  return invoke("acknowledge_prompt_popover_mode", { requestId, mode });
}

export async function openMainWindow(): Promise<void> {
  return invoke("open_main_window");
}

export async function setMenuLanguage(language: AppLanguage): Promise<void> {
  return invoke("set_menu_language", { language });
}

export async function quitPromptPicker(): Promise<void> {
  return invoke("quit_prompt_picker");
}

export async function readPromptLibraryFile(path: string): Promise<PromptLibraryFile> {
  return invoke<PromptLibraryFile>("read_prompt_library_file", { path });
}

export async function writePromptLibraryFile(
  path: string,
  content: string
): Promise<PromptLibraryFileMetadata> {
  return invoke<PromptLibraryFileMetadata>("write_prompt_library_file", { path, content });
}

export async function getPromptLibraryFileMetadata(
  path: string
): Promise<PromptLibraryFileMetadata> {
  return invoke<PromptLibraryFileMetadata>("prompt_library_file_metadata", { path });
}
