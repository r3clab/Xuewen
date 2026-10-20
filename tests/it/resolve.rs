use crate::support;

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::library::model::Identifier;
use xuewen::metadata::resolve::{Resolver, TitleQuery};

const ARXIV_FIXTURE: &str = support::fixture!("arxiv_attention.xml");
const CROSSREF_FIXTURE: &str = support::fixture!("crossref_kgat.json");
const OPENREVIEW_FIXTURE: &str = support::fixture!("openreview_succinct.json");

#[tokio::test]
async fn resolves_doi_via_crossref() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(
                md.title.as_deref(),
                Some("KGAT: Knowledge Graph Attention Network for Recommendation")
            );
            assert_eq!(md.doi.as_deref(), Some(doi));
            assert_eq!(md.year, Some(2019));
        }
        None => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn resolves_arxiv_via_api() {
    let server = MockServer::start().await;
    let id = "1706.03762";
    Mock::given(method("GET"))
        .and(path("/api/query"))
        .and(query_param("id_list", id))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Arxiv(id.to_string()), None)
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "arxiv");
            assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
            assert_eq!(md.arxiv_id.as_deref(), Some(id)); // stamped by the resolver
        }
        None => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn doi_with_fragment_char_reaches_crossref_percent_encoded() {
    // '#' is legal in a DOI; sent raw it would truncate the request path to
    // /works/10.7912/c2 as a URL fragment. The mock answers only the encoded
    // form, so resolution succeeding proves the path arrived encoded.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works/10.7912/c2%23abc"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi("10.7912/c2#abc".to_string()), None)
        .await;
    assert!(res.is_some(), "encoded request must reach the mock");
}

#[tokio::test]
async fn http_error_degrades_to_unresolved() {
    // A server with no stubs returns 404 for everything.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi("10.9999/nope".to_string()), None)
        .await;
    assert_eq!(res, None);
}

#[tokio::test]
async fn none_identifier_is_unresolved() {
    let resolver = Resolver::new(None).unwrap();
    assert_eq!(resolver.resolve(&Identifier::None, None).await, None);
}

#[tokio::test]
async fn parse_error_degrades_to_unresolved() {
    // Server returns 200 but a malformed body: fetch succeeds, parse fails -> Unresolved.
    let server = MockServer::start().await;
    let doi = "10.1234/malformed";
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string("{ not valid json"))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;
    assert_eq!(res, None);
}

const DBLP_FIXTURE: &str = support::fixture!("dblp_kgat.json");
const CROSSREF_SEARCH_FIXTURE: &str = support::fixture!("crossref_search_kgat.json");

const KGAT_TITLE: &str = "KGAT: Knowledge Graph Attention Network for Recommendation";

#[tokio::test]
async fn resolves_title_via_dblp() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(KGAT_TITLE)))
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "dblp");
            assert_eq!(md.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
            assert_eq!(md.venue.as_deref(), Some("KDD"));
            assert_eq!(md.year, Some(2019));
        }
        None => panic!("expected Resolved via DBLP"),
    }
}

#[tokio::test]
async fn falls_back_to_crossref_search_when_dblp_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(KGAT_TITLE)))
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        }
        None => panic!("expected Resolved via Crossref fallback"),
    }
}

#[tokio::test]
async fn dblp_error_falls_back_to_crossref() {
    let server = MockServer::start().await;
    // DBLP returns a server error...
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    // ...Crossref bibliographic search then succeeds.
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(KGAT_TITLE)))
        .await;

    match res {
        Some(md) => assert_eq!(md.source, "crossref"),
        None => panic!("expected Crossref fallback after DBLP 500"),
    }
}

#[tokio::test]
async fn low_similarity_title_is_unresolved() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    // Crossref search has no stub -> 404 -> None. So overall Unresolved.

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(
            &Identifier::None,
            Some(TitleQuery::new(
                "An Entirely Unrelated Paper Title About Frogs",
            )),
        )
        .await;
    assert_eq!(res, None);
}

#[tokio::test]
async fn resolves_after_transient_429() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    // First request is rate-limited, the retry succeeds.
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;

    assert!(res.is_some());
}

#[tokio::test]
async fn search_candidates_merges_both_sources_ungated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    // A truncated query that the 0.85 gate would reject still yields the hit.
    let cands = resolver.search_candidates("KGAT: Knowledge Graph").await;
    assert_eq!(cands.len(), 1, "same DOI from both sources dedups to one");
    assert_eq!(cands[0].source, "dblp"); // DBLP queried first, wins the dedup
    assert_eq!(cands[0].doi.as_deref(), Some("10.1145/3292500.3330701"));

    // Empty query short-circuits without network.
    assert!(resolver.search_candidates("  ").await.is_empty());
}

