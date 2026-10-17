//! Process configuration (masterplan §15.1, rust.md I.12).
//!
//! Three layers, later wins: built-in defaults < `<home>/config.toml` <
//! environment. The API key is a secret and lives apart, in a `0600`
//! `<home>/secrets.toml` (or the `GROQ_API_KEY` variable), so the readable
//! config file can be shared or committed without leaking it.
//!
//! Unknown keys are a startup error (`deny_unknown_fields`): a typo such as
//! `modle = …` must not silently fall back to the default.

mod runtime;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub use runtime::{ConfigPatch, ConfiguredPlanner, Settings, SettingsError, Snapshot};

/// File name of the user-editable config inside the home directory.
pub const CONFIG_FILE: &str = "config.toml";
/// File name of the secrets file inside the home directory.
pub const SECRETS_FILE: &str = "secrets.toml";

/// Model genai 0.1.23 recognises; unknown names fall back to Ollama.
pub const DEFAULT_MODEL: &str = "llama-3.3-70b-versatile";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid value for {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
}

/// Everything the user may tune. Add a field only together with the code that
/// reads it (rust.md I.15).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub ai: AiConfig,
    pub compile: CompileConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CompileConfig {
    /// Wall-clock budget for one compile, in seconds. A document still
    /// running after this is killed (an infinite macro loop, a huge `TikZ`
    /// picture). The first compile after install also downloads the TeX
    /// bundle, so keep this generous.
    pub timeout_secs: u64,
}

impl Default for CompileConfig {
    fn default() -> Self {
        Self { timeout_secs: 60 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AiConfig {
    /// Text planner model.
    pub model: String,
    /// Optional vision model for visual fixes. Absent = text-only fallback.
    pub vision_model: Option<String>,
    /// Upper bound on one LLM call, in seconds.
    pub timeout_secs: u64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_owned(),
            vision_model: None,
            timeout_secs: 120,
        }
    }
}

impl Config {
    /// Reject values the rest of the system cannot work with.
    ///
    /// # Errors
    /// [`ConfigError::Invalid`] naming the offending field.
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_model("ai.model", &self.ai.model)?;
        if let Some(v) = &self.ai.vision_model {
            validate_model("ai.vision_model", v)?;
        }
        if !(5..=600).contains(&self.ai.timeout_secs) {
            return Err(ConfigError::Invalid {
                field: "ai.timeout_secs",
                reason: "must be between 5 and 600".to_owned(),
            });
        }
        if !(5..=600).contains(&self.compile.timeout_secs) {
            return Err(ConfigError::Invalid {
                field: "compile.timeout_secs",
                reason: "must be between 5 and 600".to_owned(),
            });
        }
        Ok(())
    }
}

fn validate_model(field: &'static str, value: &str) -> Result<(), ConfigError> {
    let ok = !value.is_empty()
        && value.len() <= 200
        && value.chars().all(|c| !c.is_whitespace() && !c.is_control());
    if ok {
        Ok(())
    } else {
        Err(ConfigError::Invalid {
            field,
            reason: "must be 1-200 characters with no whitespace".to_owned(),
        })
    }
}

/// An API key. `Debug` is redacted so it cannot reach a log through a stray
/// `{:?}` or `#[instrument]`; read it only with [`ApiKey::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// # Errors
    /// [`ConfigError::Invalid`] when the key is empty, over-long or contains
    /// whitespace/control characters (a pasted newline is the usual culprit).
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        let key = raw.trim();
        let ok = !key.is_empty()
            && key.len() <= 512
            && key.chars().all(|c| !c.is_whitespace() && !c.is_control());
        if ok {
            Ok(Self(key.to_owned()))
        } else {
            Err(ConfigError::Invalid {
                field: "api_key",
                reason: "must be 1-512 characters with no whitespace".to_owned(),
            })
        }
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Where the active key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// `GROQ_API_KEY` is set; it cannot be changed from the UI.
    Env,
    /// Stored in `secrets.toml`.
    File,
    None,
}

