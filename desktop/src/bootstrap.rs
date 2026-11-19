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

const CONFIG_HEADER: &str = "\
# Created by the Xuewen desktop app on first launch. Safe to edit; the app
# never overwrites this file. AI features ([ai.*] sections) are opt-in; put
# API keys in the `env` file next to this one, one KEY=value per line.
";

#[derive(serde::Serialize)]
struct DefaultConfig<'a> {
    data_dir: &'a Path,
}

/// Earlier versions spelled every path out (the database as `xuewen.db`);
/// the app never rewrites a config, so those installs keep the database their
/// file names.
pub fn default_config_toml(data: &Path) -> Result<String> {
    let body = toml::to_string(&DefaultConfig { data_dir: data })?;
    Ok(format!("{CONFIG_HEADER}\n{body}"))
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
        std::fs::write(&cfg, default_config_toml(&dirs.data)?)
            .with_context(|| format!("writing {}", cfg.display()))?;
    }
    Ok(cfg)
}

pub fn load_env_file(path: &Path) -> Result<()> {
    match dotenvy::from_path_override(path) {
        Err(e) if e.not_found() => Ok(()),
        loaded => loaded.with_context(|| format!("env file: {}", path.display())),
    }
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
    fn default_config_puts_everything_under_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Application Support/Xuewen \"quoted\"");
        std::fs::create_dir_all(&data).unwrap();
        let f = data.join("xuewen.toml");
        std::fs::write(&f, default_config_toml(&data).unwrap()).unwrap();

        let cfg = xuewen::config::Config::load(&f).unwrap();
        assert_eq!(cfg.inbox_dir, data.join("inbox"));
        assert_eq!(cfg.library_root, data.join("library"));
        assert_eq!(
            cfg.database_url,
            format!("sqlite:{}", data.join("library.db").display())
        );
        assert_eq!(cfg.search.index_dir, data.join("search-index"));
        assert_eq!(cfg.search.vector_dir, data.join("search-vectors"));
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
    fn load_env_file_reads_shell_style_assignments() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("env");
        std::fs::write(
            &f,
            "\n# comment\nXUEWEN_DESKTOP_TEST_PLAIN=sk-abc\nexport XUEWEN_DESKTOP_TEST_EXPORTED=e\nXUEWEN_DESKTOP_TEST_QUOTED=\"v a l\"\nXUEWEN_DESKTOP_TEST_SINGLE='x'\n",
        )
        .unwrap();

        load_env_file(&f).unwrap();
        for (key, value) in [
            ("XUEWEN_DESKTOP_TEST_PLAIN", "sk-abc"),
            ("XUEWEN_DESKTOP_TEST_EXPORTED", "e"),
            ("XUEWEN_DESKTOP_TEST_QUOTED", "v a l"),
            ("XUEWEN_DESKTOP_TEST_SINGLE", "x"),
        ] {
            assert_eq!(std::env::var(key).unwrap(), value, "{key}");
        }
    }

    #[test]
    fn load_env_file_tolerates_a_missing_file_but_not_a_broken_one() {
        let tmp = tempfile::tempdir().unwrap();
        load_env_file(&tmp.path().join("absent")).unwrap();

        let f = tmp.path().join("env");
        std::fs::write(&f, "XUEWEN_DESKTOP_TEST_BROKEN=\"unterminated\n").unwrap();
        assert!(load_env_file(&f).is_err());
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