#[tokio::test]
async fn search_candidates_degrades_when_one_source_fails() {
    let server = MockServer::start().await;
    // DBLP is down...
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    // ...Crossref search still answers.
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    let cands = resolver.search_candidates("KGAT Knowledge Graph").await;
    assert_eq!(cands.len(), 1, "surviving source still yields candidates");
    assert_eq!(cands[0].source, "crossref");
}

/// The camera-ready title after `identify::guess_title` rejoins pdftotext's
/// small-caps splits; DBLP has no ICLR 2026 volume yet and Crossref has never
/// carried ICLR at all, so OpenReview is the only source that can answer.
const ICLR_TITLE: &str = "TRANSFORMERS ARE INHERENTLY SUCCINCT";

#[tokio::test]
async fn resolves_iclr_title_via_openreview_when_dblp_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    // API 1 holds only pre-2023 venues, so it answers with nothing.
    Mock::given(method("GET"))
        .and(path("/api1/notes/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"notes":[],"count":0}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .and(query_param("term", ICLR_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(OPENREVIEW_FIXTURE))
        .mount(&server)
        .await;
    // Crossref must never be reached: a confident OpenReview hit ends the chain.
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .expect(0)
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![
            format!("{}/api2", server.uri()),
            format!("{}/api1", server.uri()),
        ]);
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(ICLR_TITLE)))
        .await;

    match res {
        Some(md) => {
            assert_eq!(md.source, "openreview");
            assert_eq!(
                md.title.as_deref(),
                Some("Transformers are Inherently Succinct")
            );
            assert_eq!(md.venue.as_deref(), Some("ICLR"));
            assert_eq!(md.year, Some(2026));
            assert_eq!(md.authors.len(), 3);
        }
        None => panic!("expected Resolved via OpenReview"),
    }
}

#[tokio::test]
async fn openreview_host_failure_still_falls_through_to_crossref() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    // Both OpenReview hosts are down.
    Mock::given(method("GET"))
        .and(path("/api1/notes/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![
            format!("{}/api1", server.uri()),
            format!("{}/api2", server.uri()),
        ]);
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(KGAT_TITLE)))
        .await;

    assert_eq!(res.expect("expected Crossref fallback").source, "crossref");
}

#[tokio::test]
async fn search_candidates_includes_openreview() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"message":{"items":[]}}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(OPENREVIEW_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![format!("{}/api2", server.uri())]);

    // A truncated query the 0.85 gate would reject still reaches the picker.
    let cands = resolver
        .search_candidates("Transformers are Inherently")
        .await;
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].source, "openreview");
    assert_eq!(
        cands[0].url.as_deref(),
        Some("https://openreview.net/forum?id=Yxz92UuPLQ")
    );
}

#[tokio::test]
async fn openreview_venue_beats_a_dblp_corr_preprint() {
    let server = MockServer::start().await;
    // DBLP has indexed the arXiv posting but not the ICLR volume yet.
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"result":{"hits":{"hit":[{"info":{
                "title":"Transformers are Inherently Succinct.",
                "venue":"CoRR","year":"2025","volume":"abs/2510.19315",
                "key":"journals/corr/abs-2510-19315",
                "doi":"10.48550/ARXIV.2510.19315",
                "type":"Informal and Other Publications"
            }}]}}}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(OPENREVIEW_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![format!("{}/api2", server.uri())]);
    let md = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(ICLR_TITLE)))
        .await
        .expect("expected Resolved");

    assert_eq!(md.source, "openreview");
    assert_eq!(md.venue.as_deref(), Some("ICLR"));
    assert_eq!(md.year, Some(2026));
    // The preprint's identifiers ride along rather than being dropped.
    assert_eq!(md.doi.as_deref(), Some("10.48550/ARXIV.2510.19315"));
    assert_eq!(md.dblp_key.as_deref(), Some("journals/corr/abs-2510-19315"));
}

#[tokio::test]
async fn a_dblp_corr_preprint_survives_when_openreview_has_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"result":{"hits":{"hit":[{"info":{
                "title":"Transformers are Inherently Succinct.",
                "venue":"CoRR","year":"2025",
                "key":"journals/corr/abs-2510-19315"
            }}]}}}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"notes":[],"count":0}"#))
        .mount(&server)
        .await;
    // Crossref is never consulted: DBLP already answered, if only as a preprint.
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .expect(0)
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![format!("{}/api2", server.uri())]);
    let md = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(ICLR_TITLE)))
        .await
        .expect("expected the DBLP preprint");

    assert_eq!(md.source, "dblp");
    assert_eq!(md.venue.as_deref(), Some("CoRR"));
}

