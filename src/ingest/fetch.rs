use anyhow::{anyhow, Result};
use std::time::Duration;

/// Downloads PDF bytes. `client` follows redirects (open URLs). `no_redirect`
/// does NOT auto-follow: the proxied path drives redirects manually so it can
/// re-attach the `Cookie` header through the EZproxy chain (reqwest strips
/// `Cookie` on cross-host redirects otherwise).
pub struct Fetcher {
    client: reqwest::Client,
    no_redirect: reqwest::Client,
    proxy_login_url: Option<String>,
    proxy_host: Option<String>,
}

/// Percent-encode a URL for use as the `?url=` value of the EZproxy login.
/// Encodes everything except the RFC3986 unreserved set.
fn encode_target(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
    // NON_ALPHANUMERIC minus the RFC3986 unreserved marks = encode everything
    // except [A-Za-z0-9._~-], matching EZproxy's expectations.
    const TARGET: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    utf8_percent_encode(s, TARGET).to_string()
}

impl Fetcher {
    pub fn new(proxy_login_url: Option<String>) -> Result<Self> {
        let ua = crate::http::user_agent(None);
        let client = reqwest::Client::builder()
            .user_agent(&ua)
            .timeout(Duration::from_secs(30))
            .build()?;
        let no_redirect = reqwest::Client::builder()
            .user_agent(&ua)
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let proxy_host = proxy_login_url
            .as_deref()
            .and_then(|u| reqwest::Url::parse(u).ok())
            .and_then(|u| u.host_str().map(str::to_string));
        Ok(Self {
            client,
            no_redirect,
            proxy_login_url,
            proxy_host,
        })
    }

    /// Whether `proxy_login_url` is configured (paywalled fetch is possible).
    pub(crate) fn proxy_enabled(&self) -> bool {
        self.proxy_login_url.is_some()
    }

    /// Host of the configured EZproxy (`None` without `[proxy]`). Surfaced by
    /// GET /api/settings so the UI can say which host a cookie is for instead
    /// of hardcoding one deployment's university.
    pub(crate) fn proxy_host(&self) -> Option<&str> {
        self.proxy_host.as_deref()
    }

    /// GET `url` following redirects. `Ok(Some(bytes))` if the body is a PDF,
    /// `Ok(None)` if the fetch succeeded but the body is not a PDF, `Err` on a
    /// network/HTTP error.
    pub(crate) async fn fetch_plain(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let resp = self.client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!("HTTP {} fetching {url}", resp.status()));
        }
        let bytes = read_body_capped(resp).await?;
        Ok(is_pdf(&bytes).then_some(bytes))
    }

    /// GET the proxied `target_url` carrying `cookie`, following redirects
    /// manually (so the cookie rides along) but only to the proxy host or its
    /// subdomains. `Ok(Some(bytes))` iff a PDF is returned; `Ok(None)` when the
    /// body is non-PDF (typically an expired-session login page).
    pub(crate) async fn fetch_proxied(
        &self,
        target_url: &str,
        cookie: &str,
    ) -> Result<Option<Vec<u8>>> {
        let login = self
            .proxy_login_url
            .as_deref()
            .ok_or_else(|| anyhow!("proxy not configured"))?;
        let mut url = format!("{login}{}", encode_target(target_url));
        for _ in 0..10 {
            let resp = self
                .no_redirect
                .get(&url)
                .header(reqwest::header::COOKIE, cookie)
                .send()
                .await?;
            let status = resp.status();
            if status.is_redirection() {
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow!("redirect without Location"))?;
                let next = reqwest::Url::parse(&url)?.join(loc)?;
                if !self.host_allowed(next.host_str()) {
                    // Redirected off the proxy domain (e.g. to an IdP): treat as
                    // an expired session rather than following and leaking the cookie.
                    return Ok(None);
                }
                url = next.to_string();
                continue;
            }
            if !status.is_success() {
                return Err(anyhow!("HTTP {status} via proxy for {target_url}"));
            }
            let bytes = read_body_capped(resp).await?;
            return Ok(is_pdf(&bytes).then_some(bytes));
        }
        Err(anyhow!("too many redirects via proxy for {target_url}"))
    }

    /// A redirect target host is allowed iff it equals the proxy host or is a
    /// subdomain of it (`dl-acm-org.proxy.uchicago.edu` for `proxy.uchicago.edu`).
    fn host_allowed(&self, host: Option<&str>) -> bool {
        match (host, self.proxy_host.as_deref()) {
            (Some(h), Some(p)) => h == p || h.ends_with(&format!(".{p}")),
            _ => false,
        }
    }
}

