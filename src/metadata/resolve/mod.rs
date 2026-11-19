pub(crate) mod arxiv;
mod crossref;
mod dblp;
pub mod grobid;
mod openreview;
mod unpaywall;

use crate::http::{HttpClient, RetryPolicy};
use crate::library::model::{canonical_arxiv, canonical_doi, Identifier};
use crate::metadata::matching;
use anyhow::Result;
use std::time::Duration;

/// Normalized bibliographic metadata produced by a source resolver. On the
/// wire it is a manual-identify candidate, which round-trips through
/// POST /api/papers/{id}/identify unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, rename = "Candidate")]
pub struct ResolvedMetadata {
    pub title: Option<String>,
    #[serde(rename = "abstract")]
    pub(crate) abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    /// Which source produced this record: "arxiv" | "crossref" | "dblp" |
    /// "grobid" | "openreview".
    pub source: String,
}

/// Fold an issue that names a conference into the venue string. ACM publishes
/// PACMPL and PACMHCI as one issue per conference, so the container title
/// ("Proceedings of the ACM on Programming Languages") cannot say whether a
/// paper is POPL, ICFP, OOPSLA or PLDI — only `issue`/`number` can, and
/// without it the venue is the journal alone. An ordinary numeric issue names
/// nothing and is left off.
fn venue_with_issue(venue: Option<String>, issue: Option<&str>) -> Option<String> {
    let venue = venue?;
    let issue = issue.unwrap_or("").trim();
    if !issue.chars().any(|c| c.is_ascii_alphabetic()) || venue.contains(issue) {
        return Some(venue);
    }
    Some(format!("{venue} ({issue})"))
}

/// Fetches authoritative metadata for an identifier or a title. The
/// manual-identify entry points (`lookup`, `search_candidates`) report an
/// upstream failure; the ingest ones (`lookup_or_skip`, `search_title`)
/// degrade it to `None`, so resolution never aborts ingestion.
pub struct Resolver {
    http: HttpClient,
    email: Option<String>,
    upstreams: Upstreams,
}

/// Where each metadata source is reached. A source without a base URL (or,
/// for OpenReview, without hosts) takes no part in resolution.
struct Upstreams {
    arxiv: Option<String>,
    crossref: Option<String>,
    dblp: Option<String>,
    /// Both OpenReview hosts, searched together. API 2 (`api2.`) holds the
    /// 2023-onwards venues and API 1 everything before, and a title search
    /// cannot know which applies until it has the year.
    openreview: Vec<String>,
    unpaywall: Option<String>,
}

impl Upstreams {
    fn live() -> Self {
        Self {
            arxiv: Some("https://export.arxiv.org".into()),
            crossref: Some("https://api.crossref.org".into()),
            dblp: Some("https://dblp.org".into()),
            openreview: vec![
                "https://api2.openreview.net".into(),
                "https://api.openreview.net".into(),
            ],
            unpaywall: Some("https://api.unpaywall.org".into()),
        }
    }
}

/// What a PDF carrying no identifier offers the resolver.
#[derive(Debug, Clone, Copy)]
pub struct TitleQuery<'a> {
    pub title: &'a str,
    /// The paper's own extracted text. A candidate clearing the title gate is
    /// additionally required to be corroborated by it; empty text abstains,
    /// leaving the title gate alone as the only test.
    pub text: &'a str,
}

#[cfg(any(test, feature = "test-support"))]
impl<'a> TitleQuery<'a> {
    /// A title search with no text to corroborate candidates against.
    pub fn new(title: &'a str) -> Self {
        Self { title, text: "" }
    }
}

impl Resolver {
    /// A resolver for the live sources, retrying per `retry`.
    pub(crate) fn new(contact_email: Option<&str>, retry: RetryPolicy) -> Result<Self> {
        Self::build(contact_email, retry, Upstreams::live())
    }

