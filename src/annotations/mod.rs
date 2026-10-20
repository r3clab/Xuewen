//! Reader annotations: highlights, underlines, strikeouts, squigglies and
//! sticky notes drawn over a paper's PDF.
//!
//! The PDF file itself is never written. `@embedpdf/plugin-annotation` runs
//! with `autoCommit: false` in the frontend and the marks live here instead,
//! so `papers.content_hash` keeps matching the stored bytes.
//!
//! Each row is a *projection* (`kind`, `color`, `page_index`, `quoted_text`,
//! `note`) over an opaque `payload` — the plugin's own `AnnotationTransferItem`,
//! stored verbatim. The projection is what SQL, the CLI and the search
//! indexer read; the payload is what the reader replays to redraw the mark.
//! Splitting them this way means a field this module has never heard of
//! survives a save/load round trip instead of being silently dropped.

pub(crate) mod store;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

/// The mark types the reader can create. Mirrors the subset of
/// `PdfAnnotationSubtype` our tools produce — deliberately closed, so an
/// annotation subtype we don't render can never reach the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, TS)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum AnnotationKind {
    Highlight,
    Underline,
    Strikeout,
    Squiggly,
    TextComment,
}

/// The fixed reader palette. A closed enum rather than a stored hex string on
/// purpose: the rendered color has to differ between light mode and the dark
/// `dim`/`invert` page filters, so the *semantic* color is what persists and
/// the frontend resolves it to pixels at draw time. Restyling the palette
/// later therefore reflows every existing mark with no migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, TS)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum AnnotationColor {
    Amber,
    Rose,
    Green,
    Blue,
    Violet,
}

impl AnnotationKind {
    /// The stored/wire spelling. Kept in step with the serde and sqlx
    /// renames by `names_match_the_wire_spelling` below.
    fn as_str(&self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Strikeout => "strikeout",
            Self::Squiggly => "squiggly",
            Self::TextComment => "text_comment",
        }
    }
}

impl std::fmt::Display for AnnotationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AnnotationColor {
    /// The stored/wire spelling.
    fn as_str(&self) -> &'static str {
        match self {
            Self::Amber => "amber",
            Self::Rose => "rose",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Violet => "violet",
        }
    }
}

impl std::fmt::Display for AnnotationColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored annotation. Column names match the `annotations` table.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub(crate) struct Annotation {
    paper_id: String,
    /// The plugin's own annotation id — unique within one paper, not globally.
    pub(crate) id: String,
    pub(crate) page_index: i64,
    pub(crate) kind: AnnotationKind,
    pub(crate) color: AnnotationColor,
    pub(crate) quoted_text: Option<String>,
    pub(crate) note: Option<String>,
    /// Verbatim `AnnotationTransferItem`. `null` when the stored JSON failed
    /// to parse — the row still lists (its projection is intact) but the
    /// reader skips redrawing it rather than crashing the whole document.
    #[ts(type = "unknown")]
    payload: serde_json::Value,
    created_at: String,
    updated_at: String,
}

/// A create-or-replace request. The id is not in here: it is assigned by the
/// frontend plugin and travels as the address (the last path segment), which
/// is what makes the write idempotent under a retried save.
#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[ts(export)]
pub(crate) struct NewAnnotation {
    pub(crate) page_index: i64,
    pub(crate) kind: AnnotationKind,
    pub(crate) color: AnnotationColor,
    #[serde(default)]
    pub(crate) quoted_text: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[ts(type = "unknown")]
    pub(crate) payload: serde_json::Value,
}

/// Longest accepted note or quoted-text field. Generous for commentary while
/// still bounding what one mark can push into the search index.
const MAX_TEXT_LEN: usize = 10_000;
/// Longest accepted serialized payload. A multi-page highlight's
/// `segmentRects` is the realistic worst case and is far below this.
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
/// Longest accepted annotation id.
const MAX_ID_LEN: usize = 128;

/// Validate an annotation id from the request path.
pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("annotation id must not be empty".into());
    }
    if id.len() > MAX_ID_LEN {
        return Err(format!("annotation id must be at most {MAX_ID_LEN} bytes"));
    }
    Ok(())
}

/// Validate a create-or-replace body. Returns the message for a 400, or
/// `Ok(())`. Pure so the web handler and any future caller share one rule set.
pub(crate) fn validate_new(a: &NewAnnotation) -> Result<(), String> {
    if a.page_index < 0 {
        return Err("page_index must not be negative".into());
    }
    check_text("quoted_text", a.quoted_text.as_deref())?;
    check_text("note", a.note.as_deref())?;
    check_payload(&a.payload)
}

fn check_text(field: &str, v: Option<&str>) -> Result<(), String> {
    match v {
        Some(s) if s.chars().count() > MAX_TEXT_LEN => {
            Err(format!("{field} must be at most {MAX_TEXT_LEN} characters"))
        }
        _ => Ok(()),
    }
}

fn check_payload(v: &serde_json::Value) -> Result<(), String> {
    if !v.is_object() {
        return Err("payload must be a JSON object".into());
    }
    let len = serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX);
    if len > MAX_PAYLOAD_BYTES {
        return Err(format!("payload must be at most {MAX_PAYLOAD_BYTES} bytes"));
    }
    Ok(())
}

