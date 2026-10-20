use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Arxiv(String),
    None,
}

impl Identifier {
    /// A DOI in canonical form (see `canonical_doi`). Every `Identifier` built
    /// from external input — PDF text, pasted import strings, identify
    /// arguments — goes through here, so stored identifiers compare equal
    /// across case variants and dedupe works.
    pub(crate) fn doi(s: impl Into<String>) -> Self {
        Identifier::Doi(canonical_doi(&s.into()))
    }

    /// An arXiv id in canonical form (see `canonical_arxiv`); same boundary
    /// rule as `Identifier::doi`.
    pub(crate) fn arxiv(s: impl Into<String>) -> Self {
        Identifier::Arxiv(canonical_arxiv(&s.into()))
    }
}

/// Canonical DOI: lowercase. DOIs compare case-insensitively by spec, but a
/// PDF banner prints whatever case the publisher chose, while Crossref returns
/// lowercase — stored as-is the two never matched (`papers::find_by_identifier`
/// additionally tolerates legacy mixed-case rows with COLLATE NOCASE).
pub(crate) fn canonical_doi(doi: &str) -> String {
    doi.to_ascii_lowercase()
}

/// Canonical arXiv id: the trailing `v<N>` version suffix is stripped —
/// `1706.03762v5` and `1706.03762` are the same work, and a versioned stored
/// id would never dedupe against a bare one (`papers::find_by_identifier`
/// additionally tolerates legacy versioned rows).
pub(crate) fn canonical_arxiv(id: &str) -> String {
    let id = id.trim();
    match id.rfind(['v', 'V']) {
        Some(i)
            if i > 0
                && !id[i + 1..].is_empty()
                && id[i + 1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            id[..i].to_string()
        }
        _ => id.to_string(),
    }
}

/// Author list stored as a JSON array in a nullable TEXT column.
/// NULL ⇄ empty; unparseable stored JSON decodes to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Authors(pub Vec<String>);

impl sqlx::Type<sqlx::Sqlite> for Authors {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for Authors {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let raw = <Option<&str> as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        // Garbage JSON degrades to empty silently: this runs per-row on every list
        // fetch (no paper id in scope, and a warn here would spam on each poll).
        Ok(Authors(
            raw.and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        ))
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for Authors {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        if self.0.is_empty() {
            return Ok(sqlx::encode::IsNull::Yes);
        }
        let json = serde_json::to_string(&self.0)?;
        <String as sqlx::Encode<sqlx::Sqlite>>::encode(json, buf)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type, ts_rs::TS)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PaperStatus {
    Resolved,
    NeedsReview,
}

/// The metadata block shared by resolution output and the stored record.
/// Column names match the `papers` table; flattened into `Paper` for sqlx/serde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct PaperMeta {
    pub title: Option<String>,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Authors,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: PaperStatus,
}

impl PaperMeta {
    /// Assemble a full `Paper` with a fresh id/timestamp and the given location.
    pub(crate) fn into_paper(
        self,
        content_hash: String,
        rel_path: String,
        cite_key: Option<String>,
    ) -> Paper {
        Paper {
            id: uuid::Uuid::now_v7().to_string(),
            content_hash,
            rel_path,
            cite_key,
            added_at: crate::db::now_rfc3339(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: self,
        }
    }
}

/// A stored bibliographic record. Column names match the papers table.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Paper {
    pub id: String,
    pub content_hash: String,
    pub rel_path: String,
    pub cite_key: Option<String>,
    pub added_at: String,
    pub deleted_at: Option<String>,
    #[sqlx(default)]
    pub starred: bool,
    /// Manual "known as" name (the system/tool the paper proposes, e.g.
    /// "RVSpec"). Lives outside `PaperMeta` on purpose: identify/refresh
    /// replace the whole metadata block, and this field must survive that.
    #[sqlx(default)]
    pub name: Option<String>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub meta: PaperMeta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    Invalid,
    TooLong { max: usize },
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NameError::Empty => f.write_str("empty name"),
            NameError::Invalid => f.write_str("name must not contain control characters"),
            NameError::TooLong { max } => write!(f, "name is too long (max {max} characters)"),
        }
    }
}

impl std::error::Error for NameError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_match_schema() {
        assert_eq!(
            serde_json::to_string(&PaperStatus::Resolved).unwrap(),
            "\"resolved\""
        );
        assert_eq!(
            serde_json::to_string(&PaperStatus::NeedsReview).unwrap(),
            "\"needs_review\""
        );
    }

    #[test]
    fn identifier_equality() {
        assert_eq!(
            Identifier::Doi("10.1/x".into()),
            Identifier::Doi("10.1/x".into())
        );
        assert_ne!(Identifier::Doi("10.1/x".into()), Identifier::None);
    }

    #[test]
    fn identifier_constructors_canonicalize() {
        assert_eq!(
            Identifier::doi("10.1145/AbC.XYZ"),
            Identifier::Doi("10.1145/abc.xyz".into())
        );
        assert_eq!(
            Identifier::arxiv("1706.03762v5"),
            Identifier::Arxiv("1706.03762".into())
        );
    }

    #[test]
    fn canonical_arxiv_strips_only_a_version_suffix() {
        assert_eq!(canonical_arxiv("1706.03762v5"), "1706.03762");
        assert_eq!(canonical_arxiv("1706.03762V12"), "1706.03762");
        assert_eq!(canonical_arxiv("1706.03762"), "1706.03762");
        // Old-style ids: a 'v' inside the archive name is not a version.
        assert_eq!(canonical_arxiv("cs.CV/9901002"), "cs.CV/9901002");
        assert_eq!(canonical_arxiv("math.NA/0303029v1"), "math.NA/0303029");
    }
}
