use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub inbox_dir: PathBuf,
    pub library_root: PathBuf,
    pub database_url: String,
    #[serde(default)]
    pub(crate) grobid_url: Option<String>,
    #[serde(default)]
    pub(crate) contact_email: Option<String>,
    #[serde(default)]
    pub(crate) proxy: Option<ProxyConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub preview: PreviewConfig,
    /// Daily arXiv recommendations. Absent ⇒ the feature is off.
    #[serde(default)]
    pub(crate) daily: Option<DailyConfig>,
    /// Translate-on-selection defaults. Like `[search]`/`[ui]`, this section
    /// only sets defaults — the feature is gated by the presence of a
    /// provider (`[ai.translate]` or `[translate.deepl]`), not this section.
    #[serde(default)]
    pub(crate) translate: TranslateConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub(crate) ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProxyConfig {
    /// EZproxy login prefix; a target URL is percent-encoded and appended.
    /// e.g. "https://proxy.uchicago.edu/login?url="
    pub(crate) login_url: String,
}

/// Search settings. Always present: defaults apply when `[search]` is absent.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Tantivy index directory (derived data; safe to delete).
    pub index_dir: PathBuf,
    /// Qdrant Edge vector store directory (derived data; safe to delete while
    /// no xuewen process holds it).
    pub vector_dir: PathBuf,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            index_dir: PathBuf::from("./search-index"),
            vector_dir: PathBuf::from("./search-vectors"),
        }
    }
}

impl SearchConfig {
    fn check_dirs(&self) -> Result<()> {
        let (index, vectors) = (&self.index_dir, &self.vector_dir);
        anyhow::ensure!(
            !index.starts_with(vectors) && !vectors.starts_with(index),
            "[search] index_dir ({}) and vector_dir ({}) must not contain each other: \
             `index rebuild` wipes each of them",
            index.display(),
            vectors.display()
        );
        Ok(())
    }
}

/// Picker preview settings. Always present, like `[search]`: the previews
/// themselves are never optional, only where their renders are kept.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    /// Rendered page PNGs (derived data; safe to delete — pages re-render on
    /// demand). Keyed by content hash, so nothing here can go stale.
    pub cache_dir: PathBuf,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("./preview-cache"),
        }
    }
}

/// Daily arXiv recommendations (`[daily]`).
#[derive(Debug, Clone, Deserialize)]
pub struct DailyConfig {
    /// arXiv category codes, e.g. ["cs.AI", "cs.LG"].
    pub categories: Vec<String>,
    /// Also keep cross-listed announcements.
    #[serde(default)]
    pub include_cross_list: bool,
    /// Ranked papers kept per day.
    #[serde(default = "default_daily_max_papers")]
    pub max_papers: usize,
    /// Daily run time, UTC wall clock "HH:MM".
    #[serde(default = "default_daily_run_at")]
    pub run_at: String,
    /// Batches older than this many days are pruned.
    #[serde(default = "default_daily_retention_days")]
    pub retention_days: u32,
}

fn default_daily_max_papers() -> usize {
    20
}
fn default_daily_run_at() -> String {
    "09:00".to_string()
}
fn default_daily_retention_days() -> u32 {
    14
}

/// Endpoint + model overrides shared by every AI use. `#[serde(flatten)]`-ed
/// into `[ai]` and each use-section so its fields sit at that section's level.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AiDefaults {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
}

/// A use's endpoint resolved against the `[ai]` defaults + built-ins.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub(crate) base_url: String,
    pub(crate) api_key: Option<String>,
    /// The env var `api_key` was resolved through — kept so a missing key can
    /// be reported by name instead of as a silently disabled feature.
    pub(crate) api_key_env: String,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
}

