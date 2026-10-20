use anyhow::{anyhow, Result};
use std::path::Path;

/// Move `src` into `dir` under its basename, uniquifying on collision
/// (`name-1.pdf`, `name-2.pdf`, …): `_processed`/`_failed` are archives, and
/// `rename` replaces silently on Unix — a later same-named inbox drop must
/// not destroy the bytes an earlier one archived (for `SameWork` those bytes
/// exist nowhere else). Falls back to copy+remove across filesystems.
fn move_to(src: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = src
        .file_name()
        .ok_or_else(|| anyhow!("path has no file name"))?;
    if is_same_file(&dir.join(name), src) {
        // Already in the archive (a dedup re-ingest from `_processed`
        // itself), under whatever spelling the caller used: HEAD's plain
        // rename was a POSIX no-op on two names for the same file, and the
        // uniquify loop below would instead shuffle the archived file to a
        // `-1` suffix on every re-ingest.
        return Ok(());
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = Path::new(name).extension().and_then(|s| s.to_str());
    // Reserve a free name with an exclusive create so the probe itself cannot
    // be clobbered by a concurrent mover; renaming over our own reservation
    // (or truncating it in the copy fallback) is then safe.
    let mut dest = dir.join(name);
    for n in 1.. {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                dest = dir.join(match ext {
                    Some(ext) => format!("{stem}-{n}.{ext}"),
                    None => format!("{stem}-{n}"),
                });
            }
            Err(e) => return Err(e.into()),
        }
    }
    if std::fs::rename(src, &dest).is_err() {
        if let Err(e) = std::fs::copy(src, &dest) {
            // The reservation is ours alone, and empty: left behind it would
            // wear the PDF's name in the archive (the watcher's quarantine
            // signal is bare existence) and push later same-named archives to
            // suffixes. A successful copy whose trailing source removal fails
            // is different — dest then holds the full bytes and stays.
            let _ = std::fs::remove_file(&dest);
            return Err(e.into());
        }
        std::fs::remove_file(src)?;
    }
    Ok(())
}

/// Whether two paths name the same file, by (dev, ino) — path spellings
/// (relative vs absolute, `./` prefixes, symlinked components) don't
/// normalize under `PathBuf` equality. Any metadata error reads as "not the
/// same file": a vanished `src` must fall through to the move's own error
/// path rather than fail here.
fn is_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
    }
    false
}

/// Whether the file at `path` hashes to `content_hash`; any read failure is
/// "no".
pub(crate) async fn file_hash_matches(path: &Path, content_hash: &str) -> bool {
    let p = path.to_path_buf();
    matches!(
        tokio::task::spawn_blocking(move || crate::hash::sha256_file(&p)).await,
        Ok(Ok(h)) if h == content_hash
    )
}

/// Copy `from` to the exact path `to`, creating parent directories.
fn copy_to(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// The exclusive create found the destination already present. A dedicated
/// type rather than matching `io::ErrorKind::AlreadyExists` anywhere in the
/// chain: `create_dir_all` also fails with `AlreadyExists` when `library_root`
/// exists as a regular file, and reading that as a cite-key collision would
/// send the filing loop through an endless suffix walk instead of surfacing
/// the misconfiguration.
#[derive(Debug)]
struct DestExists;

impl std::fmt::Display for DestExists {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("destination file already exists")
    }
}

impl std::error::Error for DestExists {}

/// `copy_to`, but refusing to replace an existing file (`create_new`):
/// cite-key destinations are chosen by a racy read-compute-write, and
/// `std::fs::copy` would silently truncate the concurrent winner's file.
/// A destination this call created but could not fill is removed before the
/// error propagates — a stranded partial file would otherwise hold the path
/// against every retry (and steal the cite key for good).
fn copy_to_excl(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut dest = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(anyhow::Error::new(DestExists));
        }
        Err(e) => return Err(e.into()),
    };
    let copied = std::fs::File::open(from)
        .and_then(|mut src| std::io::copy(&mut src, &mut dest))
        .map(|_| ());
    if copied.is_err() {
        // Only ever our own fresh file: a loser of the create_new race
        // returns above without having created anything.
        let _ = std::fs::remove_file(to);
    }
    Ok(copied?)
}

/// Whether `e` is `copy_to_excl` reporting an occupied destination (a lost
/// `create_new` race). Any other failure — including `AlreadyExists` from
/// `create_dir_all` — is a real error, not a collision.
pub(crate) fn is_already_exists(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<DestExists>().is_some())
}

