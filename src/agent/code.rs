//! Attach a paper's code repository: validate the URL, shallow-clone into
//! the agent workspace as `repo/`, pin the commit, and record the outcome
//! in `paper_code`. Clones are local-only and never redistributed.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reqwest::Url;
use sqlx::SqlitePool;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::config::AgentConfig;

/// Public git forges permitted as clone targets out of the box. Self-hosted
/// forges are added via `[ai.agent].clone_allowed_hosts`.
const DEFAULT_ALLOWED_HOSTS: &[&str] =
    &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// How often a running clone's staging dir is measured against the budget.
const SIZE_POLL: Duration = Duration::from_secs(1);

/// Bytes of git's stderr kept for the error message.
const STDERR_CAP: u64 = 4096;

/// Prefix of every per-generation staging dir and of its pid lock.
const STAGING_PREFIX: &str = ".repo.cloning.";

const PUBLISH_LOCK: &str = ".repo.publish.lock";

/// Endpoint-level guard, returning the URL `git` is handed. https only, no
/// embedded credentials, query or fragment, and the host must be on an
/// **allowlist** (built-in forges plus `extra_hosts` from config).
///
/// An allowlist — rather than a block-list of internal ranges — is what closes
/// clone SSRF here: the endpoint may be reachable remotely (`--allow-remote`),
/// and `git` resolves DNS itself, so a block-list plus name resolution is
/// bypassable via DNS rebinding. `git` receives the WHATWG re-serialization
/// of the parsed URL, never the raw input, so the host checked here is the
/// host it connects to. (Tests hand `run_clone` file:// URLs directly, below
/// this gate.)
pub(crate) fn validate_repo_url(raw: &str, extra_hosts: &[String]) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.len() > 2000 {
        return Err("the repo URL is implausibly long".into());
    }
    let url = Url::parse(raw).map_err(|e| format!("the repo URL must be an https:// URL ({e})"))?;
    if url.scheme() != "https" {
        return Err("the repo URL must start with https://".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the repo URL must not carry credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("the repo URL must not carry a query or fragment".into());
    }
    let Some(host) = url.host_str() else {
        return Err("the repo URL has no host".into());
    };
    if !host_is_allowed(&url, extra_hosts) {
        return Err(format!(
            "the repo host '{host}' is not allowed; add it to [ai.agent].clone_allowed_hosts"
        ));
    }
    Ok(url)
}

/// Whether `url`'s host is on the allowlist (built-in forges plus the
/// configured extras, normalized by the same parser). A domain also matches
/// as a subdomain of an entry, on a label boundary; an IP literal matches
/// only an identical entry.
fn host_is_allowed(url: &Url, extra_hosts: &[String]) -> bool {
    let mut entries = DEFAULT_ALLOWED_HOSTS
        .iter()
        .map(|s| s.to_string())
        .chain(extra_hosts.iter().filter_map(|h| normalized_host(h)));
    match (url.domain(), url.host_str()) {
        (Some(domain), _) => {
            let domain = domain.trim_end_matches('.');
            entries.any(|entry| {
                domain == entry
                    || domain
                        .strip_suffix(entry.as_str())
                        .is_some_and(|sub| sub.ends_with('.'))
            })
        }
        (None, Some(literal)) => entries.any(|entry| entry == literal),
        (None, None) => false,
    }
}

/// A configured host as the URL parser spells it: lowercased, IDNA-encoded,
/// IPv6 in brackets, no trailing dot.
fn normalized_host(entry: &str) -> Option<String> {
    let url = Url::parse(&format!("https://{}/", entry.trim())).ok()?;
    Some(url.host_str()?.trim_end_matches('.').to_string())
}

/// The budget a clone runs under.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CloneLimits {
    pub(crate) max_repo_mb: u64,
    pub(crate) timeout: Duration,
}

impl CloneLimits {
    pub(crate) fn from_config(cfg: &AgentConfig) -> Self {
        Self {
            max_repo_mb: cfg.max_repo_mb,
            timeout: Duration::from_secs(cfg.clone_timeout_secs),
        }
    }

    fn max_bytes(&self) -> u64 {
        self.max_repo_mb.saturating_mul(1024 * 1024)
    }

