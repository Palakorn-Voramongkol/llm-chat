fn main() {
    // Only the GUI build embeds the webview assets/config via tauri-build. The
    // headless build (--no-default-features) has no tauri-build dependency.
    #[cfg(feature = "gui")]
    tauri_build::build();
}
