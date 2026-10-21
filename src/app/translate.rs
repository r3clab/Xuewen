use crate::config::Config;

pub(super) const NOT_CONFIGURED: &str =
    "translate is not configured — set [ai.translate] (LLM) or [translate.deepl]";

/// Why `TranslateService::from_config` came back empty: DeepL needs only its
/// section, so only an LLM section can be present and still unusable.
pub(super) fn disabled_reason(cfg: &Config) -> String {
    match cfg.ai.translate.as_ref() {
        None => NOT_CONFIGURED.to_string(),
        Some(use_) => cfg.ai.resolve(use_).unusable_reason("ai.translate"),
    }
}
