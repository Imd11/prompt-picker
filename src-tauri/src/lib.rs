use tauri::Manager;

mod platform;
pub use platform::{
    accessibility_status, frontmost_app, AccessibilityStatus, CandidateInput, FrontmostApp,
};
mod overlay_position;
pub use overlay_position::{prompt_button_position, OverlayPoint};
mod windows;
pub use windows::{
    hide_prompt_button, hide_prompt_popover, move_prompt_button_to, prompt_button_position_cmd,
    show_prompt_button, show_prompt_popover, show_prompt_popover_from_button,
};
mod macos_panels;
pub use macos_panels::configure_non_activating_panel;

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
    platform::macos::current_input_target()
}

#[tauri::command]
fn paste_prompt(body: String) -> Result<(), String> {
    platform::macos::paste_prompt(&body)
}

#[tauri::command]
fn paste_prompt_to_app(body: String, bundle_id: String) -> Result<(), String> {
    platform::macos::paste_prompt_to_app(&body, &bundle_id)
}
#[tauri::command]
fn open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
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
            paste_prompt_to_app,
            show_prompt_button,
            hide_prompt_button,
            show_prompt_popover,
            hide_prompt_popover,
            show_prompt_popover_from_button,
            prompt_button_position_cmd,
            move_prompt_button_to,
            open_main_window
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            window.set_title("Prompt Picker").unwrap();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
