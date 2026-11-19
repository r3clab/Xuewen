mod ask;
mod daily;
mod freshness;
mod library;
mod translate;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use sqlx::SqlitePool;
use tempfile::TempDir;

use crate::support;

async fn library() -> (SqlitePool, PathBuf, TempDir) {
    library_with("").await
}

async fn library_with(extra: &str) -> (SqlitePool, PathBuf, TempDir) {
    let (pool, _root, dir) = support::pool_and_root_with_paper("p1").await;
    let config = support::write_config(dir.path(), extra);
    (pool, config, dir)
}

fn command(config: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xuewen"));
    cmd.arg("--config")
        .arg(config)
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE");
    cmd
}

fn xuewen(config: &Path, args: &[&str]) -> Output {
    command(config, args).stdin(Stdio::null()).output().unwrap()
}

fn spawn_xuewen(config: &Path, args: &[&str]) -> Child {
    command(config, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn xuewen_with_stdin(config: &Path, args: &[&str], input: &str) -> Output {
    let mut child = spawn_xuewen(config, args);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn ok(config: &Path, args: &[&str]) -> String {
    let out = xuewen(config, args);
    assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    stdout(&out)
}

fn fails(config: &Path, args: &[&str]) -> String {
    let out = xuewen(config, args);
    assert!(
        !out.status.success(),
        "{args:?} succeeded: {}",
        stdout(&out)
    );
    stderr(&out)
}

#[tokio::test]
async fn proxy_cookie_set_refuses_a_blank_cookie() {
    let (pool, config, _dir) = library().await;
    pool.close().await;

    ok(&config, &["proxy-cookie", "--set", " a=b "]);
    for blank in ["", "   "] {
        assert_eq!(
            fails(&config, &["proxy-cookie", "--set", blank]),
            "Error: empty cookie — to remove the stored cookie, run: xuewen proxy-cookie --clear\n"
        );
    }
    assert!(ok(&config, &["proxy-cookie"]).starts_with("proxy cookie set (updated "));
}

#[tokio::test]
async fn code_commands_refuse_a_trashed_paper() {
    let (pool, _root, dir) = support::pool_and_root_with_paper("cccc3333").await;
    xuewen::library::trash::soft_delete(&pool, "cccc3333")
        .await
        .unwrap();
    let config = support::write_config(dir.path(), "");

    for args in [
        &["code", "set", "cccc", "https://github.com/x/y"][..],
        &["code", "status", "cccc"],
        &["code", "rm", "cccc"],
    ] {
        assert_eq!(
            fails(&config, args),
            "Error: cccc3333 is in the trash — run: xuewen restore cccc3333\n",
            "{args:?}"
        );
    }
    assert!(xuewen::agent::store::get_paper_code(&pool, "cccc3333")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn code_set_needs_agent_ask_like_the_web_does() {
    let (pool, config, _dir) = library().await;
    assert_eq!(
        fails(&config, &["code", "set", "p1", "https://github.com/x/y"]),
        "Error: agent ask is not configured — set [ai.agent.claude_code] or [ai.agent.codex]\n"
    );
    assert!(xuewen::agent::store::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn xuewen_config_names_the_config_when_the_flag_is_absent() {
    let (pool, config, _dir) = library().await;
    pool.close().await;
    let elsewhere = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xuewen"))
        .arg("stats")
        .env("XUEWEN_CONFIG", &config)
        .env_remove("RUST_LOG")
        .current_dir(elsewhere.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).starts_with("1 paper"), "got: {}", stdout(&out));
}
