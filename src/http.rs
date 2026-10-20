//! Crate-wide HTTP transport: the retrying client shared by the resolvers,
//! the LLM chat/embeddings clients, and the daily feed, plus the outbound
//! user-agent and URL-encoding helpers those callers share.

use anyhow::{anyhow, Result};
use std::time::Duration;

/// The polite user-agent for outbound API calls: `xuewen/<version>`, plus a
/// `(mailto:…)` contact when one is configured — the mailto form is what
/// routes Crossref/Unpaywall requests into their polite pools.
pub(crate) fn user_agent(email: Option<&str>) -> String {
    const BASE: &str = concat!("xuewen/", env!("CARGO_PKG_VERSION"));
    match email {
        Some(email) => format!("{BASE} (mailto:{email})"),
        None => BASE.to_string(),
    }
}

/// Percent-encode a DOI for embedding in a URL path. `/` stays literal (DOIs
/// contain it structurally and the APIs accept it raw), so a plain DOI
/// produces a byte-identical path; encoded are only the characters the URL
/// parser cannot rescue — `#` (fragment cut), `?` (query cut), `%`
/// (double-decode) — plus the parser-fragile CONTROLS/space/`"`/`<`/`>`/`` ` ``
/// set. Legacy SICI DOIs (`10.1002/(SICI)…<1893::AID-APP1>…;2-9`) are real.
pub(crate) fn encode_doi_path(doi: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    const DOI_PATH: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'%');
    utf8_percent_encode(doi, DOI_PATH).to_string()
}

/// How `HttpClient` retries transient HTTP failures.
#[derive(Debug, Clone)]
pub(crate) struct RetryPolicy {
    /// Total attempts including the first (default 4 = initial + 3 retries).
    max_attempts: u32,
    /// Exponential back-off base: the delay before retry `n` is `base_delay * 2^n`.
    base_delay: Duration,
    /// Upper bound on any single back-off sleep.
    max_delay: Duration,
    /// Upper bound on honoring a server's Retry-After header.
    retry_after_cap: Duration,
}

impl RetryPolicy {
    /// Polite defaults for the real bibliographic APIs.
    pub(crate) fn production() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
            retry_after_cap: Duration::from_secs(30),
        }
    }

    /// Short budget for interactive use (web import): a single quick retry so a
    /// synchronous upload response never stalls for minutes.
    pub(crate) fn interactive() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(2),
            retry_after_cap: Duration::from_secs(2),
        }
    }

    /// Near-zero delays so tests exercise the retry paths without real sleeps.
    pub(crate) fn fast_for_tests() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            retry_after_cap: Duration::from_millis(5),
        }
    }

    /// OpenAI-compatible API budget (chat/embeddings): matches the retry
    /// behavior the LLM clients hand-rolled before consolidating here
    /// (3 attempts, 500ms doubling), plus Retry-After support for free.
    pub(crate) fn llm() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
            retry_after_cap: Duration::from_secs(30),
        }
    }
}

/// A `reqwest::Client` wrapper that retries transient `429`/`5xx`/network
/// failures with bounded exponential back-off, then returns the body text.
/// A non-retryable status (e.g. 404) or an exhausted budget returns `Err`, so
/// callers still degrade to `Unresolved` exactly as before.
pub(crate) struct HttpClient {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl HttpClient {
    pub(crate) fn new(client: reqwest::Client, retry: RetryPolicy) -> Self {
        Self { client, retry }
    }

    /// Start a GET request the caller can add `.query(...)` to before handing it
    /// to `send_text`.
    pub(crate) fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    /// Start a POST request (JSON APIs: chat, embeddings) for `send_text`.
    pub(crate) fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    /// GET `url` with retries, returning the body text.
    pub(crate) async fn get_text(&self, url: &str) -> Result<String> {
        self.send_text(self.get(url)).await
    }

