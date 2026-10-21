use super::error::Error;
use super::{App, Trash};
use crate::library::trash;

impl App {
    /// Idempotent on a paper already in the trash.
    pub(crate) async fn trash_paper(&self, id: &str) -> Result<(), Error> {
        self.paper(id, Trash::Allow).await?;
        trash::soft_delete(self.pool(), id).await?;
        self.wake_search();
        Ok(())
    }

    /// `NotFound` when the paper doesn't exist or isn't trashed.
    pub(crate) async fn restore_paper(&self, id: &str) -> Result<(), Error> {
        if !trash::restore(self.pool(), id).await? {
            return Err(Error::not_found());
        }
        self.wake_search();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::pool_with_paper;

    #[tokio::test]
    async fn trash_and_restore_round_trip_and_mark_the_index_dirty() {
        let (pool, dir) = pool_with_paper("p1").await;
        let app = App::builder(pool.clone(), dir.path().to_path_buf()).build();

        app.trash_paper("p1").await.unwrap();
        assert!(app.take_index_dirty());
        app.trash_paper("p1").await.unwrap();
        assert!(app.paper("p1", Trash::Deny).await.is_err());

        app.restore_paper("p1").await.unwrap();
        assert!(app.take_index_dirty());
        assert!(matches!(
            app.restore_paper("p1").await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            app.trash_paper("nope").await,
            Err(Error::NotFound(_))
        ));
    }
}