/// The environment variables that override the file. Captured once so tests
/// need not mutate the process environment.
#[derive(Debug, Clone, Default)]
pub struct EnvOverrides {
    pub model: Option<String>,
    pub vision_model: Option<String>,
    pub api_key: Option<String>,
}

impl EnvOverrides {
    #[must_use]
    pub fn from_process() -> Self {
        let get = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            model: get("LATEX_AI_MODEL"),
            vision_model: get("LATEX_AI_VISION_MODEL"),
            api_key: get("GROQ_API_KEY"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overridden {
    pub model: bool,
    pub vision_model: bool,
}

impl Overridden {
    /// Field names, as the API reports them.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.model {
            out.push("model");
        }
        if self.vision_model {
            out.push("vision_model");
        }
        out
    }
}

/// Result of reading all layers.
#[derive(Debug)]
pub(crate) struct Loaded {
    /// The file layer alone (defaults + `config.toml`): what a save persists,
    /// so env overrides are never baked into the file.
    pub file: Config,
    pub effective: Config,
    pub overridden: Overridden,
    /// The key from `secrets.toml`, whether or not an env key shadows it.
    pub file_key: Option<ApiKey>,
    pub env_key: Option<ApiKey>,
}

pub(crate) fn load(home: &Path, env: &EnvOverrides) -> Result<Loaded, ConfigError> {
    let file = read_toml::<Config>(&home.join(CONFIG_FILE))?.unwrap_or_default();
    file.validate()?;

    let (effective, overridden) = apply_env(&file, env);
    effective.validate()?;

    let env_key = env.api_key.as_deref().map(ApiKey::parse).transpose()?;
    let file_key = read_secrets(home)?;

    Ok(Loaded {
        file,
        effective,
        overridden,
        file_key,
        env_key,
    })
}

/// Layer the environment over the file config.
pub(crate) fn apply_env(file: &Config, env: &EnvOverrides) -> (Config, Overridden) {
    let mut effective = file.clone();
    let mut overridden = Overridden::default();
    if let Some(m) = &env.model {
        effective.ai.model.clone_from(m);
        overridden.model = true;
    }
    if let Some(v) = &env.vision_model {
        effective.ai.vision_model = Some(v.clone());
        overridden.vision_model = true;
    }
    (effective, overridden)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretsFile {
    groq_api_key: String,
}

fn read_secrets(home: &Path) -> Result<Option<ApiKey>, ConfigError> {
    let path = home.join(SECRETS_FILE);
    warn_if_group_or_world_readable(&path);
    match read_toml::<SecretsFile>(&path)? {
        Some(s) => Ok(Some(ApiKey::parse(&s.groq_api_key)?)),
        None => Ok(None),
    }
}

pub(crate) fn secrets_toml(key: &ApiKey) -> Result<String, ConfigError> {
    toml::to_string(&SecretsFile {
        groq_api_key: key.expose().to_owned(),
    })
    .map_err(|e| ConfigError::Invalid {
        field: "api_key",
        reason: e.to_string(),
    })
}

pub(crate) fn config_toml(config: &Config) -> Result<String, ConfigError> {
    toml::to_string_pretty(config).map_err(|e| ConfigError::Invalid {
        field: "config",
        reason: e.to_string(),
    })
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|e| ConfigError::Parse {
            path: path.to_owned(),
            message: e.to_string(),
        })
}

#[cfg(unix)]
fn warn_if_group_or_world_readable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.permissions().mode() & 0o077 != 0 {
            tracing::warn!(
                path = %path.display(),
                "secrets file is accessible to other users; run `chmod 600` on it"
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_group_or_world_readable(_path: &Path) {}

/// The data directory: `$LATEX_AI_HOME`, else `$HOME/.latex-ai`, else a
/// temp-dir fallback.
#[must_use]
pub fn default_home() -> PathBuf {
    std::env::var_os("LATEX_AI_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".latex-ai")))
        .unwrap_or_else(|| std::env::temp_dir().join(".latex-ai"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with(config: Option<&str>, secrets: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(c) = config {
            std::fs::write(dir.path().join(CONFIG_FILE), c).unwrap();
        }
        if let Some(s) = secrets {
            std::fs::write(dir.path().join(SECRETS_FILE), s).unwrap();
        }
        dir
    }

    #[test]
    fn defaults_apply_when_nothing_is_configured() {
        let dir = home_with(None, None);
        let l = load(dir.path(), &EnvOverrides::default()).unwrap();
        assert_eq!(l.effective, Config::default());
        assert!(l.file_key.is_none() && l.env_key.is_none());
    }

    #[test]
    fn file_beats_defaults_and_env_beats_file() {
        let dir = home_with(
            Some("[ai]\nmodel = \"from-file\"\ntimeout_secs = 30\n"),
            None,
        );

        let l = load(dir.path(), &EnvOverrides::default()).unwrap();
        assert_eq!(l.effective.ai.model, "from-file");
        assert_eq!(l.effective.ai.timeout_secs, 30);
        assert!(!l.overridden.model);

        let env = EnvOverrides {
            model: Some("from-env".into()),
            ..EnvOverrides::default()
        };
        let l = load(dir.path(), &env).unwrap();
        assert_eq!(l.effective.ai.model, "from-env");
        assert!(l.overridden.model);
        // The file layer is untouched, so saving cannot bake the env in.
        assert_eq!(l.file.ai.model, "from-file");
    }

    #[test]
    fn unknown_keys_are_a_startup_error_that_names_the_file() {
        let dir = home_with(Some("[ai]\nmodle = \"typo\"\n"), None);
        let err = load(dir.path(), &EnvOverrides::default()).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("config.toml"), "{text}");
        assert!(text.contains("modle"), "{text}");

        let dir = home_with(Some("[surprise]\nx = 1\n"), None);
        assert!(load(dir.path(), &EnvOverrides::default()).is_err());
    }

    #[test]
    fn invalid_values_are_rejected() {
        let dir = home_with(Some("[ai]\ntimeout_secs = 0\n"), None);
        assert!(matches!(
            load(dir.path(), &EnvOverrides::default()),
            Err(ConfigError::Invalid {
                field: "ai.timeout_secs",
                ..
            })
        ));
        let dir = home_with(Some("[ai]\nmodel = \"has space\"\n"), None);
        assert!(load(dir.path(), &EnvOverrides::default()).is_err());
    }

    #[test]
    fn key_comes_from_env_first_then_file() {
        let dir = home_with(None, Some("groq_api_key = \"file-key\"\n"));
        let l = load(dir.path(), &EnvOverrides::default()).unwrap();
        assert!(l.env_key.is_none());
        assert_eq!(l.file_key.unwrap().expose(), "file-key");

        let env = EnvOverrides {
            api_key: Some("env-key".into()),
            ..EnvOverrides::default()
        };
        let l = load(dir.path(), &env).unwrap();
        assert_eq!(l.env_key.unwrap().expose(), "env-key");
    }

    #[test]
    fn api_key_debug_is_redacted_and_input_is_validated() {
        let key = ApiKey::parse("  gsk_secret  ").unwrap();
        assert_eq!(key.expose(), "gsk_secret");
        let shown = format!("{key:?}");
        assert!(!shown.contains("gsk_secret"), "{shown}");
        assert!(ApiKey::parse("").is_err());
        assert!(ApiKey::parse("two words").is_err());
        assert!(ApiKey::parse("line\nbreak").is_err());
    }

    #[test]
    fn example_config_file_matches_the_defaults() {
        // config.example.toml is documentation users copy; it must parse and
        // must say what the program actually does (rust.md golden file).
        let example = include_str!("../../../../config.example.toml");
        let parsed: Config = toml::from_str(example).unwrap();
        assert_eq!(parsed, Config::default());
    }
}
