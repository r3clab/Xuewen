//! Attach a paper's code repository: validate the URL, shallow-clone into
//! the agent workspace as `repo/`, pin the commit, and record the outcome
//! in `paper_code`. Clones are local-only and never redistributed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use sqlx::SqlitePool;
use tokio::process::Command;

/// Public git forges permitted as clone targets out of the box. Self-hosted
/// forges are added via `[ai.agent].clone_allowed_hosts`.
const DEFAULT_ALLOWED_HOSTS: &[&str] =
    &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// Endpoint-level guard. https only, no embedded credentials, and the host must
/// be on an **allowlist** (built-in forges plus `extra_hosts` from config).
///
/// An allowlist — rather than a block-list of internal ranges — is what closes
/// clone SSRF here: the endpoint may be reachable remotely (`--allow-remote`),
/// and `git` resolves DNS itself, so a block-list plus name resolution is
/// bypassable via DNS rebinding. Only letting known hosts through sidesteps that
/// entirely. (Tests hand `run_clone` file:// URLs directly, below this gate.)
pub(crate) fn validate_repo_url(url: &str, extra_hosts: &[String]) -> Result<(), String> {
    let u = url.trim();
    if u.len() > 2000 {
        return Err("the repo URL is implausibly long".into());
    }
    let Some(rest) = u.strip_prefix("https://") else {
        return Err("the repo URL must start with https://".into());
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err("the repo URL has no host".into());
    }
    if authority.contains('@') {
        return Err("the repo URL must not carry credentials".into());
    }
    let host = host_of(authority)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !host_is_allowed(&host, extra_hosts) {
        return Err(format!(
            "the repo host '{host}' is not allowed; add it to [ai.agent].clone_allowed_hosts"
        ));
    }
    Ok(())
}

/// Extract the bare host from a URL authority, dropping any `:port` and the
/// brackets around an IPv6 literal (`[::1]:443` → `::1`).
fn host_of(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    }
}

/// Whether `host` equals, or is a subdomain of, any allowed host (built-in
/// forges plus the configured extras). `host` is assumed already lowercased.
fn host_is_allowed(host: &str, extra_hosts: &[String]) -> bool {
    DEFAULT_ALLOWED_HOSTS
        .iter()
        .map(|s| s.to_string())
        .chain(extra_hosts.iter().map(|s| s.trim().to_ascii_lowercase()))
        .filter(|allowed| !allowed.is_empty())
        .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
}

/// One publish lock per paper: the generation check and the remove+rename in
/// `run_clone`'s publish section are not atomic, and a stale job that passed
/// the check but then stalled (its `remove_dir_all` of a previous multi-hundred-MB
/// checkout can take seconds) could otherwise delete the checkout a newer job
/// published and reported ready. Serialized, the stale job re-reads the bumped
/// generation and takes the superseded path.
static PUBLISH_LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn publish_lock(paper_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    PUBLISH_LOCKS
        .lock()
        .unwrap()
        .entry(paper_id.to_string())
        .or_default()
        .clone()
}

/// Startup reconciliation: a crash/shutdown mid-clone strands the row at
/// 'cloning' forever (a dead process's job resolves nothing) and orphans the
/// generation-scoped staging dir, which no later job's cleanup matches.
/// Being before any clone job in *this* process is not enough to make the
/// sweep safe: a sibling process on the same database and library root — CLI
/// `xuewen code set` clones inline, and the desktop app boots the same stack
/// — may have a clone running right now. Each job therefore holds a lock
/// file naming its pid, and only lockless or dead-pid debris is swept; a
/// live owner keeps both its staging dir and its 'cloning' row.
pub(crate) async fn reconcile_startup(pool: &SqlitePool, library_root: &Path) {
    let live = sweep_staging(library_root).await;
    match super::store::fail_interrupted_clones(pool, &live).await {
        Ok(0) => {}
        Ok(n) => tracing::warn!("{n} interrupted clone(s) marked as error — re-attach to retry"),
        Err(e) => tracing::error!("reconciling interrupted clones: {e}"),
    }
}

