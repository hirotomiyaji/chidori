//! Moves what Kikitori left behind into Chidori's folders. Chidori was called Kikitori before its rename,
//! and the app's folders are named after its identifier, so without this an update would start
//! with no settings, no models and no recordings in view.

use std::path::Path;

/// The app's identifier before the rename.
pub const OLD_IDENTIFIER: &str = "dev.yusai.kikitori";
/// The folder under Documents that recordings went to by default before the rename.
pub const OLD_OUTPUT_FOLDER: &str = "Kikitori";

/// Moves `%APPDATA%\dev.yusai.kikitori` (settings, state, window position) and
/// `%LOCALAPPDATA%\dev.yusai.kikitori` (models, logs, the webview's storage) to the new identifier.
/// Runs before anything opens a file there. Returns what happened, to log once logging is up.
#[cfg(windows)]
pub fn app_folders() -> Vec<String> {
    use crate::host::IDENTIFIER;
    use std::path::PathBuf;
    let mut report = Vec::new();
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
            move_entries(&base.join(OLD_IDENTIFIER), &base.join(IDENTIFIER), &mut report);
        }
    }
    report
}

/// Kikitori only ever shipped for Windows: nothing to move elsewhere.
#[cfg(not(windows))]
pub fn app_folders() -> Vec<String> {
    Vec::new()
}

/// Moves each entry of `old` into `new` unless `new` already has one by that name, then removes
/// `old` if it is empty. Entry by entry rather than one rename, so a file still locked by the old
/// app's webview a moment after it quit costs only that file. Models are gigabytes, so this only
/// ever renames on the same volume and never copies.
#[cfg_attr(not(windows), allow(dead_code))]
fn move_entries(old: &Path, new: &Path, report: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(old) else { return };
    if let Err(e) = std::fs::create_dir_all(new) {
        report.push(format!("migrate: can't create {}: {e}", new.display()));
        return;
    }
    for entry in entries.flatten() {
        let to = new.join(entry.file_name());
        if to.exists() {
            continue;
        }
        match std::fs::rename(entry.path(), &to) {
            Ok(()) => report.push(format!("migrate: moved {} to {}", entry.path().display(), to.display())),
            Err(e) => report.push(format!("migrate: can't move {}: {e}", entry.path().display())),
        }
    }
    let _ = std::fs::remove_dir(old);
}

/// Where recordings go now that the old default folder is gone: renames `Documents\Kikitori` to
/// the new default when `root` still points at it. Returns the root to use, which stays the old
/// folder if it can't be renamed (open in Explorer, or the new one already exists).
pub fn output_root(root: &str, old_default: &Path, new_default: &Path) -> Option<String> {
    if Path::new(root) != old_default {
        return None;
    }
    if !old_default.exists() {
        return Some(new_default.to_string_lossy().into_owned());
    }
    if new_default.exists() {
        tracing::warn!(
            "migrate: both {} and {} exist; keeping the first",
            old_default.display(),
            new_default.display()
        );
        return None;
    }
    match std::fs::rename(old_default, new_default) {
        Ok(()) => {
            tracing::info!("migrate: moved {} to {}", old_default.display(), new_default.display());
            Some(new_default.to_string_lossy().into_owned())
        }
        Err(e) => {
            tracing::warn!("migrate: can't move {}: {e}", old_default.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_entries_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("old"), dir.path().join("new"));
        std::fs::create_dir_all(old.join("models")).unwrap();
        std::fs::write(old.join("models").join("m.bin"), "model").unwrap();
        std::fs::write(old.join("settings.json"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("settings.json"), "new").unwrap();

        let mut report = Vec::new();
        move_entries(&old, &new, &mut report);

        assert_eq!(std::fs::read_to_string(new.join("models").join("m.bin")).unwrap(), "model");
        assert_eq!(std::fs::read_to_string(new.join("settings.json")).unwrap(), "new");
        // settings.json stayed behind, so the old folder is not empty and stays.
        assert!(old.join("settings.json").exists());
    }

    #[test]
    fn missing_old_folder_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = Vec::new();
        move_entries(&dir.path().join("none"), &dir.path().join("new"), &mut report);
        assert!(report.is_empty());
        assert!(!dir.path().join("new").exists());
    }

    #[test]
    fn output_root_follows_the_rename() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("Kikitori"), dir.path().join("Chidori"));
        let custom = dir.path().join("Elsewhere");
        assert_eq!(output_root(&custom.to_string_lossy(), &old, &new), None);
        // No old folder: just point at the new default.
        assert_eq!(output_root(&old.to_string_lossy(), &old, &new), Some(new.to_string_lossy().into_owned()));

        std::fs::create_dir_all(old.join("2026-10-03_1513_Zoom")).unwrap();
        assert_eq!(output_root(&old.to_string_lossy(), &old, &new), Some(new.to_string_lossy().into_owned()));
        assert!(new.join("2026-10-03_1513_Zoom").exists());
        assert!(!old.exists());

        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(output_root(&old.to_string_lossy(), &old, &new), None);
    }
}