/// Whether bytes begin with the PDF magic marker.
fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// Ceiling on a fetched document, matching the multipart upload limit
/// (`DefaultBodyLimit`, web/handlers/import.rs). URL imports otherwise buffer
/// the whole response with no bound; a hostile or misconfigured server could
/// stream an arbitrarily large body into memory.
const MAX_FETCH_BYTES: u64 = 50 * 1024 * 1024;

/// Read a response body into memory, refusing anything over `MAX_FETCH_BYTES`.
/// Rejects early on an oversized declared `Content-Length`, then enforces the
/// same ceiling while streaming (the header can lie or be absent).
async fn read_body_capped(mut resp: reqwest::Response) -> Result<Vec<u8>> {
    let too_big = || {
        anyhow!(
            "document exceeds the {} MB import limit",
            MAX_FETCH_BYTES / (1024 * 1024)
        )
    };
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_FETCH_BYTES)
    {
        return Err(too_big());
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() as u64 + chunk.len() as u64 > MAX_FETCH_BYTES {
            return Err(too_big());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

#[cfg(test)]
mod fetch_tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PDF: &[u8] = b"%PDF-1.4\nfake body\n";

    #[test]
    fn encode_target_percent_encodes_reserved() {
        assert_eq!(
            encode_target("https://dl.acm.org/doi/pdf/10.1145/3292500.3330701"),
            "https%3A%2F%2Fdl.acm.org%2Fdoi%2Fpdf%2F10.1145%2F3292500.3330701"
        );
    }

    #[tokio::test]
    async fn fetch_plain_returns_pdf_and_rejects_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/nope"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
            .mount(&server)
            .await;
        let f = Fetcher::new(None).unwrap();
        assert_eq!(
            f.fetch_plain(&format!("{}/ok.pdf", server.uri()))
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
        assert_eq!(
            f.fetch_plain(&format!("{}/nope", server.uri()))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn fetch_plain_rejects_oversized_body() {
        let server = MockServer::start().await;
        // set_body_bytes sets Content-Length to the true size, so this trips
        // the early declared-length reject before any body is buffered.
        let big = vec![b'x'; (MAX_FETCH_BYTES + 1) as usize];
        Mock::given(method("GET"))
            .and(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(big))
            .mount(&server)
            .await;
        let f = Fetcher::new(None).unwrap();
        let err = f
            .fetch_plain(&format!("{}/big", server.uri()))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("import limit"), "got: {err}");
    }

    #[tokio::test]
    async fn fetch_proxied_requires_cookie_and_verifies_pdf() {
        let server = MockServer::start().await;
        // With the right cookie → the mock returns the PDF.
        Mock::given(method("GET"))
            .and(header("cookie", "ezproxy=good"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        // Any other request (no/incorrect cookie) → an HTML login page.
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>Shibboleth</html>"))
            .mount(&server)
            .await;

        let login = format!("{}/login?url=", server.uri());
        let f = Fetcher::new(Some(login)).unwrap();

        let target = "https://dl.acm.org/doi/pdf/10.1145/x";
        assert_eq!(
            f.fetch_proxied(target, "ezproxy=good")
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
        // Wrong cookie → non-PDF body → None (caller maps to CookieExpired).
        assert_eq!(
            f.fetch_proxied(target, "ezproxy=stale").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn fetch_proxied_follows_redirect_reattaching_cookie() {
        let server = MockServer::start().await;
        // Hop 1: the login URL redirects (302) to a rewritten path.
        Mock::given(method("GET"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "/rewritten/paper.pdf"),
            )
            .mount(&server)
            .await;
        // Hop 2: the rewritten path serves the PDF ONLY when the cookie rode along
        // (proves the manual redirect loop re-attaches the Cookie header).
        Mock::given(method("GET"))
            .and(path("/rewritten/paper.pdf"))
            .and(header("cookie", "ezproxy=good"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        let f = Fetcher::new(Some(format!("{}/login?url=", server.uri()))).unwrap();
        assert_eq!(
            f.fetch_proxied("https://dl.acm.org/doi/pdf/10.1145/x", "ezproxy=good")
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
    }
}
