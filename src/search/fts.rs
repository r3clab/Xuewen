use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tantivy::collector::TopDocs;
use tantivy::directory::error::LockError;
use tantivy::directory::{Directory, DirectoryLock, MmapDirectory, INDEX_WRITER_LOCK, META_LOCK};
use tantivy::query::{
    BooleanQuery, Occur, PhraseQuery, Query, QueryParser, TermQuery, TermSetQuery,
};
use tantivy::schema::{Field, IndexRecordOption, Schema, Value, STORED, STRING, TEXT};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::TokenStream;
use tantivy::{doc, Index, IndexReader, IndexWriter, TantivyDocument, TantivyError, Term};
use ts_rs::TS;

use super::snippet::Snippet;

const WRITER_HEAP_BYTES: usize = 50_000_000;

/// Outcome of trying to take the index's single-writer lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriterClaim {
    Claimed,
    Busy,
}

/// Which paper fields a query runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSel {
    pub(crate) title: bool,
    pub(crate) authors: bool,
    pub(crate) abstract_text: bool,
    pub(crate) body: bool,
    /// The reader's own annotation notes.
    pub(crate) notes: bool,
}

impl FieldSel {
    pub fn all() -> Self {
        Self {
            title: true,
            authors: true,
            abstract_text: true,
            body: true,
            notes: true,
        }
    }

    /// Nothing selected — the starting point for `parse` and for `in:` tokens.
    pub(crate) fn none() -> Self {
        Self {
            title: false,
            authors: false,
            abstract_text: false,
            body: false,
            notes: false,
        }
    }

    /// Parse a `fields=title,body` CSV. Absent, empty, or all-unknown input
    /// falls back to every field (unknown values are ignored, never an error).
    pub(crate) fn parse(csv: Option<&str>) -> Self {
        let mut sel = Self::none();
        for part in csv.unwrap_or("").split(',').map(str::trim) {
            sel.select(part);
        }
        if sel.any() {
            sel
        } else {
            Self::all()
        }
    }

    /// Turn on the field `name` addresses. Returns whether it named one, so
    /// `in:` can fall back to free text for anything unrecognized. Shared with
    /// `parse` so the CSV and the query qualifier can never accept different
    /// spellings.
    pub(crate) fn select(&mut self, name: &str) -> bool {
        match name {
            "title" => self.title = true,
            "authors" => self.authors = true,
            "abstract" => self.abstract_text = true,
            "body" => self.body = true,
            "notes" => self.notes = true,
            _ => return false,
        }
        true
    }

    pub(crate) fn any(&self) -> bool {
        self.title || self.authors || self.abstract_text || self.body || self.notes
    }

    /// Whether any field the vector tier actually holds is selected. Only
    /// title/abstract/body are chunked and embedded — `authors` and `notes`
    /// never are — so a query scoped to those alone has nothing for semantic
    /// search to match and should say so rather than silently return nothing.
    pub(crate) fn semantic_applicable(&self) -> bool {
        self.title || self.abstract_text || self.body
    }
}

/// One paper as a Tantivy document (all fields stored for snippets).
#[derive(Debug, Clone, Default)]
pub struct PaperDoc {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub venue: String,
    pub abstract_text: String,
    pub body: String,
    /// The paper's annotation notes, newline-joined in reading order.
    pub notes: String,
}

/// The searchable text fields a match can be attributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "SearchField")]
pub enum FieldName {
    Title,
    Authors,
    Abstract,
    Body,
    Notes,
}

impl FieldName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FieldName::Title => "title",
            FieldName::Authors => "authors",
            FieldName::Abstract => "abstract",
            FieldName::Body => "body",
            FieldName::Notes => "notes",
        }
    }
}

impl std::fmt::Display for FieldName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FtsHit {
    pub(crate) paper_id: String,
    /// Which field the snippet came from.
    pub(crate) field: FieldName,
    pub(crate) snippet: Snippet,
}

struct FtsFields {
    id: Field,
    title: Field,
    authors: Field,
    venue: Field,
    abstract_text: Field,
    body: Field,
    notes: Field,
}

/// Bumped whenever the Tantivy schema changes. Tantivy cannot add a field to
/// an index that already exists, so an index written before a bump is wiped
/// and re-swept. Stamping the version ourselves makes that deterministic
/// instead of depending on how `Index::open_or_create` happens to fail.
const SCHEMA_VERSION: u32 = 2;
const VERSION_FILE: &str = "xuewen-schema-version";