/// `move_to` off the async runtime.
pub(crate) async fn move_to_async(src: &Path, dir: &Path) -> Result<()> {
    let (src, dir) = (src.to_path_buf(), dir.to_path_buf());
    tokio::task::spawn_blocking(move || move_to(&src, &dir)).await?
}

/// `copy_to` off the async runtime.
pub(crate) async fn copy_to_async(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_to(&from, &to)).await?
}

/// `copy_to_excl` off the async runtime.
pub(crate) async fn copy_to_excl_async(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_to_excl(&from, &to)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_to_uniquifies_archive_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_processed");
        let a = dir.path().join("paper.pdf");
        let b = dir.path().join("sub");
        std::fs::create_dir_all(&b).unwrap();
        let b = b.join("paper.pdf");
        std::fs::write(&a, b"first bytes").unwrap();
        std::fs::write(&b, b"second bytes").unwrap();

        move_to(&a, &archive).unwrap();
        move_to(&b, &archive).unwrap();

        // The earlier archive entry survives; the later drop gets a suffix.
        assert_eq!(
            std::fs::read(archive.join("paper.pdf")).unwrap(),
            b"first bytes"
        );
        assert_eq!(
            std::fs::read(archive.join("paper-1.pdf")).unwrap(),
            b"second bytes"
        );

        // Re-archiving a file already at its archive path is a no-op.
        move_to(&archive.join("paper.pdf"), &archive).unwrap();
        assert_eq!(
            std::fs::read(archive.join("paper.pdf")).unwrap(),
            b"first bytes"
        );
        assert!(!archive.join("paper-2.pdf").exists());
    }

    #[test]
    fn move_to_cleans_its_reservation_when_the_move_fails() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_failed");
        // Source gone (concurrent mover, or deleted mid-quarantine): both the
        // rename and the copy fallback fail.
        assert!(move_to(&dir.path().join("bad.pdf"), &archive).is_err());
        assert!(
            !archive.join("bad.pdf").exists(),
            "no empty reservation may impersonate a quarantined PDF"
        );

        // The name is still free for a real quarantine.
        let src = dir.path().join("bad.pdf");
        std::fs::write(&src, b"real bytes").unwrap();
        move_to(&src, &archive).unwrap();
        assert_eq!(
            std::fs::read(archive.join("bad.pdf")).unwrap(),
            b"real bytes"
        );
    }

    #[test]
    fn move_to_no_ops_on_a_differently_spelled_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_processed");
        let src = dir.path().join("paper.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        move_to(&src, &archive).unwrap();

        // Re-ingest via a spelling PathBuf equality can't see through.
        let respelled = archive.join("..").join("_processed").join("paper.pdf");
        assert_ne!(respelled, archive.join("paper.pdf"));
        move_to(&respelled, &archive).unwrap();
        assert_eq!(std::fs::read(archive.join("paper.pdf")).unwrap(), b"bytes");
        assert!(
            !archive.join("paper-1.pdf").exists(),
            "the archived file must not be shuffled to a suffix"
        );
    }

    #[test]
    fn copy_to_excl_removes_its_partial_destination_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("lib/key.pdf");
        let err = copy_to_excl(&dir.path().join("missing.pdf"), &dest).unwrap_err();
        assert!(!is_already_exists(&err));
        assert!(
            !dest.exists(),
            "a failed copy must not strand a partial destination"
        );

        // The key is still free: a retry files under the canonical name
        // instead of ceding it to garbage and taking a suffix.
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        copy_to_excl(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"bytes");
    }

    #[test]
    fn a_file_squatting_the_destination_parent_is_not_a_key_collision() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        std::fs::write(dir.path().join("library"), b"not a directory").unwrap();
        let err = copy_to_excl(&src, &dir.path().join("library").join("key.pdf")).unwrap_err();
        assert!(
            !is_already_exists(&err),
            "create_dir_all's AlreadyExists must not read as a cite-key \
             collision — the filing loop would walk suffixes forever"
        );
    }

    #[test]
    fn copy_to_excl_refuses_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        let dest = dir.path().join("lib/key.pdf");
        std::fs::write(&src, b"new").unwrap();
        copy_to_excl(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");

        std::fs::write(&src, b"other work, same key").unwrap();
        let err = copy_to_excl(&src, &dest).unwrap_err();
        assert!(is_already_exists(&err));
        assert_eq!(std::fs::read(&dest).unwrap(), b"new", "winner untouched");
    }
}
