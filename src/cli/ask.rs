use std::io::IsTerminal as _;

use anyhow::{bail, Context as _, Result};
use futures_util::{Stream, StreamExt};
use tokio::sync::mpsc;

use super::signals::{Interrupted, Signal, Signals};
use super::Ctx;
use crate::agent::ask::{AskRequest, ToolCall, TurnEvent};
use crate::agent::AgentService;
use crate::library::model::Paper;

#[derive(clap::Args)]
pub(super) struct AskArgs {
    /// Paper id (exact or unique prefix).
    paper: String,
    /// The question. Without one, read questions line by line until end of
    /// input (Ctrl-D); Ctrl-C cancels the answer in progress.
    question: Option<String>,
    /// Agent backend (see: xuewen chat models); defaults to the first one
    /// configured.
    #[arg(long)]
    model: Option<String>,
}

pub(super) async fn ask(ctx: &Ctx, args: AskArgs) -> Result<()> {
    let paper = ctx.active_paper(&args.paper).await?;
    let model = pick_model(ctx.app.agent()?, args.model)?;
    let mut signals = Signals::install();
    let Some(question) = args.question else {
        let interactive = std::io::stdin().is_terminal();
        let ended = repl(ctx, &paper, &model, &mut signals, interactive).await;
        if interactive {
            eprintln!();
        }
        return ended;
    };
    match turn(ctx, &paper, &model, &question, &mut signals).await? {
        TurnEnd::Saved => Ok(()),
        TurnEnd::Failed(message) => bail!(message),
        TurnEnd::Interrupted(signal) => Err(Interrupted(signal).into()),
    }
}

fn pick_model(agent: &AgentService, wanted: Option<String>) -> Result<String> {
    let Some(wanted) = wanted else {
        return Ok(agent.default_backend().id.clone());
    };
    if agent.backend(&wanted).is_none() {
        let ids: Vec<&str> = agent.backends.iter().map(|b| b.id.as_str()).collect();
        bail!("unknown model {wanted} (available: {})", ids.join(", "));
    }
    Ok(wanted)
}

async fn repl(
    ctx: &Ctx,
    paper: &Paper,
    model: &str,
    signals: &mut Signals,
    interactive: bool,
) -> Result<()> {
    let mut questions = stdin_lines();
    loop {
        if interactive {
            eprint!("> ");
        }
        let line = tokio::select! {
            line = questions.recv() => line,
            signal = signals.recv() => return Err(Interrupted(signal).into()),
        };
        let Some(line) = line else {
            return Ok(());
        };
        let line = line.context("reading the question")?;
        let question = line.trim();
        if question.is_empty() {
            continue;
        }
        match turn(ctx, paper, model, question, signals).await? {
            TurnEnd::Saved => {}
            TurnEnd::Failed(message) => eprintln!("error: {message}"),
            TurnEnd::Interrupted(Signal::Interrupt) => eprintln!("(cancelled)"),
            TurnEnd::Interrupted(signal) => return Err(Interrupted(signal).into()),
        }
    }
}

/// Lines read on a plain thread: a blocked read must not hold up the
/// runtime's shutdown the way `tokio::io::stdin`'s blocking-pool read would.
fn stdin_lines() -> mpsc::Receiver<std::io::Result<String>> {
    let (tx, rx) = mpsc::channel(1);
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            if tx.blocking_send(line).is_err() {
                return;
            }
        }
    });
    rx
}

#[derive(Debug, PartialEq)]
enum TurnEnd {
    Saved,
    Failed(String),
    Interrupted(Signal),
}

async fn turn(
    ctx: &Ctx,
    paper: &Paper,
    model: &str,
    question: &str,
    signals: &mut Signals,
) -> Result<TurnEnd> {
    let request = AskRequest {
        model_id: model.to_string(),
        message: question.to_string(),
    };
    // Preparing the workspace extracts the PDF's text on the first ask, which
    // can take a while and has no timeout of its own.
    let events = tokio::select! {
        events = ctx.app.ask(paper, request) => events?,
        signal = signals.recv() => return Ok(TurnEnd::Interrupted(signal)),
    };
    drain(events, signals, &mut std::io::stdout()).await
}

/// Stream one answer to `out` and tool calls to stderr. Returning drops
/// `events`, which kills the agent's process group and saves nothing; the
/// exchange is saved only when the stream runs to `Done`.
async fn drain(
    events: impl Stream<Item = TurnEvent>,
    signals: &mut Signals,
    out: &mut impl std::io::Write,
) -> Result<TurnEnd> {
    futures_util::pin_mut!(events);
    let mut mid_line = false;
    let end = loop {
        let event = tokio::select! {
            event = events.next() => event,
            signal = signals.recv() => break TurnEnd::Interrupted(signal),
        };
        match event {
            Some(TurnEvent::Delta(text)) => {
                out.write_all(text.as_bytes())
                    .and_then(|()| out.flush())
                    .context("writing the answer")?;
                if !text.is_empty() {
                    mid_line = !text.ends_with('\n');
                }
            }
            Some(TurnEvent::Tool(ToolCall { name, detail })) => {
                if mid_line {
                    eprintln!();
                }
                eprintln!("[{name}] {detail}");
            }
            Some(TurnEvent::Done(_)) => break TurnEnd::Saved,
            Some(TurnEvent::Error(message)) => break TurnEnd::Failed(message),
            None => break TurnEnd::Failed("the agent stopped mid-answer".into()),
        }
    };
    if mid_line {
        writeln!(out).context("writing the answer")?;
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    #[tokio::test]
    async fn an_answer_goes_to_stdout_and_ends_its_line() {
        let events = stream::iter([
            TurnEvent::Delta("Hel".into()),
            TurnEvent::Delta("lo".into()),
            TurnEvent::Done(7),
        ]);
        let mut out = Vec::new();
        let end = drain(events, &mut Signals::install(), &mut out)
            .await
            .unwrap();
        assert_eq!(end, TurnEnd::Saved);
        assert_eq!(String::from_utf8(out).unwrap(), "Hello\n");
    }

    #[tokio::test]
    async fn an_agent_error_ends_the_turn_unsaved() {
        let events = stream::iter([
            TurnEvent::Delta("partial\n".into()),
            TurnEvent::Error("boom".into()),
        ]);
        let mut out = Vec::new();
        let end = drain(events, &mut Signals::install(), &mut out)
            .await
            .unwrap();
        assert_eq!(end, TurnEnd::Failed("boom".into()));
        assert_eq!(String::from_utf8(out).unwrap(), "partial\n");
    }

    #[tokio::test]
    async fn a_stream_that_just_stops_is_a_failure() {
        let events = stream::iter([TurnEvent::Delta("half".into())]);
        let mut out = Vec::new();
        let end = drain(events, &mut Signals::install(), &mut out)
            .await
            .unwrap();
        assert!(matches!(end, TurnEnd::Failed(_)), "{end:?}");
        assert_eq!(String::from_utf8(out).unwrap(), "half\n");
    }
}
