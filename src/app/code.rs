use super::App;
use super::error::Error;
use crate::agent::store::PaperCode;
use crate::agent::{code, store};
use crate::library::model::Paper;

pub(crate) enum CloneMode {
    Background,
    Foreground,
}

impl App {
    /// Refused unless Agent Ask is configured: a checkout serves nothing else.
    pub(crate) async fn attach_code(
        &self,
        paper: &Paper,
        repo_url: &str,
        mode: CloneMode,
    ) -> Result<Option<PaperCode>, Error> {
        self.agent()?;
        let url = code::validate_repo_url(repo_url, &self.inner.clone_allowed_hosts)
            .map_err(Error::Unprocessable)?;
        let clone_gen =
            store::upsert_paper_code_cloning(self.pool(), &paper.id, url.as_str()).await?;
        let job = code::run_clone(
            self.pool().clone(),
            self.library_root().to_path_buf(),
            paper.id.clone(),
            url,
            self.inner.clone_limits,
            clone_gen,
        );
        match mode {
            CloneMode::Background => self.tasks().spawn_until_cancelled(job),
            CloneMode::Foreground => job.await,
        }
        Ok(store::get_paper_code(self.pool(), &paper.id).await?)
    }

    pub(crate) async fn detach_code(&self, paper: &Paper) -> Result<(), Error> {
        code::detach(self.pool(), self.library_root(), &paper.id).await?;
        Ok(())
    }
}
