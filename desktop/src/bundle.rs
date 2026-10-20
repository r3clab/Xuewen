use std::path::{Path, PathBuf};

// Locate the .app bundle's directories from the running executable's path.
// Sidecar binaries (`pdftotext`, `node`) live in Contents/MacOS next to the
// app binary; resources (agent-runner, relocated dylibs) in
// Contents/Resources.

/// Pure path logic: `Some((Contents/MacOS, Contents/Resources))` when `exe`
/// looks like a macOS bundle executable.
pub fn bundle_dirs_from_exe(exe: &Path) -> Option<(PathBuf, PathBuf)> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    Some((macos.to_path_buf(), contents.join("Resources")))
}

/// Bundle dirs of the running process, `None` when unbundled (`cargo run`,
/// `tauri dev`) or when the Resources directory is missing.
pub fn bundle_dirs() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let (macos, resources) = bundle_dirs_from_exe(&exe)?;
    resources.is_dir().then_some((macos, resources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_bundled_exe_path() {
        let (macos, resources) = bundle_dirs_from_exe(Path::new(
            "/Applications/Xuewen.app/Contents/MacOS/xuewen-desktop",
        ))
        .unwrap();
        assert_eq!(macos, Path::new("/Applications/Xuewen.app/Contents/MacOS"));
        assert_eq!(
            resources,
            Path::new("/Applications/Xuewen.app/Contents/Resources")
        );
    }

    #[test]
    fn rejects_an_unbundled_exe_path() {
        assert!(bundle_dirs_from_exe(Path::new("/repo/target/debug/xuewen-desktop")).is_none());
    }
}
