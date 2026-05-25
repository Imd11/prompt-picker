#![cfg(target_os = "macos")]

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct FrontmostApp {
    pub name: String,
    pub bundle_id: String,
}

#[derive(Debug, Serialize)]
pub struct AccessibilityStatus {
    pub trusted: bool,
}

pub fn accessibility_status() -> AccessibilityStatus {
    AccessibilityStatus { trusted: false }
}

pub fn frontmost_app() -> Option<FrontmostApp> {
    None
}

pub fn paste_prompt(body: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Command;

    // Write to clipboard via pbcopy
    let mut child = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }

    child.wait().map_err(|e| e.to_string())?;

    // Simulate Cmd+V using osascript
    Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to keystroke \"v\" using command down"])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(())
}