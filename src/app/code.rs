use super::error::Error;
use super::App;
use crate::agent::store::PaperCode;
use crate::agent::{code, store};
use crate::library::model::Paper;

pub(crate) enum CloneMode {
    Background,
    Foreground,
}

impl App {
    pub(crate) async fn attach_code(
        &self,
        paper: &Paper,
        repo_url: &str,
        mode: CloneMode,
    ) -> Result<Option<PaperCode>, Error> {
        code::validate_repo_url(repo_url, &self.inner.clone_allowed_hosts)
            .map_err(Error::Unprocessable)?;
        let repo_url = repo_url.trim();
        let clone_gen = store::upsert_paper_code_cloning(self.pool(), &paper.id, repo_url).await?;
        let job = code::run_clone(
            self.pool().clone(),
            self.library_root().to_path_buf(),
            paper.id.clone(),
            repo_url.to_string(),
            self.inner.max_repo_mb,
            clone_gen,
        );
        match mode {
            CloneMode::Background => self.tasks().spawn_until_cancelled(job),
            CloneMode::Foreground => job.await,
        }
        Ok(store::get_paper_code(self.pool(), &paper.id).await?)
    }

    pub(crate) async fn detach_code(&self, paper: &Paper) -> Result<(), Error> {
        code::remove_checkout(self.library_root(), &paper.id).await;
        store::delete_paper_code(self.pool(), &paper.id).await?;
        Ok(())
    }
}
