use super::fetch::Fetcher;
use super::pipeline::{IngestCtx, Outcome};
use super::source::{parse_source, pdf_target, source_identifier, Source};
use crate::library::model::Identifier;
use crate::metadata::resolve::{ResolvedMetadata, Resolver};

#[derive(Debug)]
enum Fetched {
    /// PDF bytes plus the identifier to seed ingest metadata resolution.
    Pdf {
        bytes: Vec<u8>,
        hint: Option<Identifier>,
    },
    Unfetched {
        metadata: Option<Box<ResolvedMetadata>>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Imported {
    Ingested(Outcome),
    Unfetched {
        title: Option<String>,
        doi: Option<String>,
    },
}

/// Why an import failed: no PDF could be produced (`Unsupported`..`Network`),
/// or the PDF failed to ingest (`Ingest`).
#[derive(Debug)]
pub enum ImportError {
    Unsupported,
    CookieExpired,
    Network(anyhow::Error),
    Ingest(anyhow::Error),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportError::Unsupported => "unsupported input",
            ImportError::CookieExpired => "proxy session expired",
            ImportError::Network(_) => "fetch failed",
            ImportError::Ingest(_) => "import failed",
        })
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImportError::Network(e) | ImportError::Ingest(e) => Some(e.as_ref()),
            ImportError::Unsupported | ImportError::CookieExpired => None,
        }
    }
}

/// Turn an input string into PDF bytes, following the spec's fetch order:
/// arXiv direct → publisher-via-proxy (cookie) → Unpaywall OA → clean failure.
async fn import_source(
    fetcher: &Fetcher,
    resolver: &Resolver,
    input: &str,
    cookie: Option<&str>,
) -> Result<Fetched, ImportError> {
    let src = parse_source(input).ok_or(ImportError::Unsupported)?;
    let hint = source_identifier(&src);

    let target = pdf_target(&src, &fetcher.arxiv_pdf_base);

    // 1. arXiv is always taken direct (open, no proxy).
    if let Source::Arxiv(_) = &src {
        let target = target.expect("arxiv always has a target");
        return match fetcher.fetch_plain(&target.url).await {
            Ok(Some(bytes)) => Ok(Fetched::Pdf { bytes, hint }),
            Ok(None) => Ok(Fetched::Unfetched {
                metadata: metadata_for(resolver, &src).await,
            }),
            Err(e) => Err(ImportError::Network(e)),
        };
    }

    // 2. Known paywalled publisher + a cookie → proxied fetch (preferred).
    // A fetch that fails outright is kept for step 4: without a PDF, it is
    // the reason the import failed.
    let mut cookie_expired = false;
    let mut network_error = None;
    if let (Some(target), Some(cookie)) = (target, cookie) {
        if target.requires_proxy && fetcher.proxy_enabled() {
            match fetcher.fetch_proxied(&target.url, cookie).await {
                Ok(Some(bytes)) => return Ok(Fetched::Pdf { bytes, hint }),
                Ok(None) => cookie_expired = true, // non-PDF: likely expired session
                Err(e) => network_error = Some(e.context("proxied fetch")),
            }
        }
    }

    // 3. Open-access fallback via Unpaywall (needs a DOI).
    if let Source::Doi(doi) = &src {
        if let Some(oa) = resolver.oa_pdf_url(doi).await {
            match fetcher.fetch_plain(&oa).await {
                Ok(Some(bytes)) => return Ok(Fetched::Pdf { bytes, hint }),
                // Unpaywall asserted a PDF at this URL, so a non-PDF body is
                // itself noteworthy (link rot, an interstitial page).
                Ok(None) => tracing::warn!("oa fetch for {oa} returned a non-PDF body"),
                Err(e) => {
                    network_error = Some(e.context(format!("open-access fetch of {oa}")));
                }
            }
        }
    }

    // 4. Give up, with the most actionable reason: an expired cookie the user
    // can renew, then the fetch that broke, and only then "no PDF anywhere".
    if cookie_expired {
        return Err(ImportError::CookieExpired);
    }
    if let Some(e) = network_error {
        return Err(ImportError::Network(e));
    }
    Ok(Fetched::Unfetched {
        metadata: metadata_for(resolver, &src).await,
    })
}