/// All AI/LLM config (`[ai]`): shared defaults plus each use.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    #[serde(flatten)]
    defaults: AiDefaults,
    /// Semantic-search embeddings. Absent ⇒ semantic search off.
    pub(crate) embedding: Option<EmbeddingConfig>,
    /// Per-paper library summaries. Absent ⇒ off.
    pub(crate) summary: Option<AiDefaults>,
    /// Daily-feed summaries. Absent ⇒ off.
    pub(crate) daily: Option<AiDefaults>,
    /// Structured reference parsing for PDF citation popovers. Absent ⇒ off.
    pub(crate) citations: Option<AiDefaults>,
    /// LLM provider for translate-on-selection. Absent ⇒ LLM translate off.
    pub(crate) translate: Option<AiDefaults>,
    /// Agent Ask. No backend subsections ⇒ off.
    pub agent: AgentConfig,
}

impl AiConfig {
    /// Merge a use's overrides over the `[ai]` defaults and built-ins.
    pub(crate) fn resolve(&self, use_: &AiDefaults) -> Resolved {
        let pick = |a: &Option<String>, b: &Option<String>| a.clone().or_else(|| b.clone());
        let base_url =
            pick(&use_.base_url, &self.defaults.base_url).unwrap_or_else(default_ai_base_url);
        let api_key_env =
            pick(&use_.api_key_env, &self.defaults.api_key_env).unwrap_or_else(default_api_key_env);
        let api_key = resolve_key(pick(&use_.api_key, &self.defaults.api_key), &api_key_env);
        Resolved {
            base_url,
            api_key,
            api_key_env,
            model: pick(&use_.model, &self.defaults.model),
            reasoning_effort: pick(&use_.reasoning_effort, &self.defaults.reasoning_effort),
        }
    }
}

/// Inline key wins; else the named env var; empty ⇒ None.
fn resolve_key(inline: Option<String>, api_key_env: &str) -> Option<String> {
    inline
        .or_else(|| std::env::var(api_key_env).ok())
        .filter(|k| !k.trim().is_empty())
}

fn default_ai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

/// Semantic-search embeddings (`[ai.embedding]`).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EmbeddingConfig {
    #[serde(flatten)]
    pub(crate) endpoint: AiDefaults,
    #[serde(default = "default_embed_dims")]
    pub(crate) dims: usize,
}
fn default_embed_dims() -> usize {
    1536
}

impl EmbeddingConfig {
    /// The embedding model: its own override, or the built-in default.
    /// Embeddings never inherit `[ai].model` (a chat model is never a valid
    /// embedding model), which is why this ignores the `[ai]` defaults.
    pub(crate) fn model(&self) -> String {
        self.endpoint
            .model
            .clone()
            .unwrap_or_else(|| "text-embedding-3-small".to_string())
    }
}

/// Agent Ask (`[ai.agent]`): the tool-using agent behind the reader's Ask
/// tab. Off unless at least one backend subsection is present. Auth is the
/// SDKs' own (CLI logins or ANTHROPIC_API_KEY / OPENAI_API_KEY) — never this
/// file. Backends do not inherit `[ai]` defaults: base_url/api_key_env are
/// meaningless to them, only their own `model` applies.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Claude Code backend. Presence enables it.
    pub claude_code: Option<AgentBackendConfig>,
    /// Codex backend. Presence enables it.
    pub codex: Option<AgentBackendConfig>,
    /// Attached-repo clone size guard, in MB.
    pub max_repo_mb: u64,
    /// Per-turn wall-clock timeout.
    pub timeout_secs: u64,
    /// Runner script override (default: agent-runner/src/runner.mjs,
    /// resolved from the server's working directory).
    pub runner: Option<PathBuf>,
    /// Extra hosts (beyond the built-in public forges) permitted as repo-clone
    /// targets — e.g. a self-hosted `git.example.com`. A host matches when it
    /// equals an entry or is a subdomain of it. Guards against clone SSRF.
    pub clone_allowed_hosts: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            claude_code: None,
            codex: None,
            max_repo_mb: 500,
            timeout_secs: 300,
            runner: None,
            clone_allowed_hosts: Vec::new(),
        }
    }
}

impl AgentConfig {
    #[cfg(test)]
    fn enabled(&self) -> bool {
        self.claude_code.is_some() || self.codex.is_some()
    }
}

