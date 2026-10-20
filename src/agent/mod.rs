//! Agent Ask: a per-turn Node sidecar drives the Claude Code / Codex SDKs
//! over a read-only per-paper workspace (`<library_root>/agent/<paper_id>/`
//! holding `paper.txt` and, when attached, `repo/`). One JSON request goes
//! in on stdin; JSON-lines events come back on stdout.

pub(crate) mod ask;
pub(crate) mod code;
pub mod history;
pub mod store;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::config::AgentConfig;
use crate::library::model::Paper;

#[derive(Debug, Clone)]
pub struct AgentBackend {
    pub(crate) id: String,
    pub(crate) label: String,
    pub model: Option<String>,
}

/// One event from the runner (`{"type": "..."}` JSON-lines).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Delta { text: String },
    Tool { name: String, detail: String },
    Done,
    Error { message: String },
}

#[derive(Debug, serde::Serialize)]
pub struct TurnPaper {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
}

impl TurnPaper {
    fn from_paper(p: &Paper) -> Self {
        Self {
            title: p.meta.title.clone(),
            authors: p.meta.authors.0.clone(),
            venue: p.meta.venue.clone(),
            year: p.meta.year,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TurnMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TurnRequest {
    pub backend: String,
    pub model: Option<String>,
    pub workspace: String,
    #[serde(rename = "hasRepo")]
    pub has_repo: bool,
    pub paper: TurnPaper,
    pub transcript: Vec<TurnMessage>,
    pub question: String,
}

/// `<library_root>/agent/<paper_id>` — the per-paper agent workspace.
pub fn workspace_dir(library_root: &Path, paper_id: &str) -> PathBuf {
    library_root.join("agent").join(paper_id)
}

pub struct AgentService {
    pub backends: Vec<AgentBackend>,
    runner: PathBuf,
    timeout: Duration,
}

impl AgentService {
    /// `None` when no backend subsection is configured — the feature is off.
    pub fn from_config(cfg: &AgentConfig) -> Option<Arc<Self>> {
        let mut backends = Vec::new();
        if let Some(b) = &cfg.claude_code {
            backends.push(AgentBackend {
                id: "claude_code".into(),
                label: "Claude Code".into(),
                model: b.model.clone(),
            });
        }
        if let Some(b) = &cfg.codex {
            backends.push(AgentBackend {
                id: "codex".into(),
                label: "Codex".into(),
                model: b.model.clone(),
            });
        }
        if backends.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            backends,
            runner: cfg
                .runner
                .clone()
                .unwrap_or_else(|| PathBuf::from("agent-runner/src/runner.mjs")),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }))
    }

    pub fn backend(&self, id: &str) -> Option<&AgentBackend> {
        self.backends.iter().find(|b| b.id == id)
    }

    /// Startup sanity check: the feature stays enabled either way (per-turn
    /// errors carry the actionable message); this only powers a launch warning.
    /// The probe runs the real runner in `--preflight` mode, which checks the
    /// Node version and imports each configured backend's SDK — so missing
    /// `agent-runner/node_modules` is caught at launch, not on the first turn.
    pub async fn preflight(&self) -> Vec<String> {
        if !self.runner.exists() {
            return vec![format!(
                "agent runner not found at {} — run `npm --prefix agent-runner install` or set [ai.agent].runner",
                self.runner.display()
            )];
        }
        let mut cmd = Command::new("node");
        cmd.arg(&self.runner).arg("--preflight");
        for b in &self.backends {
            cmd.arg(&b.id);
        }
        match cmd.output().await {
            Ok(o) if o.status.success() => Vec::new(),
            Ok(o) => {
                let detail: String = String::from_utf8_lossy(&o.stderr)
                    .trim()
                    .chars()
                    .take(400)
                    .collect();
                vec![if detail.is_empty() {
                    "the agent runner failed its preflight check".to_string()
                } else {
                    detail
                }]
            }
            Err(_) => vec!["`node` not found on PATH — the agent needs Node ≥ 20".to_string()],
        }
    }