fn stored_schema_version(dir: &Path) -> Option<u32> {
    std::fs::read_to_string(dir.join(VERSION_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Tantivy's writer lock on an index directory, taken without opening the
/// index — the one way to hold it over an index this build cannot open, so
/// that only its holder ever wipes the directory.
pub(crate) struct DirWriterLock {
    dir: PathBuf,
    _lock: DirectoryLock,
}

impl DirWriterLock {
    /// Fails with "in use" when another `FtsIndex` — in this process or
    /// another — holds the writer.
    pub(crate) fn acquire(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        match MmapDirectory::open(dir)?.acquire_lock(&INDEX_WRITER_LOCK) {
            Ok(lock) => Ok(Self {
                dir: dir.to_path_buf(),
                _lock: lock,
            }),
            Err(LockError::LockBusy) => bail!(
                "search index at {} is in use by another xuewen process — stop it and retry",
                dir.display()
            ),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete the index, leaving an empty directory (lock files aside, so the
    /// lock stays meaningful to anyone opening it meanwhile).
    pub(crate) fn wipe(self) -> Result<()> {
        let locks = [&INDEX_WRITER_LOCK.filepath, &META_LOCK.filepath];
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if locks.iter().any(|l| entry.file_name() == l.as_os_str()) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

pub struct FtsIndex {
    index: Index,
    /// Lazy: read-only users (CLI search while `serve` runs) must not take
    /// Tantivy's single-writer lock.
    writer: Mutex<Option<IndexWriter>>,
    reader: IndexReader,
    f: FtsFields,
    #[cfg(test)]
    commits: std::sync::atomic::AtomicUsize,
}

impl FtsIndex {
    /// Open (or create) the index at `dir`. Returns `(index, created)`; when
    /// `created` the caller must clear all FTS stamps so the sweep re-indexes
    /// everything. An index this build cannot use — an older schema version,
    /// or one that fails to open — is derived data and is recreated, but only
    /// under the writer lock: while another process holds it, this errors
    /// rather than pull the index out from under that process.
    pub fn open(dir: &Path) -> Result<(Self, bool)> {
        std::fs::create_dir_all(dir)?;
        if !dir.join("meta.json").exists() {
            return Ok((Self::create(dir)?, true));
        }
        let unusable = if stored_schema_version(dir) == Some(SCHEMA_VERSION) {
            match Self::try_open(dir) {
                Ok(idx) => return Ok((idx, false)),
                Err(e) => format!("is unusable ({e})"),
            }
        } else {
            format!("predates schema v{SCHEMA_VERSION}")
        };
        tracing::info!("tantivy index at {} {unusable}; rebuilding", dir.display());
        let lock = DirWriterLock::acquire(dir)?;
        lock.wipe()?;
        Ok((Self::create(dir)?, true))
    }

    fn create(dir: &Path) -> Result<Self> {
        let idx = Self::try_open(dir)?;
        // Best-effort: a missing stamp costs one extra rebuild on the next
        // open, which is far better than refusing to serve search at all.
        if let Err(e) = std::fs::write(dir.join(VERSION_FILE), SCHEMA_VERSION.to_string()) {
            tracing::warn!("could not stamp the tantivy schema version: {e}");
        }
        Ok(idx)
    }

    fn try_open(dir: &Path) -> Result<Self> {
        let mut b = Schema::builder();
        let id = b.add_text_field("paper_id", STRING | STORED);
        let title = b.add_text_field("title", TEXT | STORED);
        let authors = b.add_text_field("authors", TEXT | STORED);
        let venue = b.add_text_field("venue", TEXT | STORED);
        let abstract_text = b.add_text_field("abstract", TEXT | STORED);
        let body = b.add_text_field("body", TEXT | STORED);
        let notes = b.add_text_field("notes", TEXT | STORED);
        let schema = b.build();
        let index = Index::open_or_create(MmapDirectory::open(dir)?, schema)?;
        let reader = index.reader()?;
        Ok(Self {
            index,
            writer: Mutex::new(None),
            reader,
            #[cfg(test)]
            commits: Default::default(),
            f: FtsFields {
                id,
                title,
                authors,
                venue,
                abstract_text,
                body,
                notes,
            },
        })
    }

    /// Take tantivy's single-writer lock now rather than at the first write.
    /// `Busy` when another `FtsIndex` — in this process or another — holds
    /// it; idempotent once claimed.
    pub(crate) fn claim_writer(&self) -> Result<WriterClaim> {
        let mut guard = self.writer.lock().expect("fts writer lock poisoned");
        if guard.is_none() {
            match self.index.writer(WRITER_HEAP_BYTES) {
                Ok(w) => *guard = Some(w),
                Err(TantivyError::LockFailure(LockError::LockBusy, _)) => {
                    return Ok(WriterClaim::Busy)
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(WriterClaim::Claimed)
    }

    /// Give the writer lock back so another process can index. Writes staged
    /// since the last `commit` are discarded.
    pub(crate) fn release_writer(&self) {
        *self.writer.lock().expect("fts writer lock poisoned") = None;
    }

    fn with_writer<T>(&self, op: impl FnOnce(&mut IndexWriter) -> Result<T>) -> Result<T> {
        let mut guard = self.writer.lock().expect("fts writer lock poisoned");
        if guard.is_none() {
            *guard = Some(self.index.writer(WRITER_HEAP_BYTES)?);
        }
        op(guard.as_mut().expect("writer just created"))
    }

    /// Replace the paper's doc. Invisible to searches until `commit`.
    pub(crate) fn stage_upsert(&self, d: &PaperDoc) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, &d.id));
            w.add_document(doc!(
                self.f.id => d.id.clone(),
                self.f.title => d.title.clone(),
                self.f.authors => d.authors.clone(),
                self.f.venue => d.venue.clone(),
                self.f.abstract_text => d.abstract_text.clone(),
                self.f.body => d.body.clone(),
                self.f.notes => d.notes.clone(),
            ))?;
            Ok(())
        })
    }

    /// Remove the paper's doc. Invisible to searches until `commit`.
    pub(crate) fn stage_delete(&self, paper_id: &str) -> Result<()> {
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(self.f.id, paper_id));
            Ok(())
        })
    }

    /// Persist everything staged since the last commit and make it visible to
    /// the next search. On failure the staged writes are rolled back, so the
    /// next commit cannot half-apply them.
    pub(crate) fn commit(&self) -> Result<()> {
        #[cfg(test)]
        self.commits
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.with_writer(|w| match w.commit() {
            Ok(_) => Ok(()),
            Err(e) => {
                w.rollback()?;
                Err(e.into())
            }
        })?;
        self.reader.reload()?;
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn upsert(&self, d: &PaperDoc) -> Result<()> {
        self.stage_upsert(d)?;
        self.commit()
    }

    #[cfg(test)]
    pub(crate) fn delete(&self, paper_id: &str) -> Result<()> {
        self.stage_delete(paper_id)?;
        self.commit()
    }

    #[cfg(test)]
    pub(crate) fn commits(&self) -> usize {
        self.commits.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Search the free text `q` over the selected fields, ANDed with every
    /// `author:` term as a phrase on the authors field. `scope`, when given,
    /// restricts matching to those paper ids INSIDE the engine (ANDed as a
    /// `TermSetQuery`), so the top-`limit` truncation happens on the filtered
    /// set — applying a filter after the cutoff would silently drop matches
    /// ranked past it.
    pub(crate) fn search(
        &self,
        q: &str,
        authors: &[String],
        sel: &FieldSel,
        limit: usize,
        scope: Option<&[String]>,
    ) -> Result<Vec<FtsHit>> {
        let q = q.trim();
        if !sel.any() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut clauses = Vec::new();
        for author in authors {
            clauses.extend(self.author_phrase(author)?);
        }
        if !q.is_empty() {
            clauses.push(self.text_query(q, sel));
        }
        let query: Box<dyn Query> = match clauses.len() {
            0 => return Ok(Vec::new()),
            1 => clauses.remove(0),
            _ => Box::new(BooleanQuery::new(
                clauses.into_iter().map(|c| (Occur::Must, c)).collect(),
            )),
        };
        let scoped: Option<BooleanQuery> = scope.map(|ids| {
            let terms =
                TermSetQuery::new(ids.iter().map(|id| Term::from_field_text(self.f.id, id)));
            BooleanQuery::new(vec![
                (Occur::Must, query.box_clone()),
                (Occur::Must, Box::new(terms) as Box<dyn Query>),
            ])
        });
        let effective: &dyn Query = match &scoped {
            Some(s) => s,
            None => query.as_ref(),
        };

        let searcher = self.reader.searcher();
        // tantivy 0.26: TopDocs no longer implements Collector directly; the
        // ordering must be chosen explicitly (`.order_by_score()`), which
        // yields the same `(Score, DocAddress)` fruit as before.
        let top = searcher.search(effective, &TopDocs::with_limit(limit).order_by_score())?;
        let mut out = Vec::with_capacity(top.len());
        for (_, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let paper_id = doc
                .get_first(self.f.id)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (field, snippet) = self.best_snippet(&searcher, query.as_ref(), &doc, sel)?;
            out.push(FtsHit {
                paper_id,
                field,
                snippet,
            });
        }
        Ok(out)
    }

    /// The first selected field (title > authors > abstract > notes > body)
    /// with a highlighted fragment; falls back to the plain title text.
    /// Notes outrank body because a matched note is short, deliberate and
    /// already about this paper — a better thing to show than a body fragment.
    fn best_snippet(
        &self,
        searcher: &tantivy::Searcher,
        query: &dyn Query,
        doc: &TantivyDocument,
        sel: &FieldSel,
    ) -> Result<(FieldName, Snippet)> {
        let candidates: [(FieldName, Field, bool); 5] = [
            (FieldName::Title, self.f.title, sel.title),
            (FieldName::Authors, self.f.authors, sel.authors),
            (FieldName::Abstract, self.f.abstract_text, sel.abstract_text),
            (FieldName::Notes, self.f.notes, sel.notes),
            (FieldName::Body, self.f.body, sel.body),
        ];
        for (name, field, enabled) in candidates {
            if !enabled {
                continue;
            }
            let mut gen = SnippetGenerator::create(searcher, query, field)?;
            gen.set_max_num_chars(200);
            if let Some(snippet) = Snippet::highlighted(&gen.snippet_from_doc(doc)) {
                return Ok((name, snippet));
            }
        }
        let title = doc
            .get_first(self.f.title)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok((FieldName::Title, Snippet::plain(title)))
    }

    fn text_query(&self, q: &str, sel: &FieldSel) -> Box<dyn Query> {
        let fields = [
            (sel.title, self.f.title),
            (sel.authors, self.f.authors),
            (sel.abstract_text, self.f.abstract_text),
            (sel.body, self.f.body),
            (sel.notes, self.f.notes),
        ]
        .into_iter()
        .filter_map(|(on, field)| on.then_some(field))
        .collect();
        let mut parser = QueryParser::for_index(&self.index, fields);
        parser.set_field_boost(self.f.title, 3.0);
        parser.set_field_boost(self.f.authors, 2.0);
        // Above the abstract: a note is the reader's own words about this
        // paper, so matching one is a stronger signal than matching prose the
        // publisher wrote.
        parser.set_field_boost(self.f.notes, 2.5);
        parser.set_field_boost(self.f.abstract_text, 1.5);
        // Lenient: user input must never be a query syntax error.
        parser.parse_query_lenient(q).0
    }

    /// `author` as a phrase over the authors field, tokenized the way that
    /// field was indexed; `None` when it holds nothing searchable.
    fn author_phrase(&self, author: &str) -> Result<Option<Box<dyn Query>>> {
        let mut analyzer = self.index.tokenizer_for_field(self.f.authors)?;
        let mut stream = analyzer.token_stream(author);
        let mut terms = Vec::new();
        while stream.advance() {
            terms.push(Term::from_field_text(self.f.authors, &stream.token().text));
        }
        Ok(match terms.len() {
            0 => None,
            1 => Some(Box::new(TermQuery::new(
                terms.remove(0),
                IndexRecordOption::WithFreqs,
            ))),
            _ => Some(Box::new(PhraseQuery::new(terms))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> PaperDoc {
        PaperDoc {
            id: id.into(),
            title: title.into(),
            authors: "Ada Lovelace ; Alan Turing".into(),
            venue: "USENIX Security".into(),
            abstract_text: "We defend binaries against automated analysis.".into(),
            body: body.into(),
            notes: String::new(),
        }
    }

    fn open_tmp() -> (FtsIndex, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
        (idx, dir)
    }

    #[test]
    fn parse_field_selection() {
        assert!(FieldSel::parse(None).title);
        let s = FieldSel::parse(Some("authors,body"));
        assert!(!s.title && s.authors && !s.abstract_text && s.body);
        // Unknown-only input falls back to all (never an error).
        assert!(FieldSel::parse(Some("bogus")).title);
        assert!(FieldSel::parse(Some("notes")).notes);
        // Neither authors nor notes is embedded, so neither reaches the
        // vector tier — alone or together.
        assert!(!FieldSel::parse(Some("authors")).semantic_applicable());
        assert!(!FieldSel::parse(Some("authors,notes")).semantic_applicable());
        assert!(FieldSel::parse(Some("authors,title")).semantic_applicable());
    }

    #[test]
    fn the_dir_writer_lock_is_the_index_writer_lock() {
        let dir = tempfile::tempdir().unwrap();
        let (idx, _) = FtsIndex::open(dir.path()).unwrap();
        assert_eq!(idx.claim_writer().unwrap(), WriterClaim::Claimed);
        let err = DirWriterLock::acquire(dir.path()).err().unwrap();
        assert!(err.to_string().contains("in use"), "got: {err}");

        idx.release_writer();
        let lock = DirWriterLock::acquire(dir.path()).unwrap();
        assert_eq!(idx.claim_writer().unwrap(), WriterClaim::Busy);
        drop(lock);
        assert_eq!(idx.claim_writer().unwrap(), WriterClaim::Claimed);
    }

    #[test]
    fn wiping_leaves_an_empty_index_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (idx, _) = FtsIndex::open(dir.path()).unwrap();
        idx.upsert(&doc("p1", "A Title", "body")).unwrap();
        drop(idx);

        DirWriterLock::acquire(dir.path()).unwrap().wipe().unwrap();
        assert!(!dir.path().join("meta.json").exists());
        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
        assert!(idx
            .search("title", &[], &FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
    }

    /// The writer holder may be a running server: an index this build cannot
    /// use is left alone until it lets go.
    #[test]
    fn a_stale_index_another_writer_holds_is_an_error_not_a_wipe() {
        let dir = tempfile::tempdir().unwrap();
        let (holder, _) = FtsIndex::open(dir.path()).unwrap();
        holder.upsert(&doc("p1", "A Title", "body")).unwrap();
        std::fs::write(dir.path().join(VERSION_FILE), b"1").unwrap();

        let err = FtsIndex::open(dir.path()).err().unwrap();
        assert!(err.to_string().contains("in use"), "got: {err}");
        assert_eq!(
            holder
                .search("title", &[], &FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1,
            "the holder's index is untouched"
        );

        drop(holder);
        let (_, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
    }

    #[test]
    fn staged_writes_are_invisible_until_committed() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "Old Title", "b")).unwrap();
        idx.stage_upsert(&doc("p1", "New Title", "b")).unwrap();
        idx.stage_upsert(&doc("p2", "Another Title", "b")).unwrap();
        let titled = |q: &str| {
            idx.search(q, &[], &FieldSel::all(), 10, None)
                .unwrap()
                .len()
        };
        assert_eq!((titled("old"), titled("new"), titled("another")), (1, 0, 0));

        idx.commit().unwrap();
        assert_eq!((titled("old"), titled("new"), titled("another")), (0, 1, 1));

        idx.stage_delete("p2").unwrap();
        idx.release_writer();
        assert_eq!(
            titled("another"),
            1,
            "releasing the writer drops staged work"
        );
    }

    #[test]
    fn author_terms_are_phrases_anded_with_the_free_text() {
        let (idx, _dir) = open_tmp();
        let mut by_smith = doc("smith", "Sorting Networks", "batcher merge");
        by_smith.authors = "Jane Smith ; Ada Lovelace".into();
        let mut smith_sorting = doc("other", "Sorting Revisited", "thanks to jane smith");
        smith_sorting.authors = "Smith Jane".into();
        idx.upsert(&by_smith).unwrap();
        idx.upsert(&smith_sorting).unwrap();
        idx.upsert(&doc("unrelated", "Sorting Survey", "nothing"))
            .unwrap();

        let ids = |authors: &[&str], q: &str| -> Vec<String> {
            let authors: Vec<String> = authors.iter().map(|a| a.to_string()).collect();
            let mut ids: Vec<String> = idx
                .search(q, &authors, &FieldSel::all(), 10, None)
                .unwrap()
                .into_iter()
                .map(|h| h.paper_id)
                .collect();
            ids.sort();
            ids
        };
        assert_eq!(
            ids(&["jane smith"], ""),
            vec!["smith"],
            "a phrase, in order"
        );
        assert_eq!(ids(&["smith"], ""), vec!["other", "smith"]);
        assert_eq!(ids(&["smith"], "batcher"), vec!["smith"], "ANDed, not ORed");
        assert_eq!(ids(&["smith", "lovelace"], ""), vec!["smith"]);
        assert!(ids(&["nobody"], "sorting").is_empty());
        assert!(
            ids(&["!!!"], "").is_empty(),
            "nothing searchable matches nothing"
        );
    }

    #[test]
    fn claim_writer_reports_a_held_lock_as_busy_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (first, _) = FtsIndex::open(dir.path()).unwrap();
        let (second, _) = FtsIndex::open(dir.path()).unwrap();

        assert_eq!(first.claim_writer().unwrap(), WriterClaim::Claimed);
        assert_eq!(first.claim_writer().unwrap(), WriterClaim::Claimed);
        assert_eq!(second.claim_writer().unwrap(), WriterClaim::Busy);

        first.release_writer();
        assert_eq!(second.claim_writer().unwrap(), WriterClaim::Claimed);
    }

    #[test]
    fn upsert_search_and_snippet() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc(
            "p1",
            "AntiFuzz: Impeding Fuzzing Audits",
            "fuzzing resistance techniques",
        ))
        .unwrap();
        idx.upsert(&doc("p2", "Unrelated Paper", "nothing to see here"))
            .unwrap();

        let hits = idx
            .search("fuzzing", &[], &FieldSel::all(), 10, None)
            .unwrap();
        assert_eq!(hits[0].paper_id, "p1");
        assert!(
            hits[0].snippet.0.iter().any(|p| p.marked),
            "got: {:?}",
            hits[0].snippet
        );
        assert!(!hits.iter().any(|h| h.paper_id == "p2"));
    }

    #[test]
    fn snippets_carry_text_verbatim_with_the_hits_marked() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "Turing's <Test> & Beyond", "b"))
            .unwrap();
        let hits = idx
            .search("turing", &[], &FieldSel::all(), 10, None)
            .unwrap();
        let snippet = &hits[0].snippet;
        assert_eq!(snippet.text(), "Turing's <Test> & Beyond");
        let marked: Vec<&str> = snippet
            .0
            .iter()
            .filter(|p| p.marked)
            .map(|p| p.text.as_str())
            .collect();
        assert_eq!(marked, ["Turing"]);
    }

    #[test]
    fn field_selection_restricts_matching() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "A Title", "the body mentions quicksort"))
            .unwrap();
        let mut sel = FieldSel::none();
        sel.title = true;
        assert!(idx
            .search("quicksort", &[], &sel, 10, None)
            .unwrap()
            .is_empty());
        let mut sel = FieldSel::none();
        sel.body = true;
        let hits = idx.search("quicksort", &[], &sel, 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].field, FieldName::Body);
    }

    #[test]
    fn scope_restricts_matches_inside_the_engine() {
        let (idx, _dir) = open_tmp();
        // "in-title" outranks "in-body" for this query; with limit 1 an
        // after-the-fact filter would return nothing, so this pins that the
        // scope is ANDed into the engine query before truncation.
        idx.upsert(&doc("in-title", "Quicksort Analysis", "some text"))
            .unwrap();
        idx.upsert(&doc(
            "in-body",
            "Sorting Survey",
            "quicksort quicksort quicksort",
        ))
        .unwrap();

        let hits = idx
            .search("quicksort", &[], &FieldSel::all(), 1, None)
            .unwrap();
        assert_eq!(hits[0].paper_id, "in-title");

        let scope = vec!["in-body".to_string()];
        let hits = idx
            .search("quicksort", &[], &FieldSel::all(), 1, Some(&scope))
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].paper_id, "in-body");

        // A scope nothing matches yields nothing.
        let scope = vec!["zz".to_string()];
        assert!(idx
            .search("quicksort", &[], &FieldSel::all(), 10, Some(&scope))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn notes_are_searchable_and_scopeable() {
        let (idx, _dir) = open_tmp();
        let mut d = doc("p1", "A Title", "the body says nothing useful");
        d.notes
            .push_str("compare this against the Dijkstra baseline");
        idx.upsert(&d).unwrap();
        idx.upsert(&doc("p2", "Dijkstra Revisited", "unrelated body"))
            .unwrap();

        let mut sel = FieldSel::none();
        sel.notes = true;
        let hits = idx.search("dijkstra", &[], &sel, 10, None).unwrap();
        assert_eq!(hits.len(), 1, "in:notes must not reach the title field");
        assert_eq!(hits[0].paper_id, "p1");
        assert_eq!(hits[0].field, FieldName::Notes);
        assert!(hits[0].snippet.0.iter().any(|p| p.marked));
    }

    #[test]
    fn title_hit_outranks_body_hit() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("in-title", "Quicksort Analysis", "some text"))
            .unwrap();
        idx.upsert(&doc(
            "in-body",
            "Sorting Survey",
            "quicksort quicksort quicksort",
        ))
        .unwrap();
        let hits = idx
            .search("quicksort", &[], &FieldSel::all(), 10, None)
            .unwrap();
        assert_eq!(hits[0].paper_id, "in-title");
    }

    #[test]
    fn upsert_replaces_and_delete_removes() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc("p1", "Old Title", "b")).unwrap();
        idx.upsert(&doc("p1", "New Title", "b")).unwrap();
        assert!(idx
            .search("old", &[], &FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
        assert_eq!(
            idx.search("new", &[], &FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );
        idx.delete("p1").unwrap();
        assert!(idx
            .search("new", &[], &FieldSel::all(), 10, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn corrupt_dir_is_wiped_and_reports_created() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("meta.json"), b"not json").unwrap();
        let (_idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
    }

    #[test]
    fn reopening_a_current_index_neither_wipes_nor_restamps() {
        let dir = tempfile::tempdir().unwrap();
        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
        idx.upsert(&doc("p1", "A Title", "body")).unwrap();
        drop(idx); // release the writer lock before reopening

        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(!created, "an index at the current schema must survive");
        assert_eq!(
            idx.search("quicksort", &[], &FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            idx.search("title", &[], &FieldSel::all(), 10, None)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_stale_schema_stamp_forces_a_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let (idx, _) = FtsIndex::open(dir.path()).unwrap();
        idx.upsert(&doc("p1", "A Title", "body")).unwrap();
        drop(idx);
        // Stand in for an index written by an older build.
        std::fs::write(dir.path().join(VERSION_FILE), b"1").unwrap();

        let (idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created, "a stale stamp must report created so stamps clear");
        assert!(
            idx.search("title", &[], &FieldSel::all(), 10, None)
                .unwrap()
                .is_empty(),
            "the wiped index starts empty; the sweep refills it"
        );
        assert_eq!(
            stored_schema_version(dir.path()),
            Some(SCHEMA_VERSION),
            "the rebuilt index must carry the current stamp"
        );
    }

    #[test]
    fn an_unstamped_index_is_treated_as_stale() {
        // Indexes written before schema stamping existed have no version file.
        let dir = tempfile::tempdir().unwrap();
        let (idx, _) = FtsIndex::open(dir.path()).unwrap();
        drop(idx);
        std::fs::remove_file(dir.path().join(VERSION_FILE)).unwrap();
        let (_idx, created) = FtsIndex::open(dir.path()).unwrap();
        assert!(created);
    }

    #[test]
    fn second_writer_on_same_dir_is_refused() {
        // Tantivy's writer lock is per-directory and enforced even within one
        // process, so this stands in for a `rebuild` run while `serve` is
        // still holding the lock on the same index dir.
        let dir = tempfile::tempdir().unwrap();
        let (idx1, _created) = FtsIndex::open(dir.path()).unwrap();
        idx1.upsert(&doc("p1", "A Title", "body")).unwrap(); // forces writer creation, lock held

        let (idx2, _created2) = FtsIndex::open(dir.path()).unwrap(); // open is lazy, succeeds
        assert!(
            idx2.delete("x").is_err(),
            "second writer on a locked dir must fail"
        );
    }

    #[test]
    fn zero_limit_returns_empty_instead_of_panicking() {
        let (idx, _dir) = open_tmp();
        idx.upsert(&doc(
            "p1",
            "AntiFuzz: Impeding Fuzzing Audits",
            "fuzzing resistance",
        ))
        .unwrap();
        assert!(idx
            .search("fuzzing", &[], &FieldSel::all(), 0, None)
            .unwrap()
            .is_empty());
    }
}