/// One agent backend (`[ai.agent.claude_code]` / `[ai.agent.codex]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AgentBackendConfig {
    /// Optional model override passed through to the SDK.
    pub model: Option<String>,
}

/// UI preferences (`[ui]`), surfaced to the frontend via `/api/settings`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct UiConfig {
    /// Whether the abstract disclosure starts collapsed in the Details panel.
    pub(crate) fold_abstract: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            fold_abstract: true,
        }
    }
}

/// `[translate]` — translate-on-selection. Feature is enabled by the presence
/// of a provider (`[ai.translate]` or `[translate.deepl]`), not this section;
/// this section only sets defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct TranslateConfig {
    /// Target language the selection is translated INTO. Default Simplified Chinese.
    pub(crate) target_lang: String,
    /// Default provider. `None` ⇒ resolved to the single available one, or Llm.
    pub(crate) provider: Option<TranslateProvider>,
    /// Seed for the client's Auto/Manual toggle default.
    pub(crate) trigger: TranslateTrigger,
    /// DeepL provider config. Present ⇒ DeepL available.
    pub(crate) deepl: Option<DeeplConfig>,
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            target_lang: default_target_lang(),
            provider: None,
            trigger: TranslateTrigger::default(),
            deepl: None,
        }
    }
}

fn default_target_lang() -> String {
    "zh".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub(crate) enum TranslateProvider {
    Llm,
    Deepl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TranslateTrigger {
    #[default]
    Auto,
    Manual,
}

/// DeepL provider config (`[translate.deepl]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DeeplConfig {
    pub(crate) api_key_env: Option<String>,
    pub(crate) plan: DeeplPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DeeplPlan {
    #[default]
    Free,
    Pro,
}

impl DeeplPlan {
    /// DeepL API base URL for this plan.
    pub(crate) fn base_url(self) -> &'static str {
        match self {
            DeeplPlan::Free => "https://api-free.deepl.com",
            DeeplPlan::Pro => "https://api.deepl.com",
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        cfg.inbox_dir = expand_tilde(cfg.inbox_dir, home.clone());
        cfg.library_root = expand_tilde(cfg.library_root, home.clone());
        cfg.search.index_dir = expand_tilde(cfg.search.index_dir, home.clone());
        cfg.search.vector_dir = expand_tilde(cfg.search.vector_dir, home.clone());
        cfg.preview.cache_dir = expand_tilde(cfg.preview.cache_dir, home.clone());
        cfg.ai.agent.runner = cfg.ai.agent.runner.map(|p| expand_tilde(p, home));
        cfg.search.check_dirs()?;
        Ok(cfg)
    }
}

/// Expand a leading `~/` (or bare `~`) using `home`; otherwise return as-is.
fn expand_tilde(p: PathBuf, home: Option<PathBuf>) -> PathBuf {
    match (p.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_minimal_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();

        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.inbox_dir, PathBuf::from("/data/inbox"));
        assert_eq!(cfg.library_root, PathBuf::from("/data/library"));
        assert_eq!(cfg.database_url, "sqlite:/data/library.db");
        assert_eq!(cfg.grobid_url, None);
    }

    #[test]
    fn expands_leading_tilde_with_home() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            expand_tilde(PathBuf::from("~/papers/inbox"), home.clone()),
            PathBuf::from("/home/u/papers/inbox")
        );
        // No tilde, or no HOME: unchanged.
        assert_eq!(
            expand_tilde(PathBuf::from("/data/inbox"), home),
            PathBuf::from("/data/inbox")
        );
        assert_eq!(
            expand_tilde(PathBuf::from("~/x"), None),
            PathBuf::from("~/x")
        );
    }

    #[test]
    fn load_error_names_the_file() {
        let err = Config::load(Path::new("/nope/xuewen.toml")).unwrap_err();
        assert!(err.to_string().contains("/nope/xuewen.toml"));
    }

    #[test]
    fn loads_proxy_section() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[proxy]
