use super::error::Error;
use super::App;
use crate::library::model::Paper;
use crate::library::trash;

impl App {
    /// The chat/code sidecar deletes stay explicit — belt-and-braces beside the
    /// schema's ON DELETE cascades, whose regressions they would otherwise mask
    /// (see tests/it/web/code.rs). PDF and workspace removal degrade to a
    /// warning/no-op: a missing file must not block a purge.
    pub async fn purge(&self, paper: &Paper) -> Result<(), Error> {
        let (pool, library_root) = (self.pool(), self.library_root());
        let path = library_root.join(&paper.rel_path);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
        }
        crate::agent::history::clear(pool, &paper.id).await?;
        crate::agent::store::delete_paper_code(pool, &paper.id).await?;
        let _ =
            tokio::fs::remove_dir_all(crate::agent::workspace_dir(library_root, &paper.id)).await;
        self.preview().forget(&paper.content_hash).await;
        trash::hard_delete(pool, &paper.id).await?;
        Ok(())
    }
}
