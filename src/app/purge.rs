use std::io::ErrorKind;
use std::path::Path;

use super::App;
use super::error::Error;
use crate::library::model::Paper;
use crate::library::trash;

impl App {
    /// The row goes first: the schema's ON DELETE cascades take its chat and
    /// code rows with it, and a failed delete leaves the paper whole rather
    /// than pointing at files already gone. A file that then cannot be
    /// removed only warns — the paper itself is purged.
    pub async fn purge(&self, paper: &Paper) -> Result<(), Error> {
        trash::hard_delete(self.pool(), &paper.id).await?;
        self.library_changed();
        let pdf = self.library_root().join(&paper.rel_path);
        warn_unless_gone(&pdf, tokio::fs::remove_file(&pdf).await);
        let workspace = crate::agent::workspace_dir(self.library_root(), &paper.id);
        warn_unless_gone(&workspace, tokio::fs::remove_dir_all(&workspace).await);
        self.preview().forget(&paper.content_hash).await;
        Ok(())
    }
}

fn warn_unless_gone(path: &Path, removed: std::io::Result<()>) {
    if let Err(e) = removed
        && e.kind() != ErrorKind::NotFound
    {
        tracing::warn!("could not remove {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::papers;
    use crate::testutil::pool_with_paper;

    #[tokio::test]
    async fn a_failed_delete_leaves_the_files_in_place() {
        let (pool, dir) = pool_with_paper("p1").await;
        let paper = papers::get(&pool, "p1").await.unwrap().unwrap();
        let pdf = dir.path().join(&paper.rel_path);
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();
        let app = App::builder(pool.clone(), dir.path().to_path_buf()).build();

        pool.close().await;
        assert!(app.purge(&paper).await.is_err());
        assert!(pdf.exists(), "the paper's row still points at it");
    }

    #[tokio::test]
    async fn files_already_gone_do_not_block_a_purge() {
        let (pool, dir) = pool_with_paper("p1").await;
        let paper = papers::get(&pool, "p1").await.unwrap().unwrap();
        assert!(!dir.path().join(&paper.rel_path).exists());
        let app = App::builder(pool.clone(), dir.path().to_path_buf()).build();

        app.purge(&paper).await.unwrap();
        assert!(papers::get(&pool, "p1").await.unwrap().is_none());
        assert!(app.take_index_dirty());
    }
}