login_url = "https://proxy.uchicago.edu/login?url="
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(
            cfg.proxy.unwrap().login_url,
            "https://proxy.uchicago.edu/login?url="
        );
    }

    #[test]
    fn proxy_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().proxy.is_none());
    }

    #[test]
    fn search_defaults_when_section_absent() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.search.index_dir, PathBuf::from("./search-index"));
        assert_eq!(cfg.preview.cache_dir, PathBuf::from("./preview-cache"));
        assert_eq!(cfg.search.vector_dir, PathBuf::from("./search-vectors"));
        assert!(cfg.ai.embedding.is_none());
    }

    #[test]
    fn legacy_qdrant_keys_are_ignored() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
qdrant_url = "http://localhost:6333"
qdrant_collection = "xuewen"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.search.vector_dir, PathBuf::from("./search-vectors"));
    }

    #[test]
    fn nested_index_and_vector_dirs_are_rejected() {
        for (index, vectors) in [("/d/idx", "/d/idx/vec"), ("/d/vec/idx", "/d/vec")] {
            let mut f = tempfile::NamedTempFile::new().unwrap();
            write!(
                f,
                r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
index_dir = "{index}"
vector_dir = "{vectors}"
"#
            )
            .unwrap();
            let err = Config::load(f.path()).unwrap_err();
            assert!(err.to_string().contains("vector_dir"), "got: {err}");
        }
    }

    #[test]
    fn loads_ai_embedding_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
index_dir = "~/idx"
vector_dir = "~/vec"

[ai.embedding]
api_key = "sk-test"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        // tilde expanded like inbox_dir/library_root
        assert!(!cfg.search.index_dir.starts_with("~"));
        assert!(!cfg.search.vector_dir.starts_with("~"));
        let e = cfg.ai.embedding.as_ref().unwrap();
        assert_eq!(e.dims, 1536);
        assert_eq!(e.endpoint.api_key.as_deref(), Some("sk-test"));
        // embedding has no model of its own here; base_url/api_key resolve
        // through [ai] defaults + built-ins.
        let r = cfg.ai.resolve(&e.endpoint);
        assert_eq!(r.base_url, "https://api.openai.com/v1");
        assert_eq!(r.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn daily_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().daily.is_none());
    }

    #[test]
    fn loads_daily_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[daily]
categories = ["cs.AI", "cs.LG"]
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let d = cfg.daily.unwrap();
        assert_eq!(d.categories, vec!["cs.AI", "cs.LG"]);
        assert!(!d.include_cross_list);
        assert_eq!(d.max_papers, 20);
        assert_eq!(d.run_at, "09:00");
        assert_eq!(d.retention_days, 14);
        // The daily summarizer LLM now lives under [ai.daily], separately.
        assert!(cfg.ai.daily.is_none());
    }

    #[test]
    fn resolve_key_prefers_inline_over_env_and_empty_is_none() {
        assert_eq!(
            resolve_key(Some("sk-inline".into()), "XUEWEN_TEST_UNSET_ENV"),
            Some("sk-inline".into())
        );
        // Env var unset -> keyless (requests carry no Authorization).
        assert_eq!(resolve_key(None, "XUEWEN_TEST_UNSET_ENV"), None);
    }

    #[test]
    fn loads_ai_summary_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"