    fn build(
        contact_email: Option<&str>,
        retry: RetryPolicy,
        upstreams: Upstreams,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(crate::http::user_agent(contact_email))
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http: HttpClient::new(client, retry),
            email: contact_email.map(str::to_string),
            upstreams,
        })
    }

    /// The record an identifier names, for a user who asked for it: `None`
    /// when the source has no such record (or is switched off), `Err` when it
    /// could not be asked.
    pub async fn lookup(&self, ident: &Identifier) -> Result<Option<ResolvedMetadata>> {
        match ident {
            Identifier::Arxiv(id) => self.lookup_arxiv(id).await,
            Identifier::Doi(doi) => self.lookup_crossref(doi).await,
            Identifier::None => Ok(None),
        }
    }

    /// `lookup` for automatic ingest, which must never abort on an unreachable
    /// source: the failure is logged and reads as "no record".
    pub async fn lookup_or_skip(&self, ident: &Identifier) -> Option<ResolvedMetadata> {
        let key = match ident {
            Identifier::Arxiv(id) => id.as_str(),
            Identifier::Doi(doi) => doi.as_str(),
            Identifier::None => return None,
        };
        degrade("identifier lookup", key, self.lookup(ident).await).flatten()
    }

    async fn lookup_arxiv(&self, id: &str) -> Result<Option<ResolvedMetadata>> {
        let Some(base) = self.upstreams.arxiv.as_deref() else {
            return Ok(None);
        };
        Ok(
            arxiv::parse(&arxiv::fetch(&self.http, base, id).await?)?.map(|mut m| {
                // The Atom feed does not echo the queried id.
                m.arxiv_id = Some(canonical_arxiv(id));
                m
            }),
        )
    }

    async fn lookup_crossref(&self, doi: &str) -> Result<Option<ResolvedMetadata>> {
        let Some(base) = self.upstreams.crossref.as_deref() else {
            return Ok(None);
        };
        let Some(body) = crossref::fetch(&self.http, base, doi).await? else {
            return Ok(None);
        };
        Ok(crossref::parse(&body)?.map(|mut m| {
            m.doi.get_or_insert_with(|| canonical_doi(doi));
            m
        }))
    }

    /// The record a title search confidently matches: DBLP, then OpenReview,
    /// then Crossref bibliographic search, each filtered by the gate.
    ///
    /// OpenReview sits between the two because it is the only source holding
    /// the current year's ICLR/ICML proceedings: DBLP builds a conference
    /// volume months after the fact, and Crossref has never carried ICLR at
    /// all. It stays *behind* DBLP because a DBLP record is richer — the DOI
    /// and canonical key that dedupe wants.
    ///
    /// A DBLP hit that is only the CoRR preprint does not end the search,
    /// though: DBLP indexes a paper's arXiv posting long before its
    /// proceedings volume, so the camera-ready of an ICLR 2026 paper matches
    /// "CoRR 2025" and would be filed as a preprint while OpenReview knows it
    /// as ICLR 2026.
    pub async fn search_title(&self, query: TitleQuery<'_>) -> Option<ResolvedMetadata> {
        let TitleQuery { title, text } = query;
        if title.trim().is_empty() {
            return None;
        }
        let text = matching::PaperText::new(text);
        let dblp = self.try_dblp(title, &text).await;
        if dblp.as_ref().is_some_and(|m| !is_corr_preprint(m)) {
            return dblp;
        }
        if let Some(md) = best_match(title, &text, self.openreview_candidates(title).await) {
            return Some(match &dblp {
                Some(preprint) => graft_identifiers(md, preprint),
                None => md,
            });
        }
        if dblp.is_some() {
            return dblp;
        }
        self.try_crossref_search(title, &text).await
    }

    /// Candidates from every OpenReview host, searched concurrently. A host
    /// that fails degrades to no candidates from that host alone.
    async fn openreview_candidates(&self, title: &str) -> Vec<ResolvedMetadata> {
        let key = format!("{title:?}");
        self.openreview_searches(title)
            .await
            .into_iter()
            .filter_map(|r| degrade("openreview search", &key, r))
            .flatten()
            .collect()
    }

    async fn openreview_searches(&self, title: &str) -> Vec<Result<Vec<ResolvedMetadata>>> {
        let searches = self
            .upstreams
            .openreview
            .iter()
            .map(|base| self.fetch_parse_openreview(base, title));
        futures_util::future::join_all(searches).await
    }

    async fn fetch_parse_openreview(
        &self,
        base: &str,
        title: &str,
    ) -> Result<Vec<ResolvedMetadata>> {
        let body = openreview::search(&self.http, base, title).await?;
        openreview::parse(&body)
    }

    async fn try_dblp(&self, title: &str, text: &matching::PaperText) -> Option<ResolvedMetadata> {
        degrade(
            "dblp search",
            &format!("{title:?}"),
            self.fetch_parse_dblp(title).await,
        )
        .and_then(|cands| best_match(title, text, cands))
    }

    async fn fetch_parse_dblp(&self, title: &str) -> Result<Vec<ResolvedMetadata>> {
        let Some(base) = self.upstreams.dblp.as_deref() else {
            return Ok(Vec::new());
        };
        dblp::parse(&dblp::fetch(&self.http, base, title).await?)
    }

    async fn try_crossref_search(
        &self,
        title: &str,
        text: &matching::PaperText,
    ) -> Option<ResolvedMetadata> {
        degrade(
            "crossref search",
            &format!("{title:?}"),
            self.fetch_parse_crossref_search(title).await,
        )
        .and_then(|cands| best_match(title, text, cands))
    }

    async fn fetch_parse_crossref_search(&self, title: &str) -> Result<Vec<ResolvedMetadata>> {
        let Some(base) = self.upstreams.crossref.as_deref() else {
            return Ok(Vec::new());
        };
        crossref::parse_search(&crossref::search(&self.http, base, title).await?)
    }

    /// The best open-access PDF URL for a DOI via Unpaywall, or `None` when
    /// there is no OA copy, no configured contact email, or the lookup fails.
    pub(crate) async fn oa_pdf_url(&self, doi: &str) -> Option<String> {
        let email = self.email.as_deref()?;
        let base = self.upstreams.unpaywall.as_deref()?;
        degrade(
            "unpaywall lookup",
            doi,
            unpaywall::fetch(&self.http, base, doi, email).await,
        )
        .flatten()
    }
}

