use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

// First-launch bootstrap: app directories, generated default config, the
// `env` key file, and PATH assembly for bundled sidecar binaries.

/// Xuewen's per-user directories on macOS.
pub struct AppDirs {
    /// `~/Library/Application Support/Xuewen` — config, db, library, inbox.
    pub data: PathBuf,
    /// `~/Library/Logs/Xuewen`.
    pub logs: PathBuf,
}

impl AppDirs {
    pub fn resolve() -> Result<Self> {
        let data = dirs::data_dir()
            .context("no user data directory")?
            .join("Xuewen");
        let logs = dirs::home_dir()
            .context("no home directory")?
            .join("Library/Logs/Xuewen");
        Ok(Self { data, logs })
    }

    pub fn config_file(&self) -> PathBuf {
        self.data.join("xuewen.toml")
    }

    pub fn env_file(&self) -> PathBuf {
        self.data.join("env")
    }
}

/// TOML basic-string quoting (escapes `\` and `"`).
fn toml_quote_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// TOML basic-string quoting for a path.
fn toml_quote(p: &Path) -> String {
    toml_quote_str(&p.display().to_string())
}

/// Config for a fresh install: every path absolute under `data`, because a
/// Finder-launched app's working directory is `/`.
pub fn default_config_toml(data: &Path) -> String {
    format!(
        "# Created by the Xuewen desktop app on first launch. Safe to edit; the\n\
         # app never overwrites this file. AI features ([ai.*] sections) are\n\
         # opt-in; put API keys in the `env` file next to this one, one\n\
         # KEY=value per line.\n\
         \n\
         inbox_dir    = {inbox}\n\
         library_root = {library}\n\
         database_url = {db}\n\
         \n\
         [search]\n\
         index_dir = {index}\n\
         \n\
         [preview]\n\
         cache_dir = {preview}\n",
        inbox = toml_quote(&data.join("inbox")),
        library = toml_quote(&data.join("library")),
        db = toml_quote_str(&format!("sqlite:{}", data.join("xuewen.db").display())),
        index = toml_quote(&data.join("search-index")),
        preview = toml_quote(&data.join("preview-cache")),
    )
}

/// Create the app directories and, only when absent, the default config.
/// Returns the config file path.
pub fn ensure_bootstrap(dirs: &AppDirs) -> Result<PathBuf> {
    std::fs::create_dir_all(&dirs.data)?;
    std::fs::create_dir_all(&dirs.logs)?;
    std::fs::create_dir_all(dirs.data.join("inbox"))?;
    std::fs::create_dir_all(dirs.data.join("library"))?;
    let cfg = dirs.config_file();
    if !cfg.exists() {
        std::fs::write(&cfg, default_config_toml(&dirs.data))
            .with_context(|| format!("writing {}", cfg.display()))?;
    }
    Ok(cfg)
}

/// Parse `KEY=value` lines. Blank lines and `#` comments are skipped, as is
/// any line without `=` or with an empty key. One matching pair of single or
/// double quotes around the value is stripped.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let k = k.trim();
            if k.is_empty() {
                return None;
            }
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// `dir` first, so bundled tools shadow any system copies.
pub fn prepend_path(dir: &Path, current: Option<&str>) -> String {
    match current {
        Some(p) if !p.is_empty() => format!("{}:{}", dir.display(), p),
        _ => dir.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_config_load() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Xuewen");
        std::fs::create_dir_all(&data).unwrap();
        let f = data.join("xuewen.toml");
        std::fs::write(&f, default_config_toml(&data)).unwrap();

        let cfg = xuewen::config::Config::load(&f).unwrap();
        assert_eq!(cfg.inbox_dir, data.join("inbox"));
        assert_eq!(cfg.library_root, data.join("library"));
        assert_eq!(
            cfg.database_url,
            format!("sqlite:{}", data.join("xuewen.db").display())
        );
        assert_eq!(cfg.search.index_dir, data.join("search-index"));
        assert_eq!(cfg.preview.cache_dir, data.join("preview-cache"));
    }

    #[test]
    fn ensure_bootstrap_creates_then_never_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = AppDirs {
            data: tmp.path().join("data"),
            logs: tmp.path().join("logs"),
        };
        let cfg_path = ensure_bootstrap(&dirs).unwrap();
        assert!(cfg_path.is_file());
        assert!(dirs.data.join("inbox").is_dir());
        assert!(dirs.data.join("library").is_dir());
        assert!(dirs.logs.is_dir());

        std::fs::write(&cfg_path, "# user edited\n").unwrap();
        let again = ensure_bootstrap(&dirs).unwrap();
        assert_eq!(std::fs::read_to_string(again).unwrap(), "# user edited\n");
    }

    #[test]
    fn parse_env_file_handles_comments_blanks_quotes() {
        let text = "\n# comment\nOPENAI_API_KEY=sk-abc\nQUOTED=\"v a l\"\nSINGLE='x'\nNOVALUE=\nBROKEN_LINE\n = novalue\n";
        let vars = parse_env_file(text);
        assert_eq!(
            vars,
            vec![
                ("OPENAI_API_KEY".into(), "sk-abc".into()),
                ("QUOTED".into(), "v a l".into()),
                ("SINGLE".into(), "x".into()),
                ("NOVALUE".into(), "".into()),
            ]
        );
    }

    #[test]
    fn prepend_path_puts_dir_first() {
        let d = Path::new("/Applications/Xuewen.app/Contents/MacOS");
        assert_eq!(
            prepend_path(d, Some("/usr/bin:/bin")),
            "/Applications/Xuewen.app/Contents/MacOS:/usr/bin:/bin"
        );
        assert_eq!(
            prepend_path(d, None),
            "/Applications/Xuewen.app/Contents/MacOS"
        );
    }
}
