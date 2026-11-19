use futures_util::Stream;

use super::App;
use super::error::Error;
use crate::agent::ask::{self, AskRequest, TurnEvent};
use crate::library::model::Paper;

impl App {
    pub(crate) async fn ask(
        &self,
        paper: &Paper,
        req: AskRequest,
    ) -> Result<impl Stream<Item = TurnEvent> + Send + use<>, Error> {
        let agent = self.agent()?;
        Ok(ask::start(agent, self.pool(), self.library_root(), paper, req).await?)
    }
}
