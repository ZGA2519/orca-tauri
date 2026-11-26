fn main() {
    // App commands need generated permissions so the runtime capability can grant them to the orcad origin.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["move_to_trash"])),
    )
    .expect("failed to run tauri-build");
}
