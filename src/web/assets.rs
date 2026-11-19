use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};
use sha2::{Digest, Sha256};

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Assets;

/// Vite fingerprints everything it emits under `assets/` with a content hash,
/// so those URLs can never change meaning — cache them for a year and never
/// revalidate. This is what keeps the reader tier (the PDFium wasm, the worker
/// engine chunk, the viewer chunk: ~5.8 MB together) off the wire on every
/// single page load.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Served from a stable URL, so it has to be revalidated or a deploy would stay
/// invisible until the cache expired. `no-cache` still *caches* — it only
/// forces the conditional request, which the ETag below answers with a
/// bodiless 304.
const REVALIDATE: &str = "public, no-cache";

/// The shell of a binary built without the frontend: the API works, and the
/// page says how to get the real UI.
const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Xuewen</title></head><body style=\"font-family:system-ui;max-width:40rem;margin:4rem auto;padding:0 1rem\">\
<h1>Xuewen</h1><p>The API is running. The web UI has not been built yet — run \
<code>npm --prefix frontend run build</code> and rebuild.</p>\
<p>Try <a href=\"/api/stats\">/api/stats</a>.</p></body></html>";

/// The content codings the frontend build stores beside an asset
/// (`frontend/vite.config.ts` writes `<file>.br` and `<file>.gz`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coding {
    Br,
    Gzip,
}

impl Coding {
    fn token(self) -> &'static str {
        match self {
            Coding::Br => "br",
            Coding::Gzip => "gzip",
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Coding::Br => ".br",
            Coding::Gzip => ".gz",
        }
    }
}

/// One stored form of an asset: its bytes as sent, and the coding they carry.
struct Representation {
    body: Bytes,
    hash: [u8; 32],
    coding: Option<Coding>,
}

impl Representation {
    fn embedded(file: EmbeddedFile, coding: Option<Coding>) -> Self {
        let body = match file.data {
            // Release bakes the bytes into the binary, so this is a `&'static
            // [u8]` and the response body is a refcount bump rather than a
            // 4.6 MB copy.
            std::borrow::Cow::Borrowed(b) => Bytes::from_static(b),
            std::borrow::Cow::Owned(v) => Bytes::from(v),
        };
        Self {
            body,
            hash: file.metadata.sha256_hash(),
            coding,
        }
    }

    fn placeholder() -> Self {
        Self {
            body: Bytes::from_static(PLACEHOLDER.as_bytes()),
            hash: Sha256::digest(PLACEHOLDER).into(),
            coding: None,
        }
    }
}

/// Serve an embedded SPA asset by path, falling back to `index.html` for
/// client-side routes (so deep links work).
pub(crate) async fn static_handler(uri: Uri, headers: HeaderMap) -> Response {
    let accept = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());
    let path = match uri.path().trim_start_matches('/') {
        "" => "index.html",
        path => path,
    };
    let find = |path| pick(path, accept, Assets::get).map(|(f, c)| Representation::embedded(f, c));
    match find(path) {
        Some(found) => respond(path, found, &headers),
        None if names_a_file(path) => StatusCode::NOT_FOUND.into_response(),
        // A client-side route: hand back the SPA shell under its own name, so
        // it gets index.html's revalidate policy rather than the deep link's.
        None => {
            let shell = find("index.html").unwrap_or_else(Representation::placeholder);
            respond("index.html", shell, &headers)
        }
    }
}

/// Client-side routes are extensionless; a missing `app-1a2b.js` is a stale
/// or broken build, and answering it with the HTML shell would only surface
/// as a MIME-type error in the browser.
fn names_a_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
}

/// The stored form of `path` to send: the client's most preferred coding
/// that has a sibling, else the file itself. Siblings are looked up first so
/// a compressed hit never reads the identity bytes too (debug builds read
/// every lookup from disk).
fn pick<T>(
    path: &str,
    accept: Option<&str>,
    get: impl Fn(&str) -> Option<T>,
) -> Option<(T, Option<Coding>)> {
    acceptable(accept)
        .into_iter()
        .find_map(|c| get(&format!("{path}{}", c.suffix())).map(|f| (f, Some(c))))
        .or_else(|| get(path).map(|f| (f, None)))
}

fn respond(path: &str, repr: Representation, req: &HeaderMap) -> Response {
    // Strong and per representation: each coding is its own byte stream with
    // its own hash, so a cached br body can never validate a gzip one.
    let etag = format!("\"{}\"", hex::encode(&repr.hash[..16]));
    let res = Response::builder()
        // Every representation of this URL varies by what the client accepts,
        // so shared caches must key on it — including on the 304, or a proxy
        // could hand a br body to a client that never asked for one.
        .header(header::VARY, "accept-encoding")
        .header(header::ETAG, &etag)
        .header(header::CACHE_CONTROL, cache_control_for(path));
    let known = req
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|inm| matches_etag(inm, &etag));
    let res = if known {
        res.status(StatusCode::NOT_MODIFIED).body(Body::empty())
    } else {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let res = res.header(header::CONTENT_TYPE, mime.as_ref());
        match repr.coding {
            Some(c) => res.header(header::CONTENT_ENCODING, c.token()),
            None => res,
        }
        .body(Body::from(repr.body))
    };
    res.expect("static headers, a hex ETag and a guessed MIME type are all valid")
}

