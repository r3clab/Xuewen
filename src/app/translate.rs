use crate::config::Config;

pub(super) const NOT_CONFIGURED: &str =
    "translate is not configured — set [ai.translate] (LLM) or [translate.deepl]";

/// Why `TranslateService::from_config` came back empty: every configured
/// provider's reason, or that none is configured.
pub(super) fn disabled_reason(cfg: &Config) -> String {
    let llm = cfg
        .ai
        .translate
        .as_ref()
        .map(|use_| cfg.ai.resolve(use_).unusable_reason("ai.translate"));
    let deepl = cfg
        .translate
        .deepl
        .as_ref()
        .and_then(|d| d.resolve_key().err());
    let reasons: Vec<String> = llm.into_iter().chain(deepl).collect();
    if reasons.is_empty() {
        NOT_CONFIGURED.to_string()
    } else {
        reasons.join("; ")
    }
}
