use tauri::Manager;

mod platform;
pub use platform::{accessibility_status, frontmost_app, AccessibilityStatus, FrontmostApp, CandidateInput};
mod overlay_position;
pub use overlay_position::{prompt_button_position, OverlayPoint};
mod windows;
pub use windows::*;

#[tauri::command]
fn accessibility_status_cmd() -> AccessibilityStatus {
    accessibility_status()
}

#[tauri::command]
fn frontmost_app_cmd() -> Option<FrontmostApp> {
    frontmost_app()
}

#[tauri::command]
fn current_input_target() -> Option<platform::InputTarget> {
    None
}

#[tauri::command]
fn paste_prompt(body: String) -> Result<(), String> {
    platform::macos::paste_prompt(&body)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            accessibility_status_cmd,
            frontmost_app_cmd,
            current_input_target,
            paste_prompt,
            show_prompt_button,
            hide_prompt_button,
            show_prompt_popover,
            hide_prompt_popover
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            window.set_title("Prompt Picker").unwrap();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}