/// Fetch → stage → ingest → cleanup-on-error, shared by `xuewen import` and
/// `POST /api/import`. The shells only present the result: the CLI maps it to
/// println!/bail!, the web to JSON/status codes.
pub(crate) async fn fetch_stage_ingest(
    ctx: &IngestCtx,
    fetcher: &Fetcher,
    input: &str,
    cookie: Option<&str>,
) -> Result<Imported, ImportError> {
    // Best-effort dedupe BEFORE any network fetch: re-importing a known
    // DOI/arXiv id must not download the whole PDF (or spend an EZproxy round
    // trip) just to learn SameWork. The post-ingest identifier check stays the
    // authority — this can only miss (and then that check catches it).
    if let Some(ident) = parse_source(input).as_ref().and_then(source_identifier) {
        let (doi, arxiv) = match &ident {
            Identifier::Doi(d) => (Some(d.as_str()), None),
            Identifier::Arxiv(a) => (None, Some(a.as_str())),
            Identifier::None => (None, None),
        };
        if let Some(existing) =
            crate::library::papers::find_by_identifier(&ctx.pool, doi, arxiv, None)
                .await
                .map_err(ImportError::Ingest)?
        {
            return Ok(Imported::Ingested(Outcome::from_identifier_hit(&existing)));
        }
    }
    match import_source(fetcher, &ctx.resolver, input, cookie).await? {
        Fetched::Pdf { bytes, hint } => ctx
            .ingest_bytes(&bytes, "import.pdf", hint)
            .await
            .map(Imported::Ingested)
            .map_err(ImportError::Ingest),
        Fetched::Unfetched { metadata } => {
            let (title, doi) = match metadata {
                Some(m) => (m.title, m.doi),
                None => (None, None),
            };
            Ok(Imported::Unfetched { title, doi })
        }
    }
}

/// Best-effort metadata for the clean-failure message.
async fn metadata_for(resolver: &Resolver, src: &Source) -> Option<Box<ResolvedMetadata>> {
    match source_identifier(src) {
        Some(ident) => resolver.lookup_or_skip(&ident).await.map(Box::new),
        None => None,
    }
}

#[cfg(test)]
mod orchestration_tests {
    use super::*;
    use crate::metadata::resolve::Resolver;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PDF: &[u8] = b"%PDF-1.4\nx\n";

    #[tokio::test]
    async fn arxiv_pdf_is_fetched_direct() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pdf/1706.03762v5"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        let fetcher = Fetcher::new(None)
            .unwrap()
            .with_arxiv_pdf_base(format!("{}/pdf", server.uri()));