/// Construction for tests: start from a resolver that reaches nothing and
/// point the sources a test exercises at its mock server.
#[cfg(any(test, feature = "test-support"))]
impl Resolver {
    /// Every source switched off: lookups and searches resolve to `None`
    /// without sending a request.
    pub fn offline() -> Self {
        Self::build(None, RetryPolicy::fast_for_tests(), Upstreams::none())
            .expect("build the HTTP client")
    }

    /// Identify as `email` (user-agent `mailto`), which also enables Unpaywall.
    pub fn with_contact_email(self, email: &str) -> Self {
        Self::build(Some(email), RetryPolicy::fast_for_tests(), self.upstreams)
            .expect("build the HTTP client")
    }

    pub fn with_arxiv_base(mut self, base: impl Into<String>) -> Self {
        self.upstreams.arxiv = Some(base.into());
        self
    }

    pub fn with_crossref_base(mut self, base: impl Into<String>) -> Self {
        self.upstreams.crossref = Some(base.into());
        self
    }

    pub fn with_dblp_base(mut self, base: impl Into<String>) -> Self {
        self.upstreams.dblp = Some(base.into());
        self
    }

    /// Two distinct paths on one mock server stand in for the two hosts.
    pub fn with_openreview_bases(mut self, bases: Vec<String>) -> Self {
        self.upstreams.openreview = bases;
        self
    }