/// Remove crash debris (`.repo.cloning.*` staging dirs and their pid locks)
/// under every agent workspace, returning the paper ids whose clone a live
/// process still owns. A live lock protects the whole workspace even before
/// git has created the staging dir — the lock is written first, so that
/// window is real.
async fn sweep_staging(library_root: &Path) -> Vec<String> {
    let mut live = Vec::new();
    let Ok(mut workspaces) = tokio::fs::read_dir(library_root.join("agent")).await else {
        return live; // no agent workspaces yet
    };
    while let Ok(Some(ws)) = workspaces.next_entry().await {
        let Ok(mut entries) = tokio::fs::read_dir(ws.path()).await else {
            continue;
        };
        let (mut dirs, mut locks) = (Vec::new(), Vec::new());
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(".repo.cloning.") {
                continue;
            }
            if name.ends_with(".lock") {
                locks.push(entry.path());
            } else {
                dirs.push(entry.path());
            }
        }
        // Locks first: dead ones are removed here, so the dir pass below can
        // treat "no readable live lock" uniformly as "no owner".
        let mut ws_live = false;
        for lock in locks {
            if lock_owner_alive(&lock).await {
                ws_live = true;
            } else {
                let _ = tokio::fs::remove_file(&lock).await;
            }
        }
        for dir in dirs {
            let mut lock = dir.clone().into_os_string();
            lock.push(".lock");
            if lock_owner_alive(Path::new(&lock)).await {
                continue;
            }
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }
        if ws_live {
            live.push(ws.file_name().to_string_lossy().into_owned());
        }
    }
    live
}

/// Whether `lock` names a pid that is still running. Absent or unparseable
/// content reads as dead — the lock is written whole before git spawns, so
/// garbage means a crashed writer, and sweeping is the safe reading.
async fn lock_owner_alive(lock: &Path) -> bool {
    let Ok(content) = tokio::fs::read_to_string(lock).await else {
        return false;
    };
    content
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|pid| *pid > 0)
        .is_some_and(pid_alive)
}

/// `kill(pid, 0)` probes existence without signaling; EPERM still proves a
/// live process (one owned by another user).
#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_alive(_pid: i32) -> bool {
    false
}

/// Removes `run_clone`'s pid lock on every exit path — early returns, panics,
/// and task cancellation at shutdown all drop the guard. A hard kill leaves
/// the lock behind with a dead pid, which is exactly what `sweep_staging`
/// treats as debris.
struct CloneLockGuard(PathBuf);

