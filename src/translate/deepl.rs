use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::config::DeeplConfig;
use crate::http::{HttpClient, RetryPolicy};

pub(crate) struct DeeplTranslator {
    base_url: String,
    api_key: String,
    /// Shared retrying transport with a bounded timeout: `/api/translate`
    /// answers synchronously, so a hung DeepL endpoint must not hang it.
    http: HttpClient,
}

impl DeeplTranslator {
    pub(crate) fn new(cfg: &DeeplConfig) -> Option<Self> {
        let api_key = match cfg.resolve_key() {
            Ok(k) => k,
            Err(reason) => {
                tracing::warn!("{reason} — DeepL translate disabled");
                return None;
            }
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("building DeepL HTTP client");
        Some(Self {
            base_url: cfg.plan.base_url().to_string(),
            api_key,
            http: HttpClient::new(client, RetryPolicy::interactive()),
        })
    }

    #[cfg(test)]
    fn set_base_url_for_test(&mut self, url: String) {
        self.base_url = url;
    }
}

/// DeepL uses uppercase language codes (e.g. ZH, EN, DE).
fn deepl_target(target: &str) -> String {
    target.to_ascii_uppercase()
}

#[derive(Deserialize)]
struct DeeplResp {
    translations: Vec<DeeplItem>,
}
#[derive(Deserialize)]
struct DeeplItem {
    text: String,
    #[serde(default)]
    detected_source_language: Option<String>,
}

#[async_trait]
impl super::Translator for DeeplTranslator {
    async fn translate(&self, text: &str, target: &str) -> Result<(String, Option<String>)> {
        let url = format!("{}/v2/translate", self.base_url.trim_end_matches('/'));
        let req = self
            .http
            .post(&url)
            .header("Authorization", format!("DeepL-Auth-Key {}", self.api_key))
            .json(&serde_json::json!({
                "text": [text],
                "target_lang": deepl_target(target),
            }));
        let body = self
            .http
            .send_text(req)
            .await
            .context("DeepL request failed")?;
        let parsed: DeeplResp =
            serde_json::from_str(&body).context("DeepL response parse failed")?;
        let item = parsed
            .translations
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("DeepL returned no translations"))?;
        Ok((item.text, item.detected_source_language))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeeplConfig, DeeplPlan};
    use crate::translate::Translator;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn translator(base_url: String) -> DeeplTranslator {
        let cfg = DeeplConfig {
            api_key: Some("secret".into()),
            api_key_env: None,
            plan: DeeplPlan::Free,
        };
        let mut t = DeeplTranslator::new(&cfg).unwrap();
        t.set_base_url_for_test(base_url);
        t
    }

    #[test]
    fn no_translator_without_a_key() {
        let cfg = DeeplConfig {
            api_key_env: Some("XUEWEN_TEST_DEEPL_UNSET".into()),
            ..Default::default()
        };
        assert!(DeeplTranslator::new(&cfg).is_none());
    }

    #[tokio::test]
    async fn posts_to_deepl_and_parses_translation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .and(header("Authorization", "DeepL-Auth-Key secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "translations": [{ "detected_source_language": "EN", "text": "你好世界" }]
            })))
            .mount(&server)
            .await;

        let (text, src) = translator(server.uri())
            .translate("hello world", "zh")
            .await
            .unwrap();
        assert_eq!(text, "你好世界");
        assert_eq!(src.as_deref(), Some("EN"));
    }

    #[tokio::test]
    async fn retries_a_429_then_succeeds() {
        // Pins the wiring to the shared retrying transport.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "translations": [{ "text": "你好" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (text, src) = translator(server.uri())
            .translate("hello", "zh")
            .await
            .unwrap();
        assert_eq!(text, "你好");
        assert_eq!(src, None);
    }
}