    pub fn with_unpaywall_base(mut self, base: impl Into<String>) -> Self {
        self.upstreams.unpaywall = Some(base.into());
        self
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Upstreams {
    fn none() -> Self {
        Self {
            arxiv: None,
            crossref: None,
            dblp: None,
            openreview: Vec::new(),
            unpaywall: None,
        }
    }
}

/// Whether a DBLP record is the paper's arXiv posting rather than its
/// published version. DBLP keys every CoRR entry under `journals/corr/`.
fn is_corr_preprint(md: &ResolvedMetadata) -> bool {
    md.dblp_key
        .as_deref()
        .is_some_and(|k| k.starts_with("journals/corr/"))
}

/// Carry the preprint's identifiers onto the published record. OpenReview
/// knows the venue but mints no DOI and no DBLP key, while the CoRR record
/// holds both — dropping them would lose the arXiv DOI that dedupes the
/// camera-ready against a later ingest of the same paper's preprint.
///
/// The two records are matched against each other, not just against the query
/// that found them: both clearing the gate separately only means each is close
/// to the query, and stamping one paper's DOI onto another that merely shares
/// a near-identical title would be far worse than shipping no DOI at all.
fn graft_identifiers(
    mut published: ResolvedMetadata,
    preprint: &ResolvedMetadata,
) -> ResolvedMetadata {
    let same_work = match (published.title.as_deref(), preprint.title.as_deref()) {
        (Some(a), Some(b)) => matching::title_similarity(a, b) >= matching::MATCH_THRESHOLD,
        _ => false,
    };
    if !same_work {
        return published;
    }
    published.doi = published.doi.or_else(|| preprint.doi.clone());
    published.arxiv_id = published.arxiv_id.or_else(|| preprint.arxiv_id.clone());
    published.dblp_key = published.dblp_key.or_else(|| preprint.dblp_key.clone());
    published.abstract_text = published
        .abstract_text
        .or_else(|| preprint.abstract_text.clone());
    published
}

/// The log-and-degrade shared by every resolver wrapper: an upstream failure
/// warns (`"<what> failed for <key>: <err>"`) and resolves to `None` —
/// resolution never aborts ingestion.
fn degrade<T>(what: &str, key: &str, r: Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("{what} failed for {key}: {e}");
            None
        }
    }
}

/// Most candidates a manual-identify search returns.
const MAX_CANDIDATES: usize = 8;

impl Resolver {
    /// Title-search candidates from DBLP, Crossref and OpenReview, queried
    /// concurrently, WITHOUT the confidence gate: the caller (a human picking
    /// a match) is the gate.
    /// Deduped, ranked by similarity to `query`, capped at `MAX_CANDIDATES`.
    /// A failing source only drops its own candidates; `Err` means every
    /// source failed, so an empty list is never an outage in disguise.
    pub async fn search_candidates(&self, query: &str) -> Result<Vec<ResolvedMetadata>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let (dblp, crossref, openreview) = tokio::join!(
            self.fetch_parse_dblp(query),
            self.fetch_parse_crossref_search(query),
            self.openreview_searches(query)
        );
        let searches = [("dblp", dblp), ("crossref", crossref)]
            .into_iter()
            .chain(openreview.into_iter().map(|r| ("openreview", r)));
        let (mut cands, mut answered, mut last_err) = (Vec::new(), false, None);
        for (source, result) in searches {
            match result {
                Ok(c) => {
                    answered = true;
                    cands.extend(c);
                }
                Err(e) => {
                    tracing::warn!("{source} candidate search failed for {query:?}: {e}");
                    last_err = Some(e.context(format!("{source} search")));
                }
            }
        }
        match last_err {
            Some(e) if !answered => Err(e),
            _ => Ok(rank_candidates(query, cands)),
        }
    }
}