impl Drop for CloneLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The clone job body (awaitable directly in tests). Never panics; every
/// outcome lands in `paper_code.status` — unless a later attach (a higher
/// `clone_gen`) has superseded this job, in which case its writes are dropped
/// and it cleans up only its own staging directory, never the live checkout.
pub(crate) async fn run_clone(
    pool: SqlitePool,
    library_root: PathBuf,
    paper_id: String,
    repo_url: String,
    max_repo_mb: u64,
    clone_gen: i64,
) {
    let fail = |e: String| {
        let pool = pool.clone();
        let paper_id = paper_id.clone();
        async move {
            // Ok(false) = a newer attach superseded this job; drop the outcome.
            if let Err(db) =
                super::store::set_paper_code_error(&pool, &paper_id, &e, clone_gen).await
            {
                tracing::error!("paper_code error write failed: {db}");
            }
        }
    };

    let ws = super::workspace_dir(&library_root, &paper_id);
    let dst = ws.join("repo");
    // Clone into a generation-scoped staging dir so concurrent jobs (an old one
    // still running when a re-attach starts) never write the same path, then
    // swap it into place atomically once this job is confirmed current.
    let staging = ws.join(format!(".repo.cloning.{clone_gen}"));
    if let Err(e) = tokio::fs::create_dir_all(&ws).await {
        return fail(format!("could not create the workspace: {e}")).await;
    }
    // Held for the whole job: a concurrently booting sibling process
    // (`reconcile_startup`) swept staging dirs and 'cloning' rows as crash
    // debris until this pid lock taught it to tell a live clone from one.
    let lock_path = ws.join(format!(".repo.cloning.{clone_gen}.lock"));
    if let Err(e) = tokio::fs::write(&lock_path, std::process::id().to_string()).await {
        return fail(format!("could not write the clone lock: {e}")).await;
    }
    let _lock = CloneLockGuard(lock_path);
    let _ = tokio::fs::remove_dir_all(&staging).await; // any remnant of this gen

    let out = Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", &repo_url])
        .arg(&staging)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true)
        .output()
        .await;
    let out = match out {
        Ok(o) => o,
        Err(e) => return fail(format!("could not run git: {e}")).await,
    };
    if !out.status.success() {
        let tail: String = String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(300)
            .collect();
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return fail(format!("git clone failed: {tail}")).await;
    }

    let sha = Command::new("git")
        .args(["-C"])
        .arg(&staging)
        .args(["rev-parse", "--short", "HEAD"])
        .kill_on_drop(true)
        .output()
        .await;
    let sha = match sha {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail("could not resolve the cloned commit".into()).await;
        }
    };

    let join_result = {
        let staging = staging.clone();
        tokio::task::spawn_blocking(move || dir_size(&staging)).await
    };
    let size = match join_result {
        Ok(size) => size,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail(format!(
                "could not measure the cloned repository's size: {e}"
            ))
            .await;
        }
    };
    if size > max_repo_mb.saturating_mul(1024 * 1024) {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return fail(format!(
            "the repository is {} MB, over the {max_repo_mb} MB limit ([ai.agent].max_repo_mb)",
            size / (1024 * 1024)
        ))
        .await;
    }

    // Publish only if still the current generation, with the whole
    // check-remove-rename-record sequence under the per-paper publish lock:
    // the check alone cannot stop a stale job that passed it and then
    // stalled from clobbering the checkout a newer job publishes meanwhile.
    let lock = publish_lock(&paper_id);
    let _publish = lock.lock().await;
    match super::store::current_clone_gen(&pool, &paper_id).await {
        Ok(Some(g)) if g == clone_gen => {}
        Ok(_) => {
            let _ = tokio::fs::remove_dir_all(&staging).await; // superseded
            return;
        }
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail(format!("could not check clone generation: {e}")).await;
        }
    }
    let _ = tokio::fs::remove_dir_all(&dst).await;
    if let Err(e) = tokio::fs::rename(&staging, &dst).await {
        return fail(format!("could not place the checkout: {e}")).await;
    }

    match super::store::set_paper_code_ready(&pool, &paper_id, &sha, size as i64, clone_gen).await {
        Ok(true) => {}
        Ok(false) => {
            // Raced a newer attach or a detach between the generation check and
            // here: our row is gone or superseded. Remove the checkout we just
            // placed so no orphaned `repo/` is left behind for chat to expose.
            // A newer clone will re-place its own before reporting ready.
            let _ = tokio::fs::remove_dir_all(&dst).await;
        }
        Err(e) => tracing::error!("paper_code ready write failed: {e}"),
    }
}

