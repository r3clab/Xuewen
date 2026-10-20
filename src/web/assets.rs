use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::{LazyLock, Mutex, RwLock};

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

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

/// Below this, compression framing costs more than it saves.
const MIN_COMPRESS_BYTES: usize = 256;

/// Content codings we negotiate, best first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Encoding {
    Br,
    Gzip,
    Identity,
}

impl Encoding {
    fn name(self) -> Option<&'static str> {
        match self {
            Encoding::Br => Some("br"),
            Encoding::Gzip => Some("gzip"),
            Encoding::Identity => None,
        }
    }
}

/// Compressed bodies, keyed by path + coding and tagged with the content hash
/// they were produced from.
///
/// Compressing the 4.6 MB PDFium wasm costs ~100 ms of CPU while the loopback
/// transfer it saves costs ~40 ms, so doing it per request would be a net loss
/// for the local case and only pay off over a real network. Doing it once and
/// keeping the bytes wins in both. The stored hash is what keeps debug builds
/// honest: there rust-embed re-reads `frontend/dist` from disk on every request
/// (that is how a frontend rebuild is served without a Rust recompile), so a
/// rebuilt asset hashes differently and misses this cache rather than being
/// served stale.
static COMPRESSED: LazyLock<RwLock<CompressionCache>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

type CompressionCache = HashMap<(String, Encoding), Cached>;

/// The outcome of compressing one asset, tagged with the content hash it was
/// produced from. `body: None` records "compressing this made it bigger" so the
/// answer is remembered instead of being recomputed on every request.
struct Cached {
    hash: String,
    body: Option<Bytes>,
}

/// Keys with a warm pass already in flight, so N concurrent requests for a cold
/// asset queue one compression rather than N.
static WARMING: LazyLock<Mutex<HashSet<(String, Encoding)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Serve an embedded SPA asset by path, falling back to `index.html` for
/// client-side routes (so deep links work).
pub(crate) async fn static_handler(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(content) => respond(path, content, &headers),
        // A client-side route: hand back the SPA shell under its own name, so
        // it gets index.html's revalidate policy rather than the deep link's.
        None => match Assets::get("index.html") {
            Some(index) => respond("index.html", index, &headers),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

fn respond(path: &str, file: EmbeddedFile, req: &HeaderMap) -> Response {
    let hash = hex::encode(file.metadata.sha256_hash());
    // Weak, deliberately: the same resource is served as identity, gzip or br
    // depending on what the client accepts, and a strong ETag would be claiming
    // those byte streams are identical. Weak comparison is what `If-None-Match`
    // revalidation uses anyway.
    let etag = format!("W/\"{}\"", &hash[..32]);
    let cache_control = cache_control_for(path);

    let known = req
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|inm| matches_etag(inm, &etag));
    if known {
        return build(StatusCode::NOT_MODIFIED, None, None, &etag, cache_control);
    }

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mime = mime.as_ref();
    let raw = match file.data {
        // Release bakes the bytes into the binary, so this is a `&'static [u8]`
        // and the response body is a refcount bump rather than a 4.6 MB copy.
        std::borrow::Cow::Borrowed(b) => Bytes::from_static(b),
        std::borrow::Cow::Owned(v) => Bytes::from(v),
    };

    let wanted = if compressible(mime) && raw.len() >= MIN_COMPRESS_BYTES {
        negotiate(
            req.get(header::ACCEPT_ENCODING)
                .and_then(|v| v.to_str().ok()),
        )
    } else {
        Encoding::Identity
    };
    // Label the response with the coding actually applied, never with the one
    // that was merely wanted. Falling back to identity bytes while still
    // announcing `content-encoding: br` makes the client decode garbage — it
    // ends up with nothing at all, which is how an asset that simply refused to
    // shrink would take the whole page down.
    let (coding, body) = match wanted {
        Encoding::Identity => (Encoding::Identity, raw),
        c => match encoded(path, &hash, c, &raw) {
            Some(compressed) => (c, compressed),
            None => (Encoding::Identity, raw),
        },
    };

    build(
        StatusCode::OK,
        Some((mime, body)),
        coding.name(),
        &etag,
        cache_control,
    )
}

fn build(
    status: StatusCode,
    body: Option<(&str, Bytes)>,
    coding: Option<&str>,
    etag: &str,
    cache_control: &str,
) -> Response {
    let mut res = Response::builder().status(status);
    let headers = res.headers_mut().expect("a freshly built response builder");
    // Every representation of this URL varies by what the client accepts, so
    // shared caches must key on it — including on the 304, or a proxy could
    // hand a br body to a client that never asked for one.
    headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if let Ok(v) = HeaderValue::from_str(etag) {
        headers.insert(header::ETAG, v);
    }
    if let Ok(v) = HeaderValue::from_str(cache_control) {
        headers.insert(header::CACHE_CONTROL, v);
    }
    if let Some(coding) = coding {
        headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_str(coding).expect("a static coding token"),
        );
    }
    match body {
        Some((mime, bytes)) => {
            if let Ok(v) = HeaderValue::from_str(mime) {
                headers.insert(header::CONTENT_TYPE, v);
            }
            res.body(Body::from(bytes))
        }
        None => res.body(Body::empty()),
    }
    .expect("a response with a valid status and headers")
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

/// True when the body is worth compressing. An allowlist rather than a
/// denylist: guessing wrong here wastes CPU on bytes that are already
/// compressed (woff2, png) for no gain.
fn compressible(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/javascript"
                | "text/javascript"
                | "application/json"
                | "application/wasm"
                | "application/xml"
                | "image/svg+xml"
        )
}