/// Dedup (by canonical DOI, else DBLP key; first occurrence wins), rank by
/// title similarity to `query` (untitled candidates sink), cap the list.
fn rank_candidates(query: &str, cands: Vec<ResolvedMetadata>) -> Vec<ResolvedMetadata> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ResolvedMetadata> = Vec::new();
    for c in cands {
        let key = c
            .doi
            .as_deref()
            .map(|d| format!("doi:{}", canonical_doi(d)))
            .or_else(|| c.dblp_key.as_deref().map(|k| format!("dblp:{k}")));
        if let Some(key) = key {
            if !seen.insert(key) {
                continue;
            }
        }
        out.push(c);
    }
    // Score once per candidate (title_similarity allocates + runs a
    // Levenshtein), then sort on the precomputed key; the stable sort keeps
    // equal-scoring candidates in arrival order, as before.
    let mut scored = scored(query, out);
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<ResolvedMetadata> = scored.into_iter().map(|(_, c)| c).collect();
    out.truncate(MAX_CANDIDATES);
    out
}

/// Score every candidate against `query` (untitled candidates sink to -1).
/// The one scoring site shared by ranking and the confidence gate, so a
/// threshold or normalization change can never apply to one and not the other.
fn scored(query: &str, cands: Vec<ResolvedMetadata>) -> Vec<(f64, ResolvedMetadata)> {
    cands
        .into_iter()
        .map(|c| {
            let score = c
                .title
                .as_deref()
                .map(|t| matching::title_similarity(query, t))
                .unwrap_or(-1.0);
            (score, c)
        })
        .collect()
}