/// Only Vite's fingerprinted output can be cached without revalidation: its
/// name changes whenever its bytes do. Everything else keeps a stable URL and
/// so must be revalidated, however large it is.
fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        IMMUTABLE
    } else {
        REVALIDATE
    }
}

/// The stored codings the client accepts, most preferred first (brotli on a
/// tie). Honours q-values, including the `q=0` that spells "not this one" and
/// a `*` fallback.
fn acceptable(accept: Option<&str>) -> Vec<Coding> {
    let Some(accept) = accept else {
        return Vec::new();
    };
    let (mut br, mut gzip, mut star) = (None, None, None);
    for part in accept.split(',') {
        let mut fields = part.split(';');
        let token = fields
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let q = fields
            .filter_map(|f| {
                f.trim()
                    .strip_prefix("q=")
                    .and_then(|v| v.trim().parse().ok())
            })
            .next()
            .unwrap_or(1.0f32);
        match token.as_str() {
            "br" => br = Some(q),
            "gzip" => gzip = Some(q),
            "*" => star = Some(q),
            _ => {}
        }
    }
    let mut ranked: Vec<(f32, Coding)> = [(br, Coding::Br), (gzip, Coding::Gzip)]
        .into_iter()
        .map(|(q, c)| (q.or(star).unwrap_or(0.0), c))
        .filter(|(q, _)| *q > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    ranked.into_iter().map(|(_, c)| c).collect()
}

/// RFC 9110 `If-None-Match`: a comma-separated list, `*` matching anything, and
/// weak comparison (the `W/` prefix is not part of the comparison).
fn matches_etag(if_none_match: &str, etag: &str) -> bool {
    let want = strip_weak(etag);
    if_none_match.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || strip_weak(candidate) == want
    })
}

fn strip_weak(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_the_offered_codings() {
        assert_eq!(
            acceptable(Some("gzip, deflate, br")),
            [Coding::Br, Coding::Gzip]
        );
        assert_eq!(acceptable(Some("gzip, deflate")), [Coding::Gzip]);
        assert!(acceptable(Some("deflate")).is_empty());
        assert!(acceptable(None).is_empty());
    }

    #[test]
    fn honours_q_values() {
        // q=0 is a refusal, not a weak preference.
        assert_eq!(acceptable(Some("br;q=0, gzip")), [Coding::Gzip]);
        assert_eq!(
            acceptable(Some("br;q=0.1, gzip;q=0.9")),
            [Coding::Gzip, Coding::Br]
        );
        assert_eq!(
            acceptable(Some("br;q=0.9, gzip;q=0.1")),
            [Coding::Br, Coding::Gzip]
        );
        assert_eq!(acceptable(Some("*")), [Coding::Br, Coding::Gzip]);
        assert_eq!(acceptable(Some("gzip, *;q=0")), [Coding::Gzip]);
    }

    #[test]
    fn picks_the_most_preferred_stored_sibling() {
        let stored = ["app.js", "app.js.gz", "font.woff2"];
        let get = |p: &str| stored.contains(&p).then(|| p.to_string());

        // No br sibling: the next acceptable coding that has one wins.
        assert_eq!(
            pick("app.js", Some("br, gzip"), get),
            Some(("app.js.gz".into(), Some(Coding::Gzip)))
        );
        assert_eq!(
            pick("app.js", Some("br"), get),
            Some(("app.js".into(), None))
        );
        assert_eq!(pick("app.js", None, get), Some(("app.js".into(), None)));
        // Nothing stored beside it (already compressed, or too small).
        assert_eq!(
            pick("font.woff2", Some("br, gzip"), get),
            Some(("font.woff2".into(), None))
        );
        assert_eq!(pick("missing.js", Some("gzip"), get), None);
    }

    #[test]
    fn etag_comparison_is_weak_and_list_aware() {
        let etag = "\"abc\"";
        assert!(matches_etag("\"abc\"", etag));
        // Weak comparison: the weakness prefix is ignored.
        assert!(matches_etag("W/\"abc\"", etag));
        assert!(matches_etag("*", etag));
        assert!(matches_etag("\"zzz\", W/\"abc\"", etag));
        assert!(!matches_etag("\"zzz\"", etag));
        assert!(!matches_etag("", etag));
    }

    #[test]
    fn only_fingerprinted_paths_are_cached_without_revalidation() {
        assert_eq!(cache_control_for("assets/index-abc123.js"), IMMUTABLE);
        assert_eq!(cache_control_for("assets/pdfium-abc123.wasm"), IMMUTABLE);
        // Stable URLs: a deploy has to be able to change what they mean.
        assert_eq!(cache_control_for("index.html"), REVALIDATE);
        assert_eq!(cache_control_for("favicon.svg"), REVALIDATE);
    }
}
