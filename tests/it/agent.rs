use crate::support;

use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use xuewen::agent::{AgentEvent, AgentService, TurnMessage, TurnPaper, TurnRequest};
use xuewen::config::{AgentBackendConfig, AgentConfig};

fn stub_cfg(timeout_secs: u64) -> AgentConfig {
    AgentConfig {
        claude_code: Some(AgentBackendConfig::default()),
        codex: Some(AgentBackendConfig {
            model: Some("gpt-5.2-codex".into()),
        }),
        timeout_secs,
        runner: Some(support::stub_runner_path()),
        ..AgentConfig::default()
    }
}

fn turn(question: &str) -> TurnRequest {
    TurnRequest {
        backend: "claude_code".into(),
        model: None,
        workspace: std::env::temp_dir().to_string_lossy().into_owned(),
        has_repo: false,
        paper: TurnPaper {
            title: Some("T".into()),
            authors: vec![],
            venue: None,
            year: None,
        },
        transcript: vec![TurnMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        question: question.into(),
    }
}

async fn collect(svc: &Arc<AgentService>, req: TurnRequest) -> Vec<AgentEvent> {
    let s = svc.run_turn(req);
    futures_util::pin_mut!(s);
    let mut out = Vec::new();
    while let Some(ev) = s.next().await {
        out.push(ev);
    }
    out
}

#[tokio::test]
async fn run_turn_streams_tool_deltas_done() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("what is this?")).await;
    assert!(
        matches!(&evs[0], AgentEvent::Tool { name, detail } if name == "Read" && detail == "paper.txt")
    );
    assert!(matches!(&evs[1], AgentEvent::Delta { text } if text == "Hel"));
    assert!(matches!(&evs[2], AgentEvent::Delta { text } if text == "lo from claude_code"));
    assert!(matches!(evs.last().unwrap(), AgentEvent::Done));
}

#[tokio::test]
async fn run_turn_surfaces_runner_errors() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("please fail")).await;
    assert!(matches!(evs.last().unwrap(), AgentEvent::Error { message } if message == "boom"));
}

#[tokio::test]
async fn run_turn_times_out_hung_runner() {
    let svc = AgentService::from_config(&stub_cfg(1)).unwrap();
    let evs = collect(&svc, turn("hang forever")).await;
    assert!(
        matches!(evs.last().unwrap(), AgentEvent::Error { message } if message.contains("timed out"))
    );
}

#[tokio::test]
async fn run_turn_reports_silent_death_with_stderr() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    let evs = collect(&svc, turn("die now")).await;
    assert!(
        matches!(evs.last().unwrap(), AgentEvent::Error { message } if message.contains("stub exploded"))
    );
}

/// Stop/timeout must kill the SDKs' grandchildren, not just the runner: the
/// stub spawns a hanging grandchild and reports its pid; once the stream is
/// dropped (after the timeout error) the whole process group must be gone.
#[cfg(unix)]
#[tokio::test]
async fn run_turn_timeout_kills_grandchildren() {
    let svc = AgentService::from_config(&stub_cfg(1)).unwrap();
    let evs = collect(&svc, turn("orphan me")).await;
    let pid: i32 = evs
        .iter()
        .find_map(|e| match e {
            AgentEvent::Delta { text } => text.trim().parse().ok(),
            _ => None,
        })
        .expect("stub reports the grandchild pid");
    assert!(
        matches!(evs.last().unwrap(), AgentEvent::Error { message } if message.contains("timed out"))
    );
    // The group kill fires when the stream drops; init still has to reap the
    // orphan before kill(pid, 0) reports it gone, so poll briefly.
    let dead = support::eventually(std::time::Duration::from_secs(5), || unsafe {
        libc::kill(pid, 0) == -1
    })
    .await;
    if !dead {
        unsafe { libc::kill(pid, libc::SIGKILL) }; // don't leak it past the test
    }
    assert!(dead, "grandchild {pid} survived the process-group kill");
}

#[tokio::test]
async fn preflight_passes_through_the_runner_preflight_mode() {
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    assert_eq!(svc.preflight().await, Vec::<String>::new());
}

#[tokio::test]
async fn preflight_reports_a_missing_runner() {
    let mut cfg = stub_cfg(30);
    cfg.runner = Some(PathBuf::from("/nonexistent/runner.mjs"));
    let svc = AgentService::from_config(&cfg).unwrap();
    let problems = svc.preflight().await;
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("not found"), "got: {}", problems[0]);
}

#[tokio::test]
async fn from_config_gates_on_backends() {
    assert!(AgentService::from_config(&AgentConfig::default()).is_none());
    let svc = AgentService::from_config(&stub_cfg(30)).unwrap();
    assert_eq!(svc.backends.len(), 2);
    assert_eq!(
        svc.backend("codex").unwrap().model.as_deref(),
        Some("gpt-5.2-codex")
    );
    assert!(svc.backend("nope").is_none());
}
