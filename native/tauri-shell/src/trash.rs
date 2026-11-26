use std::path::Path;

/// Moves a local file or folder to the OS Trash / Recycle Bin; orcad itself can only delete permanently.
#[tauri::command]
pub fn move_to_trash(path: String) -> Result<(), String> {
    let path = Path::new(&path);
    if !path.is_absolute() {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    // Concurrent external deletion keeps UI deletion idempotent, as on Electron.
    if path.symlink_metadata().is_err() {
        return Ok(());
    }
    trash::delete(path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::move_to_trash;

    #[test]
    fn refuses_relative_paths_and_tolerates_vanished_ones() {
        assert!(move_to_trash("relative/file.txt".into()).is_err());
        let gone = std::env::temp_dir().join("orca-tauri-trash-test-never-created");
        assert!(move_to_trash(gone.to_string_lossy().into_owned()).is_ok());
    }
}