    fn oversize(&self) -> String {
        format!(
            "the repository is over the {} MB limit ([ai.agent].max_repo_mb)",
            self.max_repo_mb
        )
    }
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

/// Remove crash debris (staging dirs and their pid locks) under every agent
/// workspace, returning the paper ids whose clone a live process still owns.
/// A live lock protects the whole workspace even before git has created the
/// staging dir — the lock is written first, so that window is real.
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
            if !name.starts_with(STAGING_PREFIX) {
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

/// One generation's staging dir plus the pid lock that marks it live to a
/// sibling process's `reconcile_startup`. Dropping removes the lock on every
/// exit path — panics and task cancellation at shutdown included. A hard
/// kill leaves the lock behind with a dead pid, which is exactly what
/// `sweep_staging` treats as debris.
struct Staging {
    dir: PathBuf,
    lock: PathBuf,
}

impl Staging {
    /// Clone into a generation-scoped dir so concurrent jobs (an old one still
    /// running when a re-attach starts) never write the same path.
    async fn claim(ws: &Path, clone_gen: i64) -> Result<Self, String> {
        tokio::fs::create_dir_all(ws)
            .await
            .map_err(|e| format!("could not create the workspace: {e}"))?;
        let lock = ws.join(format!("{STAGING_PREFIX}{clone_gen}.lock"));
        tokio::fs::write(&lock, std::process::id().to_string())
            .await
            .map_err(|e| format!("could not write the clone lock: {e}"))?;
        let staging = Self {
            dir: ws.join(format!("{STAGING_PREFIX}{clone_gen}")),
            lock,
        };
        remove_dir(&staging.dir)
            .await
            .map_err(|e| format!("could not clear a previous staging dir: {e}"))?;
        Ok(staging)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock);
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
    url: Url,
    limits: CloneLimits,
    clone_gen: i64,
) {
    let ws = super::workspace_dir(&library_root, &paper_id);
    let Err(e) = clone_job(&pool, &ws, &paper_id, &url, limits, clone_gen).await else {
        return;
    };
    // Ok(false) = a newer attach superseded this job; drop the outcome.
    if let Err(db) = super::store::set_paper_code_error(&pool, &paper_id, &e, clone_gen).await {
        tracing::error!("paper_code error write failed: {db}");
    }
}

async fn clone_job(
    pool: &SqlitePool,
    ws: &Path,
    paper_id: &str,
    url: &Url,
    limits: CloneLimits,
    clone_gen: i64,
) -> Result<(), String> {
    let staging = Staging::claim(ws, clone_gen).await?;
    let result = async {
        git_clone(url, &staging.dir, limits).await?;
        let sha = head_commit(&staging.dir).await?;
        let size = measure(&staging.dir).await?;
        if size > limits.max_bytes() {
            return Err(limits.oversize());
        }
        publish(pool, ws, &staging.dir, paper_id, &sha, size, clone_gen).await
    }
    .await;
    // Gone already once published; a failed or superseded job's leftovers.
    let _ = remove_dir(&staging.dir).await;
    result
}

/// Shallow-clone `url` into `dst` within `limits`. git runs in its own
/// process group because the transfer happens in its children
/// (`git-remote-https`, `index-pack`), which killing git alone would leave
/// running; the whole group is killed once the deadline passes or `dst`
/// outgrows the budget.
async fn git_clone(url: &Url, dst: &Path, limits: CloneLimits) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.args([
        "clone",
        "--quiet",
        "--depth",
        "1",
        "--single-branch",
        "--no-tags",
        "--",
        url.as_str(),
    ])
    .arg(dst)
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("could not run git: {e}"))?;
    // Declared after `child` so it drops first: killpg then runs while the
    // child is still unreaped, so its pgid cannot be recycled.
    let mut group = super::GroupKillGuard::new(child.id());
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr = tokio::spawn(read_head(stderr, STDERR_CAP));
    let deadline = tokio::time::sleep(limits.timeout);
    tokio::pin!(deadline);
    let mut poll = tokio::time::interval(SIZE_POLL);
    let status = loop {
        tokio::select! {
            status = child.wait() => break status.map_err(|e| format!("could not run git: {e}"))?,
            () = &mut deadline => {
                return Err(format!(
                    "git clone did not finish within {}s ([ai.agent].clone_timeout_secs)",
                    limits.timeout.as_secs()
                ));
            }
            _ = poll.tick() => {
                if measure(dst).await? > limits.max_bytes() {
                    return Err(limits.oversize());
                }
            }
        }
    };
    group.defuse(); // reaped — the pgid may be recycled from here on
    if status.success() {
        return Ok(());
    }
    let stderr = stderr.await.unwrap_or_default();
    let tail: String = stderr.trim().chars().take(300).collect();
    Err(format!("git clone failed: {tail}"))
}