[ai.summary]
model = "gpt-4o-mini"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let s = cfg.ai.summary.as_ref().expect("summary section present");
        assert_eq!(s.model.as_deref(), Some("gpt-4o-mini"));
        let r = cfg.ai.resolve(s);
        assert_eq!(r.base_url, "https://api.openai.com/v1");
        assert_eq!(r.model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn ui_fold_abstract_defaults_true_and_summary_absent() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert!(cfg.ai.summary.is_none());
        assert!(cfg.ui.fold_abstract, "fold_abstract defaults to true");
    }

    #[test]
    fn ai_resolve_precedence_use_over_ai_over_builtin() {
        use super::{AiConfig, AiDefaults};
        let ai = AiConfig {
            defaults: AiDefaults {
                base_url: Some("https://ai.example/v1".into()),
                model: Some("gpt-4o-mini".into()),
                reasoning_effort: Some("high".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // Empty use inherits [ai] defaults.
        let r = ai.resolve(&AiDefaults::default());
        assert_eq!(r.base_url, "https://ai.example/v1");
        assert_eq!(r.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(r.reasoning_effort.as_deref(), Some("high"));
        // Use override wins.
        let r2 = ai.resolve(&AiDefaults {
            model: Some("gpt-5.6-terra".into()),
            ..Default::default()
        });
        assert_eq!(r2.model.as_deref(), Some("gpt-5.6-terra"));
        // Built-in base_url when [ai] omits it.
        let bare = AiConfig::default();
        assert_eq!(
            bare.resolve(&AiDefaults::default()).base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn ai_sections_parse_with_flatten_and_inheritance() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"

[ai]
model = "gpt-4o-mini"
reasoning_effort = "high"

[ai.embedding]
model = "text-embedding-3-small"
dims = 1536

[ai.summary]

[ai.daily]
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let ai = &cfg.ai;
        assert_eq!(ai.defaults.model.as_deref(), Some("gpt-4o-mini"));
        // embedding overrides model, keeps its own model
        assert_eq!(
            ai.embedding.as_ref().unwrap().endpoint.model.as_deref(),
            Some("text-embedding-3-small")
        );
        assert_eq!(ai.embedding.as_ref().unwrap().dims, 1536);
        // present-but-empty summary/daily ⇒ Some(defaults), inherit via resolve
        assert!(ai.summary.is_some());
        assert_eq!(
            ai.resolve(ai.summary.as_ref().unwrap()).model.as_deref(),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            ai.resolve(ai.summary.as_ref().unwrap())
                .reasoning_effort
                .as_deref(),
            Some("high")
        );
        assert!(ai.daily.is_some());
    }

    #[test]
    fn resolved_carries_the_consulted_api_key_env_name() {
        // The name rides on `Resolved` so a missing key can be reported as
        // "set $THAT_VAR" instead of a silently disabled feature.
        let ai = AiConfig::default();
        assert_eq!(
            ai.resolve(&AiDefaults::default()).api_key_env,
            "OPENAI_API_KEY"
        );
        let use_ = AiDefaults {
            api_key_env: Some("MY_PROVIDER_KEY".into()),
            ..Default::default()
        };
        assert_eq!(ai.resolve(&use_).api_key_env, "MY_PROVIDER_KEY");
    }

    #[test]
    fn ai_absent_means_features_off() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "inbox_dir=\"/d/i\"\nlibrary_root=\"/d/l\"\ndatabase_url=\"sqlite:/d/x.db\"\n"
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert!(cfg.ai.embedding.is_none());
        assert!(cfg.ai.summary.is_none());
        assert!(cfg.ai.daily.is_none());
        assert!(cfg.ai.citations.is_none());
    }

    #[test]
    fn embedding_model_uses_own_or_builtin_never_ai_default() {
        use super::{AiDefaults, EmbeddingConfig};
        let e = EmbeddingConfig {
            endpoint: AiDefaults::default(),
            dims: 1536,
        };
        assert_eq!(e.model(), "text-embedding-3-small"); // no inherit, built-in
        let e2 = EmbeddingConfig {
            endpoint: AiDefaults {
                model: Some("my-embed".into()),
                ..Default::default()
            },
            dims: 1536,
        };
        assert_eq!(e2.model(), "my-embed"); // own override
    }

    #[test]
    fn ai_resolve_inherits_shared_inline_api_key() {
        use super::{AiConfig, AiDefaults};
        let ai = AiConfig {
            defaults: AiDefaults {
                api_key: Some("sk-shared".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // A use with no key of its own inherits the [ai] inline key.
        assert_eq!(
            ai.resolve(&AiDefaults::default()).api_key.as_deref(),
            Some("sk-shared")
        );
        // A use's own inline key wins.
        let own = AiDefaults {
            api_key: Some("sk-own".into()),
            ..Default::default()
        };
        assert_eq!(ai.resolve(&own).api_key.as_deref(), Some("sk-own"));
    }

    #[test]
    fn ai_citations_section_parses_and_is_off_by_default() {
        let with: crate::config::AiConfig =
            toml::from_str("model = \"gpt-4o-mini\"\n[citations]\nmodel = \"gpt-5-mini\"").unwrap();
        let use_ = with.citations.as_ref().expect("[ai.citations] present");
        assert_eq!(with.resolve(use_).model.as_deref(), Some("gpt-5-mini"));

        let without: crate::config::AiConfig = toml::from_str("model = \"gpt-4o-mini\"").unwrap();
        assert!(without.citations.is_none());
    }

    #[test]
    fn translate_defaults_when_section_present_but_minimal() {
        let toml = r#"
inbox_dir = "/i"
library_root = "/l"
database_url = "sqlite::memory:"

[translate]

[ai.translate]
model = "gpt-4o-mini"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let t = &cfg.translate;
        assert_eq!(t.target_lang, "zh");
        assert!(matches!(t.trigger, TranslateTrigger::Auto));
        assert!(t.provider.is_none()); // unset -> resolved later by the service
        assert!(cfg.ai.translate.is_some());
    }

    #[test]
    fn translate_parses_deepl_and_explicit_provider() {
        let toml = r#"
inbox_dir = "/i"
library_root = "/l"
database_url = "sqlite::memory:"

[translate]
target_lang = "en"
provider = "deepl"
trigger = "manual"

[translate.deepl]
api_key_env = "DEEPL_API_KEY"
plan = "pro"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let t = &cfg.translate;
        assert_eq!(t.target_lang, "en");
        assert!(matches!(t.provider, Some(TranslateProvider::Deepl)));
        assert!(matches!(t.trigger, TranslateTrigger::Manual));
        let d = t.deepl.as_ref().unwrap();
        assert_eq!(d.api_key_env.as_deref(), Some("DEEPL_API_KEY"));
        assert!(matches!(d.plan, DeeplPlan::Pro));
    }

    #[test]
    fn translate_defaults_when_section_absent() {
        let toml = r#"
inbox_dir = "/i"
library_root = "/l"
database_url = "sqlite::memory:"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        // A defaults-only section (like [search]/[ui]): absence means default
        // values, and no DeepL provider — the feature gate lives with the
        // providers, not here.
        assert_eq!(cfg.translate.target_lang, "zh");
        assert!(cfg.translate.provider.is_none());
        assert!(cfg.translate.deepl.is_none());
    }

    #[test]
    fn ai_agent_absent_means_disabled_with_defaults() {
        let ai: crate::config::AiConfig = toml::from_str("").unwrap();
        assert!(!ai.agent.enabled());
        assert_eq!(ai.agent.max_repo_mb, 500);
        assert_eq!(ai.agent.timeout_secs, 300);
        assert!(ai.agent.runner.is_none());
    }

    #[test]
    fn ai_agent_backends_toggle_independently() {
        let ai: crate::config::AiConfig = toml::from_str(
            "[agent]\nmax_repo_mb = 100\n[agent.claude_code]\nmodel = \"claude-sonnet-5\"\n",
        )
        .unwrap();
        assert!(ai.agent.enabled());
        assert_eq!(ai.agent.max_repo_mb, 100);
        assert_eq!(
            ai.agent.claude_code.as_ref().unwrap().model.as_deref(),
            Some("claude-sonnet-5")
        );
        assert!(ai.agent.codex.is_none());

        let both: crate::config::AiConfig =
            toml::from_str("[agent.claude_code]\n[agent.codex]\n").unwrap();
        assert!(both.agent.claude_code.is_some());
        assert!(both.agent.codex.is_some());
        assert!(both.agent.codex.as_ref().unwrap().model.is_none());
    }
}