/// Annotation persistence. Always available — annotations need no `[ai.*]`
/// configuration, so unlike the LLM-backed services this is never `Option`al
/// on `App`. The type exists to keep timestamp stamping and storage in
/// one place, so the web API and the CLI cannot drift apart. Input validation
/// is the handlers' job — the pure `validate_*` functions above are the one
/// rule set they share.
pub(crate) struct AnnotationsService {
    pool: SqlitePool,
}

impl AnnotationsService {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// A paper's annotations in reading order (page, then creation).
    pub(crate) async fn list(&self, paper_id: &str) -> Result<Vec<Annotation>> {
        store::list_for_paper(&self.pool, paper_id).await
    }

    /// Create, or overwrite an existing mark with the same id. Idempotent so
    /// a retried save cannot produce a duplicate; `created_at` is preserved
    /// across the overwrite.
    pub(crate) async fn put(
        &self,
        paper_id: &str,
        id: &str,
        new: NewAnnotation,
    ) -> Result<Annotation> {
        let now = chrono::Utc::now().to_rfc3339();
        store::upsert(&self.pool, paper_id, id, &new, &now).await
    }

    /// Whether a row was actually removed.
    pub(crate) async fn delete(&self, paper_id: &str, id: &str) -> Result<bool> {
        store::delete(&self.pool, paper_id, id).await
    }

    /// Drop every annotation on a paper; returns how many were removed.
    pub(crate) async fn delete_all(&self, paper_id: &str) -> Result<u64> {
        store::delete_all_for_paper(&self.pool, paper_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> serde_json::Value {
        serde_json::json!({ "annotation": { "type": 9 } })
    }

    fn new_annotation() -> NewAnnotation {
        NewAnnotation {
            page_index: 0,
            kind: AnnotationKind::Highlight,
            color: AnnotationColor::Amber,
            quoted_text: Some("some text".into()),
            note: None,
            payload: payload(),
        }
    }

    #[test]
    fn kind_and_color_serialize_as_the_stored_strings() {
        assert_eq!(
            serde_json::to_string(&AnnotationKind::TextComment).unwrap(),
            "\"text_comment\""
        );
        assert_eq!(
            serde_json::to_string(&AnnotationKind::Strikeout).unwrap(),
            "\"strikeout\""
        );
        assert_eq!(
            serde_json::to_string(&AnnotationColor::Violet).unwrap(),
            "\"violet\""
        );
    }

    #[test]
    fn names_match_the_wire_spelling() {
        // `as_str` is hand-written while serde/sqlx derive theirs from
        // rename_all; this pins them together so a renamed variant can't
        // print one thing and store another.
        for k in [
            AnnotationKind::Highlight,
            AnnotationKind::Underline,
            AnnotationKind::Strikeout,
            AnnotationKind::Squiggly,
            AnnotationKind::TextComment,
        ] {
            assert_eq!(serde_json::to_string(&k).unwrap(), format!("\"{k}\""));
        }
        for c in [
            AnnotationColor::Amber,
            AnnotationColor::Rose,
            AnnotationColor::Green,
            AnnotationColor::Blue,
            AnnotationColor::Violet,
        ] {
            assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{c}\""));
        }
    }

    #[test]
    fn unknown_kind_or_color_is_rejected_at_the_wire() {
        // The closed enums are the guard that keeps an untyped subtype out of
        // the database; deserialization must fail rather than default.
        assert!(serde_json::from_str::<AnnotationKind>("\"ink\"").is_err());
        assert!(serde_json::from_str::<AnnotationColor>("\"chartreuse\"").is_err());
    }

    #[test]
    fn accepts_a_well_formed_create() {
        assert!(validate_new(&new_annotation()).is_ok());
    }

    #[test]
    fn rejects_a_blank_or_overlong_id() {
        assert!(validate_id("a1").is_ok());
        assert!(validate_id("  ").unwrap_err().contains("empty"));
        assert!(validate_id(&"x".repeat(MAX_ID_LEN + 1))
            .unwrap_err()
            .contains("at most"));
    }

    #[test]
    fn rejects_a_negative_page() {
        let bad = NewAnnotation {
            page_index: -1,
            ..new_annotation()
        };
        assert!(validate_new(&bad).unwrap_err().contains("page_index"));
    }

    #[test]
    fn rejects_oversized_text_and_non_object_payload() {
        let bad = NewAnnotation {
            note: Some("x".repeat(MAX_TEXT_LEN + 1)),
            ..new_annotation()
        };
        assert!(validate_new(&bad).unwrap_err().contains("note"));
        let bad = NewAnnotation {
            payload: serde_json::json!([1, 2, 3]),
            ..new_annotation()
        };
        assert!(validate_new(&bad).unwrap_err().contains("payload"));
    }

    #[test]
    fn text_limit_counts_characters_not_bytes() {
        // A multi-byte note at exactly the limit must pass — counting bytes
        // would reject a perfectly ordinary CJK note less than half as long.
        let ok = NewAnnotation {
            note: Some("學".repeat(MAX_TEXT_LEN)),
            ..new_annotation()
        };
        assert!(validate_new(&ok).is_ok());
    }
}