/// The first `cap` bytes of `r`, draining the rest so the writer never
/// blocks on a full pipe.
async fn read_head(mut r: impl AsyncRead + Unpin, cap: u64) -> String {
    let mut head = Vec::new();
    let _ = (&mut r).take(cap).read_to_end(&mut head).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    String::from_utf8_lossy(&head).into_owned()
}

async fn head_commit(repo: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--short", "HEAD"])
        .kill_on_drop(true)
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        _ => Err("could not resolve the cloned commit".into()),
    }
}

async fn measure(dir: &Path) -> Result<u64, String> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || dir_size(&dir))
        .await
        .map_err(|e| format!("could not measure the cloned repository's size: {e}"))
}

/// Swap the staged checkout into `repo/` if this job is still the current
/// generation. The whole check-swap-record sequence runs under the publish
/// lock: the check alone cannot stop a stale job that passed it and then
/// stalled (its removal of a previous multi-hundred-MB checkout can take
/// seconds) from clobbering the checkout a newer job publishes meanwhile.
/// Serialized, the stale job re-reads the bumped generation and bows out.
async fn publish(
    pool: &SqlitePool,
    ws: &Path,
    staging: &Path,
    paper_id: &str,
    sha: &str,
    size: u64,
    clone_gen: i64,
) -> Result<(), String> {
    let _lock = lock_publish(ws).await?;
    match super::store::current_clone_gen(pool, paper_id).await {
        Ok(Some(g)) if g == clone_gen => {}
        Ok(_) => return Ok(()), // superseded
        Err(e) => return Err(format!("could not check clone generation: {e}")),
    }
    let dst = ws.join("repo");
    remove_dir(&dst)
        .await
        .map_err(|e| format!("could not remove the previous checkout: {e}"))?;
    tokio::fs::rename(staging, &dst)
        .await
        .map_err(|e| format!("could not place the checkout: {e}"))?;
    let size = i64::try_from(size).unwrap_or(i64::MAX);
    match super::store::set_paper_code_ready(pool, paper_id, sha, size, clone_gen).await {
        Ok(true) => {}
        Ok(false) => {
            // Raced a newer attach or a detach between the generation check and
            // here: our row is gone or superseded. Remove the checkout we just
            // placed so no orphaned `repo/` is left behind for chat to expose.
            // A newer clone will re-place its own before reporting ready.
            let _ = remove_dir(&dst).await;
        }
        Err(e) => tracing::error!("paper_code ready write failed: {e}"),
    }
    Ok(())
}

/// Exclusive lock over everything that replaces or removes a workspace's
/// `repo/`, held until the returned file closes. An flock, not an
/// in-process mutex: sibling processes on the same library — CLI `xuewen
/// code set` / `code rm`, the desktop app — publish into the same workspace.
async fn lock_publish(ws: &Path) -> Result<File, String> {
    let path = ws.join(PUBLISH_LOCK);
    let locked = tokio::task::spawn_blocking(move || {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        file.lock()?;
        Ok::<_, std::io::Error>(file)
    })
    .await
    .map_err(|e| format!("could not lock the workspace: {e}"))?;
    locked.map_err(|e| format!("could not lock the workspace: {e}"))
}

/// Detach a paper's repo; `paper.txt` stays. The row goes first, so a clone
/// still in flight finds itself superseded and never publishes; the
/// checkout is then removed under the publish lock, after any publish
/// already past its generation check.
pub(crate) async fn detach(
    pool: &SqlitePool,
    library_root: &Path,
    paper_id: &str,
) -> anyhow::Result<()> {
    super::store::delete_paper_code(pool, paper_id).await?;
    let ws = super::workspace_dir(library_root, paper_id);
    if !tokio::fs::try_exists(&ws).await? {
        return Ok(());
    }
    let _lock = lock_publish(&ws).await.map_err(anyhow::Error::msg)?;
    remove_dir(&ws.join("repo")).await?;
    Ok(())
}