    /// The paper's workspace, created lazily. `paper.txt` is written once and
    /// then trusted — unless the `paper.txt.failed` sentinel marks it as the
    /// placeholder from a failed extraction, which re-attempts on every turn:
    /// a *transient* failure (pdftotext momentarily off PATH, a locked file)
    /// must not be promoted to "this paper has no text" forever. Extraction
    /// failure still writes the placeholder so the workspace always has it.
    async fn ensure_workspace(&self, library_root: &Path, paper: &Paper) -> Result<PathBuf> {
        let ws = workspace_dir(library_root, &paper.id);
        tokio::fs::create_dir_all(&ws).await?;
        let txt = ws.join("paper.txt");
        let failed = ws.join("paper.txt.failed");
        if !tokio::fs::try_exists(&txt).await? || tokio::fs::try_exists(&failed).await? {
            let pdf = library_root.join(&paper.rel_path);
            match tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf)).await? {
                Ok(text) => {
                    tokio::fs::write(&txt, text).await?;
                    let _ = tokio::fs::remove_file(&failed).await;
                }
                Err(e) => {
                    tracing::warn!("agent workspace: text extraction failed: {e}");
                    // Sentinel before placeholder: a crash between the writes
                    // then leaves the retry armed rather than the cache poisoned.
                    tokio::fs::write(&failed, "").await?;
                    tokio::fs::write(&txt, "(The paper's text could not be extracted.)").await?;
                }
            }
        }
        Ok(ws)
    }

    /// Spawn the runner for one turn and stream its events. Dropping the
    /// stream (Stop / client disconnect) kills the child's whole process
    /// group; a hung runner is killed the same way at the turn timeout.
    pub fn run_turn(
        self: &Arc<Self>,
        req: TurnRequest,
    ) -> impl futures_util::Stream<Item = AgentEvent> {
        let runner = self.runner.clone();
        let timeout = self.timeout;
        async_stream::stream! {
            let mut cmd = Command::new("node");
            cmd.arg(&runner)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                // Backstop (and the whole behavior off unix); it SIGKILLs
                // only the direct child — grandchildren are the guard's job.
                .kill_on_drop(true);
            // A fresh process group so Stop/timeout can kill the whole tree:
            // both SDKs run their vendored CLI as a *grandchild*, which a
            // SIGKILL to node alone leaves running (mid tool call or API
            // request, burning tokens) until it next writes to the dead pipe.
            #[cfg(unix)]
            cmd.process_group(0);
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    yield AgentEvent::Error { message: format!("could not start the agent runner (is Node installed?): {e}") };
                    return;
                }
            };
            // Declared after `child` so it drops first: killpg then runs while
            // the child is still unreaped, so its pgid cannot be recycled.
            let mut group = GroupKillGuard::new(child.id());
            let body = match serde_json::to_vec(&req) {
                Ok(b) => b,
                Err(e) => { yield AgentEvent::Error { message: e.to_string() }; return; }
            };
            let mut stdin = child.stdin.take().expect("stdin piped");
            if let Err(e) = stdin.write_all(&body).await {
                yield AgentEvent::Error { message: format!("could not reach the agent runner: {e}") };
                return;
            }
            drop(stdin); // EOF marks the request complete

            // Drain stderr for the whole turn: left unread, ~64KB of SDK/Node
            // noise fills the pipe and blocks the child mid-write until the
            // turn times out. Only a bounded tail is kept, for error messages.
            let stderr_tail = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
            let drain = {
                let tail = stderr_tail.clone();
                let mut stderr = child.stderr.take().expect("stderr piped");
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 4096];
                    loop {
                        match stderr.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let mut t = tail.lock().unwrap();
                                t.extend_from_slice(&buf[..n]);
                                if t.len() > STDERR_TAIL_CAP {
                                    let cut = t.len() - STDERR_TAIL_CAP;
                                    t.drain(..cut);
                                }
                            }
                        }
                    }
                })
            };
            let tail_text = |tail: &std::sync::Mutex<Vec<u8>>| -> String {
                let t = tail.lock().unwrap();
                String::from_utf8_lossy(&t).trim().chars().take(400).collect()
            };
            let tail_suffix = |tail: &std::sync::Mutex<Vec<u8>>| -> String {
                let t = tail_text(tail);
                if t.is_empty() { String::new() } else { format!("; stderr: {t}") }
            };

            let stdout = child.stdout.take().expect("stdout piped");
            let mut lines = BufReader::new(stdout).lines();
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                match tokio::time::timeout_at(deadline, lines.next_line()).await {
                    Err(_) => {
                        yield AgentEvent::Error { message: format!("the agent timed out after {}s{}", timeout.as_secs(), tail_suffix(&stderr_tail)) };
                        return; // the drops kill the process group and reap the child
                    }
                    Ok(Err(e)) => {
                        yield AgentEvent::Error { message: format!("{e}{}", tail_suffix(&stderr_tail)) };
                        return;
                    }
                    Ok(Ok(None)) => break, // stdout closed
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<AgentEvent>(&line) {
                            Ok(ev) => {
                                let terminal = matches!(ev, AgentEvent::Done | AgentEvent::Error { .. });
                                if terminal { group.defuse(); }
                                yield ev;
                                if terminal { return; }
                            }
                            // The runner owns its stdout, so an unparseable
                            // line is protocol drift or stdout pollution from
                            // an SDK dependency — exceptional, keep it visible.
                            Err(_) => tracing::warn!("agent runner emitted an unparseable line: {line}"),
                        }
                    }
                }
            }
            // stdout closed without done/error: surface the stderr tail.
            let code = child.wait().await.ok().and_then(|s| s.code());
            group.defuse(); // reaped — the pgid may be recycled from here on
            // The child is gone so stderr hits EOF; still bound the wait in
            // case a leaked grandchild holds the write end open.
            let _ = tokio::time::timeout_at(deadline, drain).await;
            let code_str = code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "none".into());
            yield AgentEvent::Error {
                message: format!("the agent runner exited unexpectedly (code {code_str}): {}", tail_text(&stderr_tail)),
            };
        }
    }
}

/// Bytes of the runner's stderr kept for error messages.
const STDERR_TAIL_CAP: usize = 4096;

