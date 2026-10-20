mod annotations;
mod chat;
mod citations;
mod code;
mod daily;
mod identify;
mod import;
mod papers;
mod preview;
mod projects;
mod search;
mod settings;
mod tags;
mod translate;

use axum::Router;

use super::error::ApiError;
use crate::app::{App, Trash};

pub(super) fn router() -> Router<App> {
    Router::new()
        .merge(papers::routes())
        .merge(import::routes())
        .merge(identify::routes())
        .merge(preview::routes())
        .merge(settings::routes())
        .merge(projects::routes())
        .merge(tags::routes())
        .merge(search::routes())
        .merge(daily::routes())
        .merge(chat::routes())
        .merge(citations::routes())
        .merge(annotations::routes())
        .merge(code::routes())
        .merge(translate::routes())
        .fallback(super::assets::static_handler)
}

pub(super) async fn fetch_paper(
    app: &App,
    id: &str,
    trash: Trash,
    ctx: &'static str,
) -> Result<crate::library::model::Paper, ApiError> {
    app.paper(id, trash)
        .await
        .map_err(|e| ApiError::context(format!("{ctx} paper lookup"))(e))
}