        let Fetched::Pdf { bytes, hint } =
            import_source(&fetcher, &Resolver::offline(), "arXiv:1706.03762v5", None)
                .await
                .unwrap()
        else {
            panic!("expected a fetched PDF");
        };
        assert_eq!(bytes, PDF);
        assert_eq!(hint, Some(Identifier::Arxiv("1706.03762".into())));
    }

    #[tokio::test]
    async fn doi_oa_copy_is_fetched() {
        // A DOI with no cookie whose Unpaywall record points at a PDF the mock
        // serves.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/10.1145/oa"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"{{"best_oa_location":{{"url_for_pdf":"{}/paper.pdf"}}}}"#,
                server.uri()
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/paper.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        let resolver = Resolver::offline()
            .with_contact_email("me@uchicago.edu")
            .with_unpaywall_base(server.uri());
        let fetcher = Fetcher::new(None).unwrap();

        let Fetched::Pdf { bytes, hint } = import_source(&fetcher, &resolver, "10.1145/oa", None)
            .await
            .unwrap()
        else {
            panic!("expected a fetched PDF");
        };
        assert_eq!(bytes, PDF);
        assert_eq!(
            hint,
            Some(crate::library::model::Identifier::Doi("10.1145/oa".into()))
        );
    }

    #[tokio::test]
    async fn unsupported_input_errors() {
        let fetcher = Fetcher::new(None).unwrap();
        let err = import_source(&fetcher, &Resolver::offline(), "not an id", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::Unsupported));
    }

    #[tokio::test]
    async fn paywalled_no_cookie_no_oa_is_unfetched() {
        let fetcher = Fetcher::new(None).unwrap(); // proxy disabled
                                                   // ACM DOI, no cookie, offline resolver → no OA, no metadata → Unfetched.
        let out = import_source(&fetcher, &Resolver::offline(), "10.1145/paywalled", None)
            .await
            .unwrap();
        assert!(matches!(out, Fetched::Unfetched { .. }));
    }

    #[tokio::test]
    async fn known_identifier_short_circuits_before_any_fetch() {
        use crate::ingest::pipeline::{IngestCtx, Libraries, Outcome};

        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let paper = crate::library::model::Paper {
            id: "01890000-0000-7000-8000-0000000000aa".into(),
            content_hash: "h1".into(),
            rel_path: "h1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-08T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: crate::library::model::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::library::model::Authors::default(),
                venue: None,
                year: None,
                doi: Some("10.1145/known".into()),
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: crate::library::model::PaperStatus::NeedsReview,
            },
        };
        crate::library::papers::insert(&pool, &paper).await.unwrap();

        let ctx = IngestCtx::new(
            pool.clone(),
            Libraries::under(&dir.path().join("inbox"), &dir.path().join("library")),
            Resolver::offline(),
            None,
        );
        // Proxy-less fetcher + offline resolver: without the pre-fetch check
        // this ACM DOI would go down the fetch chain and end in Unfetched;
        // with it, the answer comes straight from the DB. Pasted case also
        // differs, exercising identifier canonicalization end to end.
        let fetcher = Fetcher::new(None).unwrap();
        let out = fetch_stage_ingest(&ctx, &fetcher, "10.1145/KNOWN", None)
            .await
            .unwrap();
        assert_eq!(out, Imported::Ingested(Outcome::SameWork(paper.id.clone())));

        crate::library::trash::soft_delete(&pool, &paper.id)
            .await
            .unwrap();
        let out = fetch_stage_ingest(&ctx, &fetcher, "10.1145/known", None)
            .await
            .unwrap();
        assert_eq!(out, Imported::Ingested(Outcome::InTrash(paper.id)));
    }

    #[tokio::test]
    async fn expired_cookie_reported() {
        // Proxy returns HTML for any cookie → CookieExpired (no OA fallback available).
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
            .mount(&server)
            .await;
        let fetcher = Fetcher::new(Some(
            format!("{}/login?url=", server.uri()).parse().unwrap(),
        ))
        .unwrap();
        let err = import_source(
            &fetcher,
            &Resolver::offline(),
            "10.1145/3292500.3330701",
            Some("ezproxy=stale"),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ImportError::CookieExpired));
    }

    #[tokio::test]
    async fn a_failed_proxied_fetch_is_a_network_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let fetcher = Fetcher::new(Some(
            format!("{}/login?url=", server.uri()).parse().unwrap(),
        ))
        .unwrap();
        let err = import_source(
            &fetcher,
            &Resolver::offline(),
            "10.1145/3292500.3330701",
            Some("ezproxy=good"),
        )
        .await
        .unwrap_err();
        let ImportError::Network(e) = err else {
            panic!("expected a network error, got {err:?}");
        };
        assert!(format!("{e:#}").contains("502"), "got: {e:#}");
    }
}