/// Pick the best coding the client will take. Honours q-values, including the
/// `q=0` that spells "not this one" and a `*` fallback.
fn negotiate(accept: Option<&str>) -> Encoding {
    let Some(accept) = accept else {
        return Encoding::Identity;
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
    let br = br.or(star).unwrap_or(0.0);
    let gzip = gzip.or(star).unwrap_or(0.0);
    if br > 0.0 && br >= gzip {
        Encoding::Br
    } else if gzip > 0.0 {
        Encoding::Gzip
    } else {
        Encoding::Identity
    }
}

/// The cached compressed body for this exact content, compressing on first ask.
/// `None` means "serve it uncompressed" — either the codec failed or it made
/// the body bigger.
fn encoded(path: &str, hash: &str, coding: Encoding, raw: &Bytes) -> Option<Bytes> {
    let key = (path.to_string(), coding);
    if let Ok(cache) = COMPRESSED.read() {
        if let Some(cached) = cache.get(&key) {
            if cached.hash == hash {
                return cached.body.clone();
            }
        }
    }
    // Cold. Compress off the request path and serve identity meanwhile: brotli
    // on the 4.6 MB wasm takes ~550 ms, and doing that inline blocks a runtime
    // worker — with a handful of concurrent cold requests it starved unrelated
    // API calls from 0.7 ms to 523 ms. Nobody should ever wait for compression;
    // the next request gets the small body.
    warm(key, hash.to_string(), coding, raw.clone());
    None
}

/// Compress on the blocking pool, once per key. Silently does nothing outside a
/// Tokio runtime (unit tests call `warm_now` directly).
fn warm(key: (String, Encoding), hash: String, coding: Encoding, raw: Bytes) {
    {
        let Ok(mut inflight) = WARMING.lock() else {
            return;
        };
        // Already queued by another request for the same cold asset.
        if !inflight.insert(key.clone()) {
            return;
        }
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        if let Ok(mut inflight) = WARMING.lock() {
            inflight.remove(&key);
        }
        return;
    };
    handle.spawn_blocking(move || {
        warm_now(&key, hash, coding, &raw);
        if let Ok(mut inflight) = WARMING.lock() {
            inflight.remove(&key);
        }
    });
}

/// Compress and record the result — including the negative result, so an asset
/// that refuses to shrink is not recompressed on every request forever.
fn warm_now(key: &(String, Encoding), hash: String, coding: Encoding, raw: &[u8]) {
    let body = compress(coding, raw).filter(|out| out.len() < raw.len());
    if let Ok(mut cache) = COMPRESSED.write() {
        cache.insert(key.clone(), Cached { hash, body });
    }
}

fn compress(coding: Encoding, raw: &[u8]) -> Option<Bytes> {
    match coding {
        Encoding::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
            enc.write_all(raw).ok()?;
            Some(Bytes::from(enc.finish().ok()?))
        }
        // Quality 5, not brotli's maximum 11: on the 4.6 MB wasm, 11 takes tens
        // of seconds and the first client to ask would wear all of it, while 5
        // lands within a few percent of the same size in well under a second.
        Encoding::Br => {
            let mut out = Vec::new();
            let mut reader = brotli::CompressorReader::new(raw, 4096, 5, 22);
            std::io::copy(&mut reader, &mut out).ok()?;
            Some(Bytes::from(out))
        }
        Encoding::Identity => None,
    }
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
    fn negotiates_the_best_offered_coding() {
        assert_eq!(negotiate(Some("gzip, deflate, br")), Encoding::Br);
        assert_eq!(negotiate(Some("gzip, deflate")), Encoding::Gzip);
        assert_eq!(negotiate(Some("deflate")), Encoding::Identity);
        assert_eq!(negotiate(None), Encoding::Identity);
    }

    #[test]
    fn honours_q_values() {
        // q=0 is a refusal, not a weak preference.
        assert_eq!(negotiate(Some("br;q=0, gzip")), Encoding::Gzip);
        assert_eq!(negotiate(Some("br;q=0.1, gzip;q=0.9")), Encoding::Gzip);
        assert_eq!(negotiate(Some("br;q=0.9, gzip;q=0.1")), Encoding::Br);
        assert_eq!(negotiate(Some("*")), Encoding::Br);
        assert_eq!(negotiate(Some("gzip, *;q=0")), Encoding::Gzip);
    }

    #[test]
    fn etag_comparison_is_weak_and_list_aware() {
        let etag = "W/\"abc\"";
        assert!(matches_etag("W/\"abc\"", etag));
        // A client may echo the tag without the weakness prefix.
        assert!(matches_etag("\"abc\"", etag));
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

    #[test]
    fn only_compresses_what_benefits() {
        assert!(compressible("application/wasm"));
        assert!(compressible("text/javascript"));
        assert!(compressible("image/svg+xml"));
        assert!(!compressible("font/woff2"));
        assert!(!compressible("image/png"));
    }

    #[test]
    fn round_trips_both_codings() {
        let raw = "xuewen ".repeat(500).into_bytes();
        let gz = compress(Encoding::Gzip, &raw).unwrap();
        let br = compress(Encoding::Br, &raw).unwrap();
        assert!(gz.len() < raw.len());
        assert!(br.len() < raw.len());

        let mut out = Vec::new();
        let mut dec = flate2::read::GzDecoder::new(&gz[..]);
        std::io::copy(&mut dec, &mut out).unwrap();
        assert_eq!(out, raw);

        let mut out = Vec::new();
        let mut dec = brotli::Decompressor::new(&br[..], 4096);
        std::io::copy(&mut dec, &mut out).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn a_cold_asset_serves_identity_and_the_warm_pass_fills_the_cache() {
        let key = ("cold.js".to_string(), Encoding::Gzip);
        let raw = Bytes::from("a".repeat(4096));

        // Cold: nothing compressed yet, so the caller serves identity instead
        // of blocking the request on compression.
        assert!(encoded("cold.js", "hash-one", Encoding::Gzip, &raw).is_none());

        warm_now(&key, "hash-one".into(), Encoding::Gzip, &raw);
        let warmed = encoded("cold.js", "hash-one", Encoding::Gzip, &raw).unwrap();
        assert!(warmed.len() < raw.len());

        // A rebuilt asset hashes differently and must not be served stale —
        // this is what keeps debug builds' live reload honest.
        let changed = Bytes::from("b".repeat(4096));
        assert!(encoded("cold.js", "hash-two", Encoding::Gzip, &changed).is_none());
        warm_now(&key, "hash-two".into(), Encoding::Gzip, &changed);
        let after = encoded("cold.js", "hash-two", Encoding::Gzip, &changed).unwrap();

        let mut out = Vec::new();
        let mut dec = flate2::read::GzDecoder::new(&after[..]);
        std::io::copy(&mut dec, &mut out).unwrap();
        assert_eq!(out, changed);
    }

    #[test]
    fn input_that_refuses_to_shrink_is_remembered_as_identity() {
        // Random-ish bytes gzip larger than they started. The negative result
        // must be recorded, or every request recompresses it forever — and the
        // caller must then serve identity, never identity bytes labelled gzip.
        let key = ("noise.bin".to_string(), Encoding::Gzip);
        let raw: Bytes = (0..512u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect::<Vec<u8>>()
            .into();

        warm_now(&key, "h".into(), Encoding::Gzip, &raw);
        assert!(encoded("noise.bin", "h", Encoding::Gzip, &raw).is_none());
        let cache = COMPRESSED.read().unwrap();
        let cached = cache.get(&key).expect("the negative result is recorded");
        assert_eq!(cached.hash, "h");
        assert!(cached.body.is_none());
    }
}