    /// Send `req` with retries, returning the body text. `req` must be cloneable
    /// (true for the query-only GETs used here).
    pub(crate) async fn send_text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        let mut attempt: u32 = 0;
        loop {
            let last = attempt + 1 >= self.retry.max_attempts;
            let this = req
                .try_clone()
                .ok_or_else(|| anyhow!("request body is not retryable"))?;
            match this.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp.text().await?);
                    }
                    if last || !is_retryable_status(status.as_u16()) {
                        // Include a body snippet: API error payloads (e.g. an
                        // OpenAI "invalid key" JSON) are the useful part.
                        let snippet: String = resp
                            .text()
                            .await
                            .unwrap_or_default()
                            .chars()
                            .take(200)
                            .collect();
                        return Err(if snippet.trim().is_empty() {
                            anyhow!("HTTP {status}")
                        } else {
                            anyhow!("HTTP {status}: {snippet}")
                        });
                    }
                    let delay = self.delay_for(attempt, retry_after(&resp));
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    if last || !(e.is_timeout() || e.is_connect()) {
                        return Err(e.into());
                    }
                    let delay = self.delay_for(attempt, None);
                    tokio::time::sleep(delay).await;
                }
            }
            attempt += 1;
        }
    }

    /// The back-off before the next retry: `Retry-After` when supplied, otherwise
    /// exponential; both capped.
    fn delay_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        match retry_after {
            Some(d) => d.min(self.retry.retry_after_cap),
            None => {
                let shift = attempt.min(16); // guard the shift against overflow
                self.retry
                    .base_delay
                    .saturating_mul(1u32 << shift)
                    .min(self.retry.max_delay)
            }
        }
    }
}

/// Transient statuses worth retrying (rate limit + transient server errors).
fn is_retryable_status(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// Parse the delta-seconds form of `Retry-After`. The HTTP-date form is ignored
/// (falls through to exponential back-off).
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> HttpClient {
        HttpClient::new(reqwest::Client::new(), RetryPolicy::fast_for_tests())
    }

    #[test]
    fn user_agent_carries_version_and_optional_mailto() {
        let bare = user_agent(None);
        assert_eq!(bare, format!("xuewen/{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(
            user_agent(Some("me@uchicago.edu")),
            format!("{bare} (mailto:me@uchicago.edu)")
        );
    }

    #[test]
    fn encode_doi_path_leaves_plain_dois_alone() {
        assert_eq!(
            encode_doi_path("10.1145/3292500.3330701"),
            "10.1145/3292500.3330701"
        );
        // SICI-style reserved characters survive un-mangled…
        assert_eq!(
            encode_doi_path("10.1002/(SICI)1097-4628(19980620)68:12<1893::AID-APP1>3.0.CO;2-9"),
            "10.1002/(SICI)1097-4628(19980620)68:12%3C1893::AID-APP1%3E3.0.CO;2-9"
        );
        // …and the truly path-breaking characters are encoded.
        assert_eq!(encode_doi_path("10.1/a#b?c%d"), "10.1/a%23b%3Fc%25d");
    }

    #[tokio::test]
    async fn retries_429_then_succeeds() {
        let server = MockServer::start().await;
        // First hit → 429 (stops matching after 1), later hits → 200.
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let body = client()
            .get_text(&format!("{}/x", server.uri()))
            .await
            .unwrap();
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429))
            .expect(4) // RetryPolicy::max_attempts
            .mount(&server)
            .await;

        let out = client().get_text(&format!("{}/x", server.uri())).await;
        assert!(out.is_err());
        // MockServer verifies on drop that exactly 4 requests arrived.
    }

    #[tokio::test]
    async fn does_not_retry_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1) // no retry on a non-retryable status
            .mount(&server)
            .await;

        assert!(client()
            .get_text(&format!("{}/x", server.uri()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn honors_retry_after_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        assert_eq!(
            client()
                .get_text(&format!("{}/x", server.uri()))
                .await
                .unwrap(),
            "ok"
        );
    }

    #[tokio::test]
    async fn retries_503() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        assert_eq!(
            client()
                .get_text(&format!("{}/x", server.uri()))
                .await
                .unwrap(),
            "ok"
        );
    }

    #[tokio::test]
    async fn retries_then_gives_up_on_connect_error() {
        // Nothing listens on port 1 → connect error (retryable) each attempt.
        let out = client().get_text("http://127.0.0.1:1/x").await;
        assert!(out.is_err());
    }

    #[test]
    fn delay_for_caps_both_paths() {
        let http = HttpClient::new(
            reqwest::Client::new(),
            RetryPolicy {
                max_attempts: 4,
                base_delay: Duration::from_secs(10),
                max_delay: Duration::from_secs(1),
                retry_after_cap: Duration::from_secs(30),
            },
        );
        // Exponential 10s * 2^3 = 80s, clamped to max_delay (1s).
        assert_eq!(http.delay_for(3, None), Duration::from_secs(1));
        // Retry-After 120s clamped to the policy's 30s retry_after_cap.
        assert_eq!(
            http.delay_for(0, Some(Duration::from_secs(120))),
            Duration::from_secs(30)
        );

        // Under an interactive-like policy the cap is much tighter, so a
        // single 429 can't eat the whole interactive budget.
        let quick = HttpClient::new(reqwest::Client::new(), RetryPolicy::interactive());
        assert_eq!(
            quick.delay_for(0, Some(Duration::from_secs(120))),
            Duration::from_secs(2)
        );
    }
}
