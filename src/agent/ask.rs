use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

use super::{history, store, AgentEvent, AgentService, TurnMessage, TurnPaper, TurnRequest};
use crate::library::model::Paper;

pub(crate) struct AskRequest {
    pub(crate) model_id: String,
    pub(crate) message: String,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) struct ToolCall {
    detail: String,
    name: String,
}

pub(crate) enum TurnEvent {
    Delta(String),
    Tool(ToolCall),
    Error(String),
    Done(i64),
}

pub(crate) enum AskError {
    Invalid(&'static str),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for AskError {
    fn from(e: anyhow::Error) -> Self {
        AskError::Internal(e)
    }
}

pub(crate) async fn start(
    agent: &Arc<AgentService>,
    pool: &SqlitePool,
    library_root: &Path,
    paper: &Paper,
    req: AskRequest,
) -> Result<impl Stream<Item = TurnEvent> + Send + 'static, AskError> {
    let Some(backend) = agent.backend(&req.model_id).cloned() else {
        return Err(AskError::Invalid("unknown model_id"));
    };
    let user_msg = req.message.trim().to_string();
    if user_msg.is_empty() {
        return Err(AskError::Invalid("empty message"));
    }
    let history = history::list(pool, &paper.id)
        .await
        .context("chat history for send")?;
    let ws = agent
        .ensure_workspace(library_root, paper)
        .await
        .context("agent workspace")?;
    // Gate on the DB record, not just disk: a detach clears the row, but a
    // clone job racing that detach can leave a stray `repo/` behind. Trusting
    // disk alone would expose that orphaned checkout; requiring status = 'ready'
    // (and that the directory is actually present) does not.
    let repo_ready = matches!(
        store::get_paper_code(pool, &paper.id).await,
        Ok(Some(c)) if c.status == store::CodeStatus::Ready
    );
    let has_repo = repo_ready
        && tokio::fs::try_exists(ws.join("repo"))
            .await
            .unwrap_or(false);
    let treq = TurnRequest {
        backend: backend.id.clone(),
        model: backend.model.clone(),
        workspace: ws.to_string_lossy().into_owned(),
        has_repo,
        paper: TurnPaper::from_paper(paper),
        transcript: history
            .iter()
            .map(|r| TurnMessage {
                role: r.role.clone(),
                content: r.content.clone(),
            })
            .collect(),
        question: user_msg.clone(),
    };
    let (agent, pool, paper_id, label) = (
        agent.clone(),
        pool.clone(),
        paper.id.clone(),
        backend.label.clone(),
    );

    Ok(async_stream::stream! {
        let upstream = agent.run_turn(treq);
        futures_util::pin_mut!(upstream);
        let mut full = String::new();
        let mut tools: Vec<ToolCall> = Vec::new();
        while let Some(ev) = upstream.next().await {
            match ev {
                AgentEvent::Delta { text } => {
                    full.push_str(&text);
                    yield TurnEvent::Delta(text);
                }
                AgentEvent::Tool { name, detail } => {
                    let call = ToolCall { detail, name };
                    tools.push(call.clone());
                    yield TurnEvent::Tool(call);
                }
                AgentEvent::Error { message } => {
                    yield TurnEvent::Error(message);
                    return;
                }
                AgentEvent::Done => break,
            }
        }
        if full.is_empty() {
            yield TurnEvent::Error("the agent returned an empty reply".into());
            return;
        }
        let tools_json = if tools.is_empty() {
            None
        } else {
            serde_json::to_string(&tools).ok()
        };
        // Client disconnects drop this stream before we get here, so
        // nothing is persisted for aborted generations.
        match history::insert_exchange(&pool, &paper_id, &user_msg, &full, &label, tools_json.as_deref()).await {
            Ok(assistant_id) => yield TurnEvent::Done(assistant_id),
            Err(e) => yield TurnEvent::Error(format!("saving the exchange failed: {e}")),
        }
    })
}
