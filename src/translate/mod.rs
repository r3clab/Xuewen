//! Translate-on-selection: translate arbitrary selected text into a configured
//! target language via the LLM or DeepL. Feature is off unless a provider is
//! configured (mirrors summary/citations).

use anyhow::Result;
use async_trait::async_trait;

use crate::config::{Config, TranslateConfig, TranslateProvider, TranslateTrigger};
use crate::llm::LlmClient;

mod deepl; // Task 3

/// A finished translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Translated {
    pub(crate) text: String,
    pub(crate) provider: TranslateProvider,
    pub(crate) source_lang: Option<String>,
    pub(crate) target_lang: String,
}

#[async_trait]
trait Translator: Send + Sync {
    /// Returns (translated_text, detected_source_language).
    async fn translate(&self, text: &str, target: &str) -> Result<(String, Option<String>)>;
}

/// LLM-backed translator: reuses the shared OpenAI-compatible client.
struct LlmTranslator {
    client: LlmClient,
}

#[async_trait]
impl Translator for LlmTranslator {
    async fn translate(&self, text: &str, target: &str) -> Result<(String, Option<String>)> {
        let system = format!(
            "You are a translation engine. Translate the user's text into {target}. \
             Output ONLY the translation, with no quotes, notes, or preamble. \
             Preserve technical terms, math, and inline code."
        );
        let out = self.client.complete(&system, text).await?;
        Ok((out.trim().to_string(), None))
    }
}

pub struct TranslateService {
    cfg: TranslateConfig,
    llm: Option<LlmTranslator>,
    deepl: Option<deepl::DeeplTranslator>,
}

impl TranslateService {
    /// Build from config. `None` when no provider is configured. Config
    /// mistakes warn here (mirroring summary/daily/citations) so they are
    /// diagnosable from the startup log instead of silently changing behavior.
    pub fn from_config(cfg: &Config) -> Option<Self> {
        let tcfg = cfg.translate.clone();

        // LLM provider: present iff [ai.translate] resolves to a client.
        let llm = match cfg.ai.translate.as_ref() {
            None => None,
            Some(d) => match LlmClient::from_resolved(&cfg.ai.resolve(d)) {
                Some(client) => Some(LlmTranslator { client }),
                None => {
                    tracing::warn!(
                        "[ai.translate] has no model or API key — LLM translate disabled"
                    );
                    None
                }
            },
        };

        // DeepL provider: present iff [translate.deepl] is configured.
        let deepl = tcfg.deepl.as_ref().map(deepl::DeeplTranslator::new);

        if llm.is_none() && deepl.is_none() {
            return None;
        }
        if let Some(p) = tcfg.provider {
            let available = match p {
                TranslateProvider::Llm => llm.is_some(),
                TranslateProvider::Deepl => deepl.is_some(),
            };
            if !available {
                tracing::warn!(
                    "[translate].provider = {p:?} is not available — translations fall back to the other provider"
                );
            }
        }
        Some(Self {
            cfg: tcfg,
            llm,
            deepl,
        })
    }

    pub(crate) fn providers(&self) -> Vec<TranslateProvider> {
        let mut v = Vec::new();
        if self.llm.is_some() {
            v.push(TranslateProvider::Llm);
        }
        if self.deepl.is_some() {
            v.push(TranslateProvider::Deepl);
        }
        v
    }

    pub(crate) fn default_provider(&self) -> TranslateProvider {
        // Explicit config wins if that provider is available; else first available.
        if let Some(p) = self.cfg.provider {
            if self.provider_available(p) {
                return p;
            }
        }
        if self.llm.is_some() {
            TranslateProvider::Llm
        } else {
            TranslateProvider::Deepl
        }
    }

    fn provider_available(&self, p: TranslateProvider) -> bool {
        match p {
            TranslateProvider::Llm => self.llm.is_some(),
            TranslateProvider::Deepl => self.deepl.is_some(),
        }
    }

    pub(crate) fn target_lang(&self) -> &str {
        &self.cfg.target_lang
    }

    pub(crate) fn trigger(&self) -> TranslateTrigger {
        self.cfg.trigger
    }

    /// Translate `text`. `target_lang`/`provider` default to config.
    pub(crate) async fn translate(
        &self,
        text: &str,
        target_lang: Option<&str>,
        provider: Option<TranslateProvider>,
    ) -> Result<Translated> {
        let target = target_lang.unwrap_or(&self.cfg.target_lang).to_string();
        let provider = provider.unwrap_or_else(|| self.default_provider());
        let t: &dyn Translator = match provider {
            TranslateProvider::Llm => self
                .llm
                .as_ref()
                .map(|x| x as &dyn Translator)
                .ok_or_else(|| anyhow::anyhow!("LLM translate provider not configured"))?,
            TranslateProvider::Deepl => self
                .deepl
                .as_ref()
                .map(|x| x as &dyn Translator)
                .ok_or_else(|| anyhow::anyhow!("DeepL translate provider not configured"))?,
        };
        let (out, src) = t.translate(text, &target).await?;
        Ok(Translated {
            text: out,
            provider,
            source_lang: src,
            target_lang: target,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg_from(toml: &str) -> Config {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn disabled_without_any_provider() {
        let cfg = cfg_from(
            "inbox_dir='/i'\nlibrary_root='/l'\ndatabase_url='sqlite::memory:'\n[translate]\n",
        );
        assert!(TranslateService::from_config(&cfg).is_none());
    }

    #[test]
    fn llm_only_defaults_to_llm_provider() {
        let cfg = cfg_from(
            "inbox_dir='/i'\nlibrary_root='/l'\ndatabase_url='sqlite::memory:'\n\
             [ai]\napi_key='k'\n[ai.translate]\nmodel='gpt-4o-mini'\n",
        );
        let svc = TranslateService::from_config(&cfg).unwrap();
        assert_eq!(svc.providers(), vec![TranslateProvider::Llm]);
        assert_eq!(svc.default_provider(), TranslateProvider::Llm);
        assert_eq!(svc.target_lang(), "zh");
    }

    #[test]
    fn both_providers_default_llm_unless_specified() {
        let cfg = cfg_from(
            "inbox_dir='/i'\nlibrary_root='/l'\ndatabase_url='sqlite::memory:'\n\
             [ai]\napi_key='k'\n[ai.translate]\nmodel='m'\n\
             [translate]\nprovider='deepl'\n[translate.deepl]\napi_key_env='DEEPL_KEY_TEST_X'\n",
        );
        // The env var is unset, but availability is by config presence, not key.
        let svc = TranslateService::from_config(&cfg).unwrap();
        assert_eq!(svc.default_provider(), TranslateProvider::Deepl);
        let mut p = svc.providers();
        p.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(p, vec![TranslateProvider::Deepl, TranslateProvider::Llm]);
    }
}