/// Remove a paper's checkout directory (detach keeps paper.txt).
pub(crate) async fn remove_checkout(library_root: &Path, paper_id: &str) {
    let _ =
        tokio::fs::remove_dir_all(super::workspace_dir(library_root, paper_id).join("repo")).await;
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(md) = entry.metadata() {
                if md.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += md.len();
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::store::{self, CodeStatus};

    #[test]
    fn validate_rejects_non_https_and_credentials() {
        let none: &[String] = &[];
        assert!(validate_repo_url("https://github.com/x/y", none).is_ok());
        assert!(validate_repo_url("http://github.com/x/y", none).is_err());
        assert!(validate_repo_url("git@github.com:x/y.git", none).is_err());
        assert!(validate_repo_url("https://user:pw@github.com/x/y", none).is_err());
        assert!(validate_repo_url("file:///etc", none).is_err());
    }

    #[test]
    fn validate_allowlist_admits_forges_and_rejects_the_rest_ssrf() {
        let none: &[String] = &[];
        // Built-in public forges (and their subdomains) pass, port tolerated.
        for u in [
            "https://github.com/x/y",
            "https://gist.github.com/x/y",
            "https://gitlab.com/x/y.git",
            "https://bitbucket.org/x/y",
            "https://codeberg.org/x/y",
            "https://github.com:443/x/y",
        ] {
            assert!(validate_repo_url(u, none).is_ok(), "should allow {u}");
        }
        // Everything off the allowlist is refused — including internal targets
        // an SSRF probe would reach, and arbitrary public hosts.
        for u in [
            "https://localhost/x/y",
            "https://127.0.0.1/x/y",
            "https://169.254.169.254/latest/meta-data", // cloud metadata
            "https://[::1]/x/y",
            "https://gitea/x/y",
            "https://nas.local/x/y",
            "https://evil.example.com/x/y",
            "https://notgithub.com/x/y",
            "https://github.com.evil.com/x/y", // suffix-spoof attempt
        ] {
            assert!(validate_repo_url(u, none).is_err(), "should reject {u}");
        }
        // A configured self-hosted forge (and its subdomains) is admitted.
        let extra = vec!["git.example.com".to_string()];
        assert!(validate_repo_url("https://git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://ci.git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://git.example.com.evil.com/x/y", &extra).is_err());
    }

    /// Happy path against a local repo — git accepts file:// URLs for
    /// --depth clones, so this runs offline. Requires `git` (dev shell).
    #[tokio::test]
    async fn run_clone_pins_commit_and_reports_ready() {
        let src = tempfile::tempdir().unwrap();
        let ok = |st: std::process::ExitStatus| assert!(st.success());
        ok(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(src.path())
            .status()
            .unwrap());
        ok(std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ])
            .current_dir(src.path())
            .status()
            .unwrap());

        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let url = format!("file://{}", src.path().display());
        let gen = store::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            gen,
        )
        .await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Ready, "error: {:?}", c.error);
        assert!(c.commit_sha.is_some());
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        assert!(ws.join("repo/.git").exists());
        assert!(
            !ws.join(format!(".repo.cloning.{gen}.lock")).exists(),
            "the pid lock must not outlive the job"
        );
    }

    #[tokio::test]
    async fn run_clone_reports_failure_as_error_status() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let url = format!("file://{}/nonexistent-repo", std::env::temp_dir().display());
        let gen = store::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            gen,
        )
        .await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Error);
        assert!(c.error.is_some());
        assert!(
            !crate::agent::workspace_dir(lib.path(), "p1")
                .join(format!(".repo.cloning.{gen}.lock"))
                .exists(),
            "the pid lock must not outlive a failed job"
        );
    }

    /// A crash mid-clone leaves status='cloning' and a staging dir behind;
    /// the boot sweep must resolve the row and delete the staging dir while
    /// leaving the published checkout and paper.txt alone.
    #[tokio::test]
    async fn reconcile_startup_fails_stranded_rows_and_sweeps_staging() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::create_dir_all(ws.join(".repo.cloning.0"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(ws.join("repo")).await.unwrap();
        tokio::fs::write(ws.join("paper.txt"), "text")
            .await
            .unwrap();

        reconcile_startup(&pool, lib.path()).await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Error);
        assert!(c.error.unwrap().contains("re-attach"));
        assert!(!ws.join(".repo.cloning.0").exists());
        assert!(ws.join("repo").exists(), "published checkout untouched");
        assert!(ws.join("paper.txt").exists());
    }

    /// A clone running in a sibling process (CLI `xuewen code set`, the
    /// desktop app) holds a live-pid lock; the boot sweep must leave its
    /// staging dir and its 'cloning' row alone.
    #[tokio::test]
    async fn reconcile_startup_skips_a_clone_owned_by_a_live_process() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let gen = store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::create_dir_all(ws.join(format!(".repo.cloning.{gen}")))
            .await
            .unwrap();
        // This test process stands in for the sibling: its pid is live.
        tokio::fs::write(
            ws.join(format!(".repo.cloning.{gen}.lock")),
            std::process::id().to_string(),
        )
        .await
        .unwrap();

        reconcile_startup(&pool, lib.path()).await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning, "live clone must survive");
        assert_eq!(
            store::current_clone_gen(&pool, "p1").await.unwrap(),
            Some(gen)
        );
        assert!(ws.join(format!(".repo.cloning.{gen}")).exists());
        assert!(ws.join(format!(".repo.cloning.{gen}.lock")).exists());
    }

    /// A dead-pid lock and an unparseable one both read as crash debris:
    /// rows failed, dirs and locks swept — and the generation bump means a
    /// job the sweep failed anyway has its late writes dropped.
    #[tokio::test]
    async fn reconcile_startup_sweeps_dead_pid_and_garbage_locks() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        for id in ["p1", "p2"] {
            sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES (?,?,?,datetime('now'),'resolved')")
                .bind(id).bind(format!("hash-{id}")).bind(format!("{id}.pdf"))
                .execute(&pool).await.unwrap();
        }
        let gen1 = store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        let gen2 = store::upsert_paper_code_cloning(&pool, "p2", "https://github.com/x/z")
            .await
            .unwrap();
        let ws1 = crate::agent::workspace_dir(lib.path(), "p1");
        let ws2 = crate::agent::workspace_dir(lib.path(), "p2");
        tokio::fs::create_dir_all(ws1.join(format!(".repo.cloning.{gen1}")))
            .await
            .unwrap();
        // i32::MAX exceeds every platform's pid ceiling: reliably dead.
        tokio::fs::write(
            ws1.join(format!(".repo.cloning.{gen1}.lock")),
            i32::MAX.to_string(),
        )
        .await
        .unwrap();
        tokio::fs::create_dir_all(ws2.join(format!(".repo.cloning.{gen2}")))
            .await
            .unwrap();
        tokio::fs::write(ws2.join(format!(".repo.cloning.{gen2}.lock")), "not a pid")
            .await
            .unwrap();

        reconcile_startup(&pool, lib.path()).await;

        for (id, ws, gen) in [("p1", &ws1, gen1), ("p2", &ws2, gen2)] {
            let c = store::get_paper_code(&pool, id).await.unwrap().unwrap();
            assert_eq!(c.status, CodeStatus::Error, "{id}");
            assert!(!ws.join(format!(".repo.cloning.{gen}")).exists());
            assert!(!ws.join(format!(".repo.cloning.{gen}.lock")).exists());
        }

        // The bump: the dead job's own late failure write misses the guard.
        assert!(
            !store::set_paper_code_error(&pool, "p1", "late write", gen1)
                .await
                .unwrap(),
            "late writes from the failed job must be dropped"
        );
        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert!(c.error.unwrap().contains("re-attach"));
    }

    /// The sweep is idempotent and leaves resolved rows alone.
    #[tokio::test]
    async fn reconcile_startup_leaves_ready_and_error_rows_alone() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let gen = store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        store::set_paper_code_ready(&pool, "p1", "abc1234", 42, gen)
            .await
            .unwrap();

        reconcile_startup(&pool, lib.path()).await;
        reconcile_startup(&pool, lib.path()).await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Ready);
        assert_eq!(c.commit_sha.as_deref(), Some("abc1234"));
    }

    /// A stale clone (lower generation) must not overwrite the row a newer
    /// attach owns, and must not disturb the live checkout.
    #[tokio::test]
    async fn superseded_clone_does_not_overwrite_newer_status() {
        let src = tempfile::tempdir().unwrap();
        let ok = |st: std::process::ExitStatus| assert!(st.success());
        ok(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(src.path())
            .status()
            .unwrap());
        ok(std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ])
            .current_dir(src.path())
            .status()
            .unwrap());
        let url = format!("file://{}", src.path().display());

        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();

        // gen 0 is captured, then a re-attach bumps the row to gen 1 before the
        // stale gen-0 job runs.
        let stale_gen = store::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();
        let current_gen = store::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();
        assert!(current_gen > stale_gen);

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            stale_gen,
        )
        .await;

        // The stale job dropped its outcome: the row is still 'cloning' (gen 1),
        // not 'ready', and no checkout was published.
        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning);
        assert!(!crate::agent::workspace_dir(lib.path(), "p1")
            .join("repo")
            .exists());
    }
}