/// Kills the runner's process group when a turn ends abnormally (Stop, client
/// disconnect, timeout, a broken pipe). Defused once the runner emitted a
/// terminal event or has been reaped, so a recycled pgid is never signalled;
/// on non-unix it is inert and `kill_on_drop` alone applies.
struct GroupKillGuard {
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl GroupKillGuard {
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn new(pid: Option<u32>) -> Self {
        Self {
            #[cfg(unix)]
            pgid: pid.and_then(|p| i32::try_from(p).ok()),
        }
    }

    fn defuse(&mut self) {
        #[cfg(unix)]
        {
            self.pgid = None;
        }
    }
}

impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // SAFETY: killpg touches no Rust-managed state.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_is_under_library_agent() {
        assert_eq!(
            workspace_dir(Path::new("/lib"), "p1"),
            PathBuf::from("/lib/agent/p1")
        );
    }

    /// The wire contract's canonical serialization, shared with the runner:
    /// agent-runner/test/protocol.test.mjs asserts `emit` produces exactly
    /// these lines — change the fixture and both sides together (same
    /// convention as the search-qualifier fixtures).
    #[test]
    fn shared_fixture_events_deserialize_into_each_variant() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/agent-runner/test/fixtures/events.jsonl"
        );
        let data = std::fs::read_to_string(fixture).unwrap();
        let evs: Vec<AgentEvent> = data
            .lines()
            .map(|l| serde_json::from_str(l).expect(l))
            .collect();
        assert_eq!(evs.len(), 4, "one line per event type");
        assert!(matches!(&evs[0], AgentEvent::Delta { text } if text == "Hel"));
        assert!(
            matches!(&evs[1], AgentEvent::Tool { name, detail } if name == "Read" && detail == "paper.txt")
        );
        assert!(matches!(&evs[2], AgentEvent::Done));
        assert!(matches!(&evs[3], AgentEvent::Error { message } if message == "boom"));
    }

    fn test_service() -> Arc<AgentService> {
        AgentService::from_config(&crate::config::AgentConfig {
            claude_code: Some(Default::default()),
            ..Default::default()
        })
        .unwrap()
    }

    fn test_paper(rel_path: &str) -> crate::library::model::Paper {
        crate::library::model::Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: rel_path.into(),
            cite_key: Some("smith2024".into()),
            added_at: "2026-01-01".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: crate::library::model::PaperMeta {
                title: Some("A Great Paper".into()),
                abstract_text: None,
                authors: crate::library::model::Authors(vec!["A. Smith".into()]),
                venue: None,
                year: Some(2024),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: crate::library::model::PaperStatus::Resolved,
            },
        }
    }

    #[tokio::test]
    async fn ensure_workspace_keeps_existing_paper_txt() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspace_dir(dir.path(), "p1");
        tokio::fs::create_dir_all(&ws).await.unwrap();
        tokio::fs::write(ws.join("paper.txt"), "cached text")
            .await
            .unwrap();
        let got = test_service()
            .ensure_workspace(dir.path(), &test_paper("p.pdf"))
            .await
            .unwrap();
        assert_eq!(got, ws);
        assert_eq!(
            std::fs::read_to_string(ws.join("paper.txt")).unwrap(),
            "cached text"
        );
    }

    #[tokio::test]
    async fn ensure_workspace_failure_writes_placeholder_and_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        // rel_path points nowhere, so extraction fails.
        let ws = test_service()
            .ensure_workspace(dir.path(), &test_paper("missing.pdf"))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(ws.join("paper.txt")).unwrap(),
            "(The paper's text could not be extracted.)"
        );
        assert!(ws.join("paper.txt.failed").exists());
    }

    /// Requires `pdftotext` (dev shell). A prior failed extraction (sentinel
    /// present) is re-attempted, and success clears the sentinel.
    #[tokio::test]
    async fn ensure_workspace_sentinel_triggers_reextraction() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspace_dir(dir.path(), "p1");
        tokio::fs::create_dir_all(&ws).await.unwrap();
        tokio::fs::write(
            ws.join("paper.txt"),
            "(The paper's text could not be extracted.)",
        )
        .await
        .unwrap();
        tokio::fs::write(ws.join("paper.txt.failed"), "")
            .await
            .unwrap();

        // The transient failure is over: the PDF now extracts fine.
        use printpdf::*;
        let mut doc = PdfDocument::new("t");
        let ops = vec![
            Op::StartTextSection,
            Op::SetFont {
                font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                size: Pt(12.0),
            },
            Op::SetTextCursor {
                pos: Point::new(Mm(15.0), Mm(280.0)),
            },
            Op::ShowText {
                items: vec![TextItem::Text("recovered body".to_string())],
            },
            Op::EndTextSection,
        ];
        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let bytes = doc
            .with_pages(vec![page])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write(dir.path().join("p.pdf"), bytes).unwrap();

        let got = test_service()
            .ensure_workspace(dir.path(), &test_paper("p.pdf"))
            .await
            .unwrap();
        let text = std::fs::read_to_string(got.join("paper.txt")).unwrap();
        assert!(text.contains("recovered body"), "got: {text}");
        assert!(!got.join("paper.txt.failed").exists());
    }
}
