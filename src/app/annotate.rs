use super::error::Error;
use super::{App, Trash};
use crate::annotations::{self, Annotation, NewAnnotation};

impl App {
    /// Trashed papers are accepted: annotations must survive a restore, and an
    /// open reader tab keeps saving.
    pub(crate) async fn put_annotation(
        &self,
        paper_id: &str,
        annotation_id: &str,
        new: NewAnnotation,
    ) -> Result<Annotation, Error> {
        annotations::validate_id(annotation_id).map_err(Error::Invalid)?;
        annotations::validate_new(&new).map_err(Error::Invalid)?;
        self.paper(paper_id, Trash::Allow).await?;
        let a = self.annotations().put(paper_id, annotation_id, new).await?;
        self.wake_search();
        Ok(a)
    }

    pub(crate) async fn delete_annotation(
        &self,
        paper_id: &str,
        annotation_id: &str,
    ) -> Result<(), Error> {
        if !self.annotations().delete(paper_id, annotation_id).await? {
            return Err(Error::not_found());
        }
        self.wake_search();
        Ok(())
    }

    pub(crate) async fn clear_annotations(&self, paper_id: &str) -> Result<u64, Error> {
        self.paper(paper_id, Trash::Allow).await?;
        let n = self.annotations().delete_all(paper_id).await?;
        self.wake_search();
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{AnnotationColor, AnnotationKind};
    use crate::testutil::pool_with_paper;

    fn note(page_index: i64) -> NewAnnotation {
        NewAnnotation {
            page_index,
            kind: AnnotationKind::TextComment,
            color: AnnotationColor::Amber,
            quoted_text: None,
            note: Some("check the proof".into()),
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn annotation_ops_validate_and_mark_the_index_dirty() {
        let (pool, dir) = pool_with_paper("p1").await;
        let app = App::builder(pool.clone(), dir.path().to_path_buf()).build();

        assert!(matches!(
            app.put_annotation("p1", " ", note(0)).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            app.put_annotation("p1", "a1", note(-1)).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            app.put_annotation("nope", "a1", note(0)).await,
            Err(Error::NotFound(_))
        ));
        assert!(!app.take_index_dirty(), "a refused write changes nothing");

        crate::library::trash::soft_delete(&pool, "p1")
            .await
            .unwrap();
        app.put_annotation("p1", "a1", note(2)).await.unwrap();
        assert!(app.take_index_dirty());

        app.delete_annotation("p1", "a1").await.unwrap();
        assert!(matches!(
            app.delete_annotation("p1", "a1").await,
            Err(Error::NotFound(_))
        ));
        app.put_annotation("p1", "a2", note(0)).await.unwrap();
        assert_eq!(app.clear_annotations("p1").await.unwrap(), 1);
        assert!(app.take_index_dirty());
    }
}
