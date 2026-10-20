use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::config::DeeplConfig;
use crate::http::{HttpClient, RetryPolicy};

pub(crate) struct DeeplTranslator {
    base_url: String,
    api_key_env: Option<String>,
    /// Shared retrying transport with a bounded timeout: `/api/translate`
    /// answers synchronously, so a hung DeepL endpoint must not hang it.
    http: HttpClient,
}

impl DeeplTranslator {
    pub(crate) fn new(cfg: &DeeplConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("building DeepL HTTP client");
        Self {
            base_url: cfg.plan.base_url().to_string(),
            api_key_env: cfg.api_key_env.clone(),
            http: HttpClient::new(client, RetryPolicy::interactive()),
        }
    }

    #[cfg(test)]
    fn set_base_url_for_test(&mut self, url: String) {
        self.base_url = url;
    }

    fn key(&self) -> Result<String> {
        let env = self.api_key_env.as_deref().unwrap_or("DEEPL_API_KEY");
        std::env::var(env)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| anyhow!("DeepL API key env `{env}` is not set"))
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
        let key = self.key()?;
        let url = format!("{}/v2/translate", self.base_url.trim_end_matches('/'));
        let req = self
            .http
            .post(&url)
            .header("Authorization", format!("DeepL-Auth-Key {key}"))
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn posts_to_deepl_and_parses_translation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "translations": [{ "detected_source_language": "EN", "text": "你好世界" }]
            })))
            .mount(&server)
            .await;

        std::env::set_var("DEEPL_KEY_TEST", "secret");
        let cfg = DeeplConfig {
            api_key_env: Some("DEEPL_KEY_TEST".into()),
            plan: DeeplPlan::Free,
        };
        let mut t = DeeplTranslator::new(&cfg);
        t.set_base_url_for_test(server.uri());

        let (text, src) = t.translate("hello world", "zh").await.unwrap();
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

        std::env::set_var("DEEPL_KEY_TEST", "secret");
        let cfg = DeeplConfig {
            api_key_env: Some("DEEPL_KEY_TEST".into()),
            plan: DeeplPlan::Free,
        };
        let mut t = DeeplTranslator::new(&cfg);
        t.set_base_url_for_test(server.uri());

        let (text, src) = t.translate("hello", "zh").await.unwrap();
        assert_eq!(text, "你好");
        assert_eq!(src, None);
    }
}
