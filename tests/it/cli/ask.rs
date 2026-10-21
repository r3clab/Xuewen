use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use super::{
    fails, library, library_with, ok, spawn_xuewen, stderr, stdout, xuewen, xuewen_with_stdin,
};
use crate::support;

/// A library with paper `p1` and both agent backends on the stub runner.
async fn configured() -> PathBuf {
    let (_pool, config) = library_with(&format!(
        "[ai.agent]\nrunner = {:?}\n\n[ai.agent.claude_code]\n\n[ai.agent.codex]\nmodel = \"gpt-5.2-codex\"\n",
        support::stub_runner_path()
    ))
    .await;
    config
}

fn history(config: &Path) -> String {
    ok(config, &["chat", "history", "p1"])
}

#[tokio::test]
async fn a_question_streams_its_answer_and_saves_the_exchange() {
    let config = configured().await;
    let out = xuewen(&config, &["ask", "p1", "what is this?"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "Hello from claude_code\n");
    assert_eq!(stderr(&out), "[Read] paper.txt\n");

    let saved = history(&config);
    assert!(
        saved.contains(":\nwhat is this?\n\nClaude Code, "),
        "got: {saved}"
    );
    assert!(
        saved.ends_with(":\n[Read] paper.txt\nHello from claude_code\n"),
        "got: {saved}"
    );
}

#[tokio::test]
async fn model_picks_the_backend_and_rejects_unknown_ones() {
    let config = configured().await;
    assert_eq!(
        ok(&config, &["ask", "p1", "hi", "--model", "codex"]),
        "Hello from codex\n"
    );

    let err = fails(&config, &["ask", "p1", "hi", "--model", "nope"]);
    assert!(
        err.contains("unknown model nope (available: claude_code, codex)"),
        "got: {err}"
    );
}

#[tokio::test]
async fn a_failed_turn_exits_nonzero_and_saves_nothing() {
    let config = configured().await;
    let err = fails(&config, &["ask", "p1", "please fail"]);
    assert!(err.contains("boom"), "got: {err}");
    assert!(history(&config).starts_with("no conversation yet"));
}

#[tokio::test]
async fn without_a_question_each_input_line_is_a_turn() {
    let config = configured().await;
    let out = xuewen_with_stdin(
        &config,
        &["ask", "p1"],
        "first\n\n  \nplease fail\nsecond\n",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "Hello from claude_code\nHello from claude_code\n"
    );
    assert!(
        stderr(&out).contains("error: boom"),
        "got: {}",
        stderr(&out)
    );
    assert!(!stderr(&out).contains("> "), "no prompt off a terminal");
    let saved = history(&config);
    assert!(
        saved.contains(":\nfirst\n") && saved.contains(":\nsecond\n"),
        "got: {saved}"
    );
}

#[tokio::test]
async fn ask_needs_a_configured_agent_and_an_active_paper() {
    let (_pool, unconfigured) = library().await;
    let err = fails(&unconfigured, &["ask", "p1", "hi"]);
    assert!(err.contains("agent ask is not configured"), "got: {err}");

    let config = configured().await;
    ok(&config, &["delete", "p1", "--yes"]);
    let err = fails(&config, &["ask", "p1", "hi"]);
    assert!(err.contains("p1 is in the trash"), "got: {err}");
}

#[tokio::test]
async fn chat_models_lists_backends_default_first() {
    let config = configured().await;
    assert_eq!(
        ok(&config, &["chat", "models"]),
        "claude_code  Claude Code  (default)\ncodex  Codex, gpt-5.2-codex\n"
    );

    let (_pool, unconfigured) = library().await;
    assert_eq!(
        ok(&unconfigured, &["chat", "models"]),
        "agent ask is not configured — set [ai.agent.claude_code] or [ai.agent.codex]\n"
    );
}

#[tokio::test]
async fn chat_clear_forgets_the_conversation() {
    let config = configured().await;
    ok(&config, &["ask", "p1", "hi"]);
    assert_eq!(
        ok(&config, &["chat", "clear", "p1", "--yes"]),
        "removed 2\n"
    );
    assert!(history(&config).starts_with("no conversation yet"));
}

#[cfg(unix)]
mod signals {
    use super::*;

    /// The stub's `orphan` turn prints its grandchild's pid, then hangs.
    fn grandchild_pid(out: &mut impl BufRead) -> i32 {
        let chunk = out.fill_buf().unwrap();
        let (text, n) = (String::from_utf8_lossy(chunk).into_owned(), chunk.len());
        out.consume(n);
        text.trim()
            .parse()
            .unwrap_or_else(|_| panic!("expected a pid, got {text:?}"))
    }

    async fn reaped(pid: i32) -> bool {
        let dead = support::eventually(std::time::Duration::from_secs(5), || unsafe {
            libc::kill(pid, 0) == -1
        })
        .await;
        if !dead {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        dead
    }

    fn interrupt(child: &std::process::Child) {
        unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    }

    #[tokio::test]
    async fn ctrl_c_ends_a_one_shot_with_130_and_kills_the_agent() {
        let config = configured().await;
        let mut child = spawn_xuewen(&config, &["ask", "p1", "orphan"]);
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let pid = grandchild_pid(&mut out);
        interrupt(&child);
        assert_eq!(child.wait().unwrap().code(), Some(130));
        assert!(reaped(pid).await, "grandchild {pid} survived");
        assert!(history(&config).starts_with("no conversation yet"));
    }

    #[tokio::test]
    async fn ctrl_c_mid_answer_cancels_only_that_turn() {
        let config = configured().await;
        let mut child = spawn_xuewen(&config, &["ask", "p1"]);
        let mut stdin = child.stdin.take().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        writeln!(stdin, "orphan").unwrap();
        let pid = grandchild_pid(&mut out);
        interrupt(&child);
        assert!(reaped(pid).await, "grandchild {pid} survived");

        writeln!(stdin, "and now?").unwrap();
        drop(stdin);
        let mut rest = String::new();
        out.read_to_string(&mut rest).unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("(cancelled)"),
            "got: {}",
            stderr(&output)
        );
        assert!(rest.contains("Hello from claude_code"), "got: {rest}");
    }
}