/// Pick the highest-similarity candidate whose title confidently matches
/// `query` and whose authors the paper's own `text` corroborates.
///
/// Both tests are per candidate, not a veto on the winner, so a genuine match
/// sitting behind a same-titled impostor is still found rather than lost with
/// it.
fn best_match(
    query: &str,
    text: &matching::PaperText,
    candidates: Vec<ResolvedMetadata>,
) -> Option<ResolvedMetadata> {
    let mut best: Option<(f64, ResolvedMetadata)> = None;
    for (score, c) in scored(query, candidates) {
        if score < matching::MATCH_THRESHOLD || !text.corroborates(&c.authors) {
            continue;
        }
        // Strictly greater: at equal scores the FIRST candidate wins.
        // Exact ties are realistic (conference and journal versions of one
        // work share a title), and the winner must not silently flip.
        if best.as_ref().is_none_or(|(bs, _)| score > *bs) {
            best = Some((score, c));
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(title: &str, doi: Option<&str>, dblp_key: Option<&str>) -> ResolvedMetadata {
        ResolvedMetadata {
            title: Some(title.to_string()),
            doi: doi.map(str::to_string),
            dblp_key: dblp_key.map(str::to_string),
            source: "test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn rank_candidates_sorts_dedups_and_caps() {
        let query = "AntiFuzz: Impeding Fuzzing Audits of Binary Executables";
        let mut cands = vec![
            cand("Something Unrelated Entirely", None, Some("conf/x/1")),
            cand(query, Some("10.1/af"), Some("conf/uss/GulerAAH19")),
            // Same DOI from the other source: deduped, first occurrence wins.
            cand(query, Some("10.1/AF"), None),
        ];
        // Pad with distinct filler beyond the cap.
        for i in 0..10 {
            cands.push(cand(&format!("Filler Paper Number {i}"), None, None));
        }
        let ranked = rank_candidates(query, cands);
        assert_eq!(ranked.len(), 8); // capped
                                     // Exact-title match ranks first; its DOI-duplicate is gone.
        assert_eq!(ranked[0].dblp_key.as_deref(), Some("conf/uss/GulerAAH19"));
        assert_eq!(
            ranked.iter().filter(|c| c.doi.is_some()).count(),
            1,
            "case-insensitive DOI dedup"
        );
    }

    #[test]
    fn best_match_keeps_the_first_of_equal_scoring_candidates() {
        let query = "Deep Residual Learning for Image Recognition";
        // Identical titles score identically; the first (e.g. the conference
        // version DBLP lists ahead of the journal reprint) must win.
        let winner = best_match(
            query,
            &matching::PaperText::default(),
            vec![
                cand(query, None, Some("conf/cvpr/HeZRS16")),
                cand(query, None, Some("journals/corr/HeZRS15")),
            ],
        )
        .unwrap();
        assert_eq!(winner.dblp_key.as_deref(), Some("conf/cvpr/HeZRS16"));
    }

    fn authored(title: &str, key: &str, authors: &[&str]) -> ResolvedMetadata {
        ResolvedMetadata {
            authors: authors.iter().map(|s| s.to_string()).collect(),
            ..cand(title, None, Some(key))
        }
    }

    #[test]
    fn best_match_prefers_a_lower_scoring_candidate_the_paper_corroborates() {
        // The impostor scores higher, so a check that only vetoed the winner
        // would drop the real record along with it.
        let query = "Language Models are Few-Shot Learners";
        let text = matching::PaperText::new("Language Models are Few-Shot Learners\nTom B. Brown");
        let winner = best_match(
            query,
            &text,
            vec![
                authored(query, "book/svup/Malakar26", &["Sourav Malakar"]),
                authored(
                    "Language Models are Few-Shot Learners.",
                    "conf/nips/BrownMRSKDNSSAA20",
                    &["Tom B. Brown", "Benjamin Mann"],
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            winner.dblp_key.as_deref(),
            Some("conf/nips/BrownMRSKDNSSAA20")
        );
    }

    #[test]
    fn best_match_rejects_an_uncorroborated_perfect_title() {
        let query = "Language Models are Few-Shot Learners";
        let text = matching::PaperText::new("Language Models are Few-Shot Learners\nTom B. Brown");
        assert_eq!(
            best_match(
                query,
                &text,
                vec![authored(query, "book/svup/Malakar26", &["Sourav Malakar"])],
            ),
            None
        );
    }

    #[test]
    fn rank_candidates_keeps_untitled_last_and_handles_empty() {
        assert!(rank_candidates("query", Vec::new()).is_empty());
        let ranked = rank_candidates(
            "Deep Residual Learning",
            vec![
                ResolvedMetadata {
                    source: "test".into(),
                    ..Default::default()
                }, // no title
                cand("Deep Residual Learning", None, None),
            ],
        );
        assert_eq!(ranked[0].title.as_deref(), Some("Deep Residual Learning"));
    }

    #[tokio::test]
    async fn oa_pdf_url_hits_unpaywall() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/unpaywall_oa.json"
        ));
        Mock::given(method("GET"))
            .and(path("/v2/10.1145/3292500.3330701"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let r = Resolver::offline()
            .with_contact_email("me@uchicago.edu")
            .with_unpaywall_base(server.uri());
        assert_eq!(
            r.oa_pdf_url("10.1145/3292500.3330701").await.as_deref(),
            Some("https://example.org/paper.pdf")
        );
        // No email configured → skipped entirely.
        let r2 = Resolver::offline().with_unpaywall_base(server.uri());
        assert_eq!(r2.oa_pdf_url("10.1145/3292500.3330701").await, None);
    }

    #[tokio::test]
    async fn an_offline_resolver_resolves_nothing() {
        let r = Resolver::offline().with_contact_email("me@uchicago.edu");
        assert_eq!(
            r.lookup(&Identifier::Doi("10.1/x".into())).await.unwrap(),
            None
        );
        assert_eq!(
            r.lookup(&Identifier::Arxiv("1706.03762".into()))
                .await
                .unwrap(),
            None
        );
        assert_eq!(r.search_title(TitleQuery::new("Attention")).await, None);
        assert!(r.search_candidates("Attention").await.unwrap().is_empty());
        assert_eq!(r.oa_pdf_url("10.1/x").await, None);
    }
}