async fn remove_dir(dir: &Path) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(dir).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
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

    const LIMITS: CloneLimits = CloneLimits {
        max_repo_mb: 500,
        timeout: Duration::from_secs(60),
    };

    #[test]
    fn validate_rejects_non_https_and_credentials() {
        let none: &[String] = &[];
        assert!(validate_repo_url("https://github.com/x/y", none).is_ok());
        assert!(validate_repo_url("http://github.com/x/y", none).is_err());
        assert!(
            validate_repo_url("git@github.com:x/y.git", none)
                .unwrap_err()
                .contains("https://")
        );
        assert!(validate_repo_url("https://user:pw@github.com/x/y", none).is_err());
        assert!(validate_repo_url("https://user@github.com/x/y", none).is_err());
        assert!(validate_repo_url("https://:pw@github.com/x/y", none).is_err());
        assert!(validate_repo_url("file:///etc", none).is_err());
        assert!(validate_repo_url("https://github.com/x/y?a=1", none).is_err());
        assert!(validate_repo_url("https://github.com/x/y#main", none).is_err());
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
            "https://GitHub.COM/x/y",
            "https://github.com./x/y",
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
            "https://0x7f.1/x/y", // 127.0.0.1 in WHATWG IPv4 shorthand
            "https://gitea/x/y",
            "https://nas.local/x/y",
            "https://evil.example.com/x/y",
            "https://notgithub.com/x/y",
            "https://github.com.evil.com/x/y", // suffix-spoof attempt
            // The authority ends at `?`, `#` and (for https) `\`: a scan for
            // the first `/` would read each of these as a github.com subdomain.
            "https://127.0.0.1?.github.com/x",
            "https://127.0.0.1#.github.com/x",
            "https://127.0.0.1\\.github.com/x",
            // Userinfo spoofing: the host is what follows the `@`.
            "https://github.com@127.0.0.1/x/y",
            "https://github.com%40127.0.0.1/x/y",
            // Homoglyph: a Cyrillic `і` IDNA-encodes to an xn-- label.
            "https://gіthub.com/x/y",
            "https://xn--gthub-n4e.com/x/y",
        ] {
            assert!(validate_repo_url(u, none).is_err(), "should reject {u}");
        }
        // A configured self-hosted forge (and its subdomains) is admitted.
        let extra = vec!["git.example.com".to_string()];
        assert!(validate_repo_url("https://git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://ci.git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://git.example.com.evil.com/x/y", &extra).is_err());
        assert!(validate_repo_url("https://evilgit.example.com/x/y", &extra).is_err());
    }

    #[test]
    fn validate_matches_configured_hosts_as_the_parser_spells_them() {
        let extra = vec![
            " Git.Example.COM. ".to_string(),
            "bücher.example".to_string(),
            "10.0.0.5".to_string(),
            "[fd00::1]".to_string(),
        ];
        for u in [
            "https://git.example.com/x/y",
            "https://xn--bcher-kva.example/x/y",
            "https://bücher.example/x/y",
            "https://10.0.0.5/x/y",
            "https://[fd00::1]/x/y",
        ] {
            assert!(validate_repo_url(u, &extra).is_ok(), "should allow {u}");
        }
        // An IP literal never matches as a "subdomain".
        assert!(validate_repo_url("https://1.10.0.0.5/x/y", &extra).is_err());
        assert!(validate_repo_url("https://10.0.0.6/x/y", &extra).is_err());
    }

    /// `git` gets the parser's spelling, not the raw input: a backslash is a
    /// path separator to the parser, so it must be one to `git` too.
    #[test]
    fn validate_returns_the_reserialized_url() {
        let none: &[String] = &[];
        let url = validate_repo_url("  https://GitHub.com\\x\\y  ", none).unwrap();
        assert_eq!(url.as_str(), "https://github.com/x/y");
        let url = validate_repo_url("https://github.com:443/x/y", none).unwrap();
        assert_eq!(url.as_str(), "https://github.com/x/y");
    }

    fn git_repo() -> (tempfile::TempDir, Url) {
        let src = tempfile::tempdir().unwrap();
        let ok = |st: std::process::ExitStatus| assert!(st.success());
        ok(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(src.path())
            .status()
            .unwrap());
        std::fs::write(src.path().join("big.bin"), vec![7u8; 2 * 1024 * 1024]).unwrap();
        ok(std::process::Command::new("git")
            .args(["add", "big.bin"])
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
                "-m",
                "x",
            ])
            .current_dir(src.path())
            .status()
            .unwrap());
        let url = Url::from_directory_path(src.path()).unwrap();
        (src, url)
    }

    async fn pool_with_p1() -> SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf','2026-01-01T00:00:00+00:00','resolved')")
            .execute(&pool).await.unwrap();
        pool
    }

    /// Happy path against a local repo — git accepts file:// URLs for
    /// --depth clones, so this runs offline. Requires `git` (dev shell).
    #[tokio::test]
    async fn run_clone_pins_commit_and_reports_ready() {
        let (_src, url) = git_repo();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            LIMITS,
            clone_gen,
        )
        .await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Ready, "error: {:?}", c.error);
        assert!(c.commit_sha.is_some());
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        assert!(ws.join("repo/.git").exists());
        assert!(ws.join("repo/big.bin").exists());
        assert!(
            !ws.join(format!("{STAGING_PREFIX}{clone_gen}.lock"))
                .exists(),
            "the pid lock must not outlive the job"
        );
        assert!(!ws.join(format!("{STAGING_PREFIX}{clone_gen}")).exists());
    }

    #[tokio::test]
    async fn run_clone_reports_failure_as_error_status() {
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let url = Url::from_directory_path(std::env::temp_dir().join("nonexistent-repo")).unwrap();
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            LIMITS,
            clone_gen,
        )
        .await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Error);
        assert!(c.error.unwrap().starts_with("git clone failed: "));
        assert!(
            !crate::agent::workspace_dir(lib.path(), "p1")
                .join(format!("{STAGING_PREFIX}{clone_gen}.lock"))
                .exists(),
            "the pid lock must not outlive a failed job"
        );
    }

    /// The budget is enforced on the staged clone, which is then discarded:
    /// nothing over the limit is ever published.
    #[tokio::test]
    async fn run_clone_over_budget_fails_and_publishes_nothing() {
        let (_src, url) = git_repo();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        let limits = CloneLimits {
            max_repo_mb: 1,
            ..LIMITS
        };

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            limits,
            clone_gen,
        )
        .await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Error);
        assert!(c.error.unwrap().contains("over the 1 MB limit"));
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        assert!(!ws.join("repo").exists());
        assert!(!ws.join(format!("{STAGING_PREFIX}{clone_gen}")).exists());
    }

    /// A server that accepts the connection and never answers stalls git
    /// indefinitely; the deadline must kill it and resolve the row.
    #[tokio::test]
    async fn run_clone_times_out_a_stalled_transfer() {
        // Never accepted: the kernel completes the handshake from the backlog
        // and git then waits forever for a response.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/x/y.git",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        let limits = CloneLimits {
            timeout: Duration::from_millis(500),
            ..LIMITS
        };

        tokio::time::timeout(
            Duration::from_secs(30),
            run_clone(
                pool.clone(),
                lib.path().to_path_buf(),
                "p1".into(),
                url,
                limits,
                clone_gen,
            ),
        )
        .await
        .expect("the clone deadline must fire");

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Error);
        assert!(c.error.unwrap().contains("clone_timeout_secs"));
        drop(listener);
    }

    /// Publishing waits for the workspace's publish lock — held here as a
    /// sibling process's `code rm` or publish would hold it.
    #[tokio::test]
    async fn run_clone_publishes_only_under_the_workspace_lock() {
        let (_src, url) = git_repo();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::create_dir_all(&ws).await.unwrap();
        let held = lock_publish(&ws).await.unwrap();

        let job = tokio::spawn(run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            LIMITS,
            clone_gen,
        ));
        tokio::time::sleep(Duration::from_secs(2)).await;
        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning);
        assert!(!ws.join("repo").exists());

        drop(held);
        job.await.unwrap();
        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Ready, "error: {:?}", c.error);
        assert!(ws.join("repo/.git").exists());
    }

    #[tokio::test]
    async fn detach_removes_the_row_and_the_checkout_but_keeps_paper_txt() {
        let (_src, url) = git_repo();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            LIMITS,
            clone_gen,
        )
        .await;
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::write(ws.join("paper.txt"), "text")
            .await
            .unwrap();

        detach(&pool, lib.path(), "p1").await.unwrap();
        detach(&pool, lib.path(), "p1").await.unwrap();
        detach(&pool, lib.path(), "never-attached").await.unwrap();

        assert!(store::get_paper_code(&pool, "p1").await.unwrap().is_none());
        assert!(!ws.join("repo").exists());
        assert!(ws.join("paper.txt").exists());
    }

    /// A crash mid-clone leaves status='cloning' and a staging dir behind;
    /// the boot sweep must resolve the row and delete the staging dir while
    /// leaving the published checkout and paper.txt alone.
    #[tokio::test]
    async fn reconcile_startup_fails_stranded_rows_and_sweeps_staging() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf','2026-01-01T00:00:00+00:00','resolved')")
            .execute(&pool).await.unwrap();
        store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::create_dir_all(ws.join(format!("{STAGING_PREFIX}0")))
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
        assert!(!ws.join(format!("{STAGING_PREFIX}0")).exists());
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
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf','2026-01-01T00:00:00+00:00','resolved')")
            .execute(&pool).await.unwrap();
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        tokio::fs::create_dir_all(ws.join(format!("{STAGING_PREFIX}{clone_gen}")))
            .await
            .unwrap();
        // This test process stands in for the sibling: its pid is live.
        tokio::fs::write(
            ws.join(format!("{STAGING_PREFIX}{clone_gen}.lock")),
            std::process::id().to_string(),
        )
        .await
        .unwrap();

        reconcile_startup(&pool, lib.path()).await;

        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning, "live clone must survive");
        assert_eq!(
            store::current_clone_gen(&pool, "p1").await.unwrap(),
            Some(clone_gen)
        );
        assert!(ws.join(format!("{STAGING_PREFIX}{clone_gen}")).exists());
        assert!(
            ws.join(format!("{STAGING_PREFIX}{clone_gen}.lock"))
                .exists()
        );
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
            sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES (?,?,?,'2026-01-01T00:00:00+00:00','resolved')")
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
        tokio::fs::create_dir_all(ws1.join(format!("{STAGING_PREFIX}{gen1}")))
            .await
            .unwrap();
        // i32::MAX exceeds every platform's pid ceiling: reliably dead.
        tokio::fs::write(
            ws1.join(format!("{STAGING_PREFIX}{gen1}.lock")),
            i32::MAX.to_string(),
        )
        .await
        .unwrap();
        tokio::fs::create_dir_all(ws2.join(format!("{STAGING_PREFIX}{gen2}")))
            .await
            .unwrap();
        tokio::fs::write(
            ws2.join(format!("{STAGING_PREFIX}{gen2}.lock")),
            "not a pid",
        )
        .await
        .unwrap();

        reconcile_startup(&pool, lib.path()).await;

        for (id, ws, clone_gen) in [("p1", &ws1, gen1), ("p2", &ws2, gen2)] {
            let c = store::get_paper_code(&pool, id).await.unwrap().unwrap();
            assert_eq!(c.status, CodeStatus::Error, "{id}");
            assert!(!ws.join(format!("{STAGING_PREFIX}{clone_gen}")).exists());
            assert!(
                !ws.join(format!("{STAGING_PREFIX}{clone_gen}.lock"))
                    .exists()
            );
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
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf','2026-01-01T00:00:00+00:00','resolved')")
            .execute(&pool).await.unwrap();
        let clone_gen = store::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
            .await
            .unwrap();
        store::set_paper_code_ready(&pool, "p1", "abc1234", 42, clone_gen)
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
        let (_src, url) = git_repo();
        let lib = tempfile::tempdir().unwrap();
        let pool = pool_with_p1().await;

        // gen 0 is captured, then a re-attach bumps the row to gen 1 before the
        // stale gen-0 job runs.
        let stale_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        let current_gen = store::upsert_paper_code_cloning(&pool, "p1", url.as_str())
            .await
            .unwrap();
        assert!(current_gen > stale_gen);

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            LIMITS,
            stale_gen,
        )
        .await;

        // The stale job dropped its outcome: the row is still 'cloning' (gen 1),
        // not 'ready', and no checkout was published.
        let c = store::get_paper_code(&pool, "p1").await.unwrap().unwrap();
        assert_eq!(c.status, CodeStatus::Cloning);
        let ws = crate::agent::workspace_dir(lib.path(), "p1");
        assert!(!ws.join("repo").exists());
        assert!(!ws.join(format!("{STAGING_PREFIX}{stale_gen}")).exists());
    }
}