#[tokio::test]
async fn identifiers_are_not_grafted_across_different_papers() {
    let server = MockServer::start().await;
    // Both records clear the 0.85 gate against the query (0.883 and 0.885) but
    // score only 0.771 against each other: similarity is not transitive, so
    // "each matched the query" is no evidence they are the same work.
    let query = "Scaling Laws for Neural Language Models of Protein Sequences";
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"result":{"hits":{"hit":[{"info":{
                "title":"Scaling Laws for Neural Language Models of Protein Chains.",
                "venue":"CoRR","year":"2019",
                "key":"journals/corr/abs-1900-00000",
                "doi":"10.48550/ARXIV.1900.00000"
            }}]}}}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api2/notes/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"notes":[{"id":"Yxz92UuPLQ","content":{
                "title":{"value":"Emergent Laws for Neural Language Models of Protein Sequences"},
                "venue":{"value":"ICLR 2026 Oral"}}}]}"#,
        ))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri())
        .with_openreview_bases(vec![format!("{}/api2", server.uri())]);
    let md = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(query)))
        .await
        .expect("expected the OpenReview record");

    assert_eq!(md.source, "openreview");
    assert_eq!(md.venue.as_deref(), Some("ICLR"));
    assert_eq!(md.doi, None, "a different paper's DOI must not be grafted");
    assert_eq!(md.dblp_key, None);
}

const GPT3_IMPOSTOR_FIXTURE: &str = support::fixture!("crossref_search_gpt3_impostor.json");
const GPT3_TITLE: &str = "Language Models are Few-Shot Learners";

/// The GPT-3 camera-ready's first page, as `pdftotext` renders it.
const GPT3_PAGE: &str = "Language Models are Few-Shot Learners\n\n\
Tom B. Brown∗ Benjamin Mann∗ Nick Ryder∗ Melanie Subbiah∗\n\
Jared Kaplan† Prafulla Dhariwal Arvind Neelakantan\n\nOpenAI\n\n\
Abstract\n\nRecent work has demonstrated substantial gains on many NLP tasks.";

/// Crossref carries a Swami Vivekananda University book chapter whose title is
/// character-for-character the GPT-3 paper's, and DBLP's own top hits for that
/// title are all *other* few-shot papers scoring below the gate — so the search
/// really does fall through to Crossref, where the impostor scores a perfect
/// 1.00 and wins on title alone.
#[tokio::test]
async fn a_same_titled_work_by_other_authors_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", GPT3_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(GPT3_IMPOSTOR_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    // The paper's own first page names none of the record's authors.
    let res = resolver
        .resolve(
            &Identifier::None,
            Some(TitleQuery {
                title: GPT3_TITLE,
                text: GPT3_PAGE,
            }),
        )
        .await;
    assert_eq!(res, None, "the impostor must not resolve");

    // Same fixture with nothing to corroborate against: the title gate alone
    // still accepts it, which is both the pre-fix behaviour and the documented
    // fail-open path for a PDF that extracts to nothing.
    let res = resolver
        .resolve(&Identifier::None, Some(TitleQuery::new(GPT3_TITLE)))
        .await;
    assert_eq!(
        res.and_then(|md| md.doi).as_deref(),
        Some("10.65525/svup.9788199778009.2026.224-230"),
    );
}

/// The other half of the same rule: a Springer LNCS proceedings paper is a
/// Crossref `book-chapter` exactly like the impostor above and matches its
/// query just as perfectly, so nothing about the record's *type* may be what
/// separates them — only that its authors are the ones on the page.
#[tokio::test]
async fn a_book_chapter_the_paper_corroborates_still_resolves() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#),
        )
        .mount(&server)
        .await;
    let coco = r#"{"message":{"items":[{
        "DOI":"10.1007/978-3-319-10602-1_48","type":"book-chapter",
        "title":["Microsoft COCO: Common Objects in Context"],
        "container-title":["Lecture Notes in Computer Science"],
        "issued":{"date-parts":[[2014]]},
        "author":[{"given":"Tsung-Yi","family":"Lin"},{"given":"Piotr","family":"Dollár"}]
    }]}}"#;
    Mock::given(method("GET"))
        .and(path("/works"))
        .respond_with(ResponseTemplate::new(200).set_body_string(coco))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(
            &Identifier::None,
            Some(TitleQuery {
                title: "Microsoft COCO: Common Objects in Context",
                // The diacritic survives neither pdftotext nor the record
                // unchanged; both sides fold to the same "dollar".
                text: "Microsoft COCO: Common Objects in Context\nTsung-Yi Lin, Piotr Dollár",
            }),
        )
        .await;
    assert_eq!(
        res.and_then(|md| md.doi).as_deref(),
        Some("10.1007/978-3-319-10602-1_48")
    );
}
