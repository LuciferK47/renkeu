//! Hot-swappable settings (rust.md I.12 as amended: process config may change
//! at runtime, but every request works from one immutable [`Snapshot`]).
//!
//! A save is persist-then-swap: if writing the file fails, nothing changes. A
//! request that already took a snapshot keeps its model, key and timeout for
//! its whole life even if the user changes settings meanwhile.

use super::{
    ApiKey, CONFIG_FILE, Config, ConfigError, EnvOverrides, KeySource, Overridden, SECRETS_FILE,
    apply_env, config_toml, load, secrets_toml,
};
use crate::ai::{AiError, EditPlanner, LlmPlanner, PlanFuture, PlanRequest, VisualFixRequest};
use arc_swap::ArcSwap;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// The value is pinned by an environment variable, so a UI change would
    /// be silently ignored on the next start.
    #[error("{field} is set by an environment variable and cannot be changed here")]
    Locked { field: &'static str },
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("writing settings: {0}")]
    Io(#[from] std::io::Error),
}

/// A partial update. Absent = unchanged; an empty `vision_model` clears it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatch {
    pub model: Option<String>,
    pub vision_model: Option<String>,
    pub timeout_secs: Option<u64>,
}

/// One consistent, immutable view of the settings.
pub struct Snapshot {
    pub config: Config,
    pub overridden: Overridden,
    pub key_source: KeySource,
    planner: Option<Arc<LlmPlanner>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("config", &self.config)
            .field("overridden", &self.overridden)
            .field("key_source", &self.key_source)
            .field("ai_enabled", &self.ai_enabled())
            .finish_non_exhaustive()
    }
}

impl Snapshot {
    /// AI routes work only with a key.
    #[must_use]
    pub fn ai_enabled(&self) -> bool {
        self.planner.is_some()
    }
}

struct Layers {
    file: Config,
    file_key: Option<ApiKey>,
}

pub struct Settings {
    home: PathBuf,
    env: EnvOverrides,
    env_key: Option<ApiKey>,
    /// Serialises writers; readers never take it.
    layers: tokio::sync::Mutex<Layers>,
    current: ArcSwap<Snapshot>,
}

impl Settings {
    /// Read `<home>/config.toml`, `<home>/secrets.toml` and the environment.
    ///
    /// # Errors
    /// [`ConfigError`] for an unreadable or invalid file or env value —
    /// startup must fail loudly rather than run on a guessed config.
    pub fn open(home: PathBuf, env: EnvOverrides) -> Result<Self, ConfigError> {
        let loaded = load(&home, &env)?;
        let snapshot = build_snapshot(
            &loaded.effective,
            loaded.overridden,
            loaded.env_key.as_ref(),
            loaded.file_key.as_ref(),
        );
        Ok(Self {
            home,
            env,
            env_key: loaded.env_key,
            layers: tokio::sync::Mutex::new(Layers {
                file: loaded.file,
                file_key: loaded.file_key,
            }),
            current: ArcSwap::from_pointee(snapshot),
        })
    }

    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Take the view a request will use from start to finish.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.current.load_full()
    }

    /// Apply `patch`, persist it, and publish the new snapshot.
    ///
    /// # Errors
    /// [`SettingsError::Locked`] for an env-pinned field, `Config` for an
    /// invalid value, `Io` when the file cannot be written (state unchanged).
    pub async fn update(&self, patch: ConfigPatch) -> Result<Arc<Snapshot>, SettingsError> {
        if patch.model.is_some() && self.env.model.is_some() {
            return Err(SettingsError::Locked { field: "model" });
        }
        if patch.vision_model.is_some() && self.env.vision_model.is_some() {
            return Err(SettingsError::Locked {
                field: "vision_model",
            });
        }

        let mut layers = self.layers.lock().await;
        let mut file = layers.file.clone();
        if let Some(m) = patch.model {
            file.ai.model = m;
        }
        if let Some(v) = patch.vision_model {
            file.ai.vision_model = Some(v).filter(|v| !v.trim().is_empty());
        }
        if let Some(t) = patch.timeout_secs {
            file.ai.timeout_secs = t;
        }
        file.validate()?;

        tokio::fs::create_dir_all(&self.home).await?;
        crate::fsutil::atomic_write(&self.home.join(CONFIG_FILE), config_toml(&file)?.as_bytes())
            .await?;
        layers.file = file;
        Ok(self.publish(&layers))
    }

    /// Store a new API key in the `0600` secrets file.
    ///
    /// # Errors
    /// `Locked` when `GROQ_API_KEY` is set; `Config` for a malformed key.
    pub async fn set_api_key(&self, raw: &str) -> Result<Arc<Snapshot>, SettingsError> {
        if self.env_key.is_some() {
            return Err(SettingsError::Locked { field: "api_key" });
        }
        let key = ApiKey::parse(raw)?;
        let mut layers = self.layers.lock().await;
        tokio::fs::create_dir_all(&self.home).await?;
        crate::fsutil::atomic_write_private(
            &self.home.join(SECRETS_FILE),
            secrets_toml(&key)?.as_bytes(),
        )
        .await?;
        layers.file_key = Some(key);
        Ok(self.publish(&layers))
    }

    /// Remove the stored key; AI turns off.
    ///
    /// # Errors
    /// `Locked` when `GROQ_API_KEY` is set; `Io` when the file cannot be removed.
    pub async fn clear_api_key(&self) -> Result<Arc<Snapshot>, SettingsError> {
        if self.env_key.is_some() {
            return Err(SettingsError::Locked { field: "api_key" });
        }
        let mut layers = self.layers.lock().await;
        match tokio::fs::remove_file(self.home.join(SECRETS_FILE)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        layers.file_key = None;
        Ok(self.publish(&layers))
    }

    fn publish(&self, layers: &Layers) -> Arc<Snapshot> {
        let (effective, overridden) = apply_env(&layers.file, &self.env);
        let snapshot = Arc::new(build_snapshot(
            &effective,
            overridden,
            self.env_key.as_ref(),
            layers.file_key.as_ref(),
        ));
        self.current.store(Arc::clone(&snapshot));
        snapshot
    }
}

/// The environment key wins over the stored one.
fn build_snapshot(
    config: &Config,
    overridden: Overridden,
    env_key: Option<&ApiKey>,
    file_key: Option<&ApiKey>,
) -> Snapshot {
    let (key, source) = match (env_key, file_key) {
        (Some(k), _) => (Some(k), KeySource::Env),
        (None, Some(k)) => (Some(k), KeySource::File),
        (None, None) => (None, KeySource::None),
    };
    let planner = key.map(|k| {
        Arc::new(
            LlmPlanner::with_api_key(config.ai.model.clone(), k)
                .with_vision_model(config.ai.vision_model.clone()),
        )
    });
    Snapshot {
        config: config.clone(),
        overridden,
        key_source: source,
        planner,
    }
}

/// The production [`EditPlanner`]: asks [`Settings`] for the current snapshot
/// on every call, so a key or model change takes effect on the next request
/// without a restart and never mid-request.
pub struct ConfiguredPlanner {
    settings: Arc<Settings>,
}

impl ConfiguredPlanner {
    #[must_use]
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
}

async fn within<T>(
    secs: u64,
    fut: impl std::future::Future<Output = Result<T, AiError>>,
) -> Result<T, AiError> {
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .unwrap_or(Err(AiError::Timeout { secs }))
}

impl EditPlanner for ConfiguredPlanner {
    fn is_available(&self) -> bool {
        self.settings.snapshot().ai_enabled()
    }

    fn plan_edits<'a>(&'a self, req: PlanRequest<'a>, cancel: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async move {
            let snap = self.settings.snapshot();
            let planner = snap.planner.as_ref().ok_or(AiError::Disabled)?;
            within(snap.config.ai.timeout_secs, planner.plan_edits(req, cancel)).await
        })
    }

    fn visual_fix<'a>(
        &'a self,
        req: VisualFixRequest<'a>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async move {
            let snap = self.settings.snapshot();
            let planner = snap.planner.as_ref().ok_or(AiError::Disabled)?;
            within(snap.config.ai.timeout_secs, planner.visual_fix(req, cancel)).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(env: EnvOverrides) -> (tempfile::TempDir, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::open(dir.path().to_owned(), env).unwrap();
        (dir, settings)
    }

    #[tokio::test]
    async fn no_key_means_disabled_and_planning_says_so() {
        let (_d, s) = open(EnvOverrides::default());
        assert!(!s.snapshot().ai_enabled());
        let planner = ConfiguredPlanner::new(Arc::new(s));
        assert!(!planner.is_available());
        let req = PlanRequest {
            instruction: "x",
            files: &[],
            other_files: &[],
            feedback: "",
        };
        let err = planner
            .plan_edits(req, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::Disabled), "{err:?}");
    }

    #[tokio::test]
    async fn setting_a_key_enables_ai_without_a_restart_and_clearing_disables_it() {
        let (_d, s) = open(EnvOverrides::default());
        let s = Arc::new(s);
        let planner = ConfiguredPlanner::new(Arc::clone(&s));
        assert!(!planner.is_available());

        let snap = s.set_api_key("gsk_test_key").await.unwrap();
        assert!(snap.ai_enabled());
        assert_eq!(snap.key_source, KeySource::File);
        assert!(planner.is_available());

        let snap = s.clear_api_key().await.unwrap();
        assert!(!snap.ai_enabled());
        assert_eq!(snap.key_source, KeySource::None);
        assert!(!planner.is_available());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secrets_file_is_owner_only_and_the_key_is_not_in_the_config_file() {
        use std::os::unix::fs::PermissionsExt;
        let (d, s) = open(EnvOverrides::default());
        s.set_api_key("gsk_super_secret").await.unwrap();
        s.update(ConfigPatch {
            model: Some("m".into()),
            ..ConfigPatch::default()
        })
        .await
        .unwrap();

        let mode = std::fs::metadata(d.path().join(SECRETS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let config_text = std::fs::read_to_string(d.path().join(CONFIG_FILE)).unwrap();
        assert!(!config_text.contains("gsk_super_secret"));
        // And it survives a restart.
        let reopened = Settings::open(d.path().to_owned(), EnvOverrides::default()).unwrap();
        assert!(reopened.snapshot().ai_enabled());
        assert_eq!(reopened.snapshot().config.ai.model, "m");
    }

    #[tokio::test]
    async fn env_pinned_fields_refuse_ui_changes_and_are_never_persisted() {
        let env = EnvOverrides {
            model: Some("env-model".into()),
            api_key: Some("env-key".into()),
            ..EnvOverrides::default()
        };
        let (d, s) = open(env);
        assert_eq!(s.snapshot().key_source, KeySource::Env);

        let err = s
            .update(ConfigPatch {
                model: Some("other".into()),
                ..ConfigPatch::default()
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, SettingsError::Locked { field: "model" }),
            "{err:?}"
        );
        assert!(matches!(
            s.set_api_key("k").await.unwrap_err(),
            SettingsError::Locked { field: "api_key" }
        ));
        assert!(matches!(
            s.clear_api_key().await.unwrap_err(),
            SettingsError::Locked { field: "api_key" }
        ));

        // A different field still saves, and the env value is not baked in.
        s.update(ConfigPatch {
            timeout_secs: Some(45),
            ..ConfigPatch::default()
        })
        .await
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(CONFIG_FILE)).unwrap();
        assert!(!text.contains("env-model"), "{text}");
        assert_eq!(s.snapshot().config.ai.model, "env-model");
        assert_eq!(s.snapshot().config.ai.timeout_secs, 45);
        assert_eq!(s.snapshot().overridden.names(), vec!["model"]);
    }

    #[tokio::test]
    async fn a_snapshot_taken_before_an_update_keeps_its_values() {
        let (_d, s) = open(EnvOverrides::default());
        let before = s.snapshot();
        s.update(ConfigPatch {
            model: Some("new-model".into()),
            ..ConfigPatch::default()
        })
        .await
        .unwrap();
        assert_eq!(before.config.ai.model, crate::config::DEFAULT_MODEL);
        assert_eq!(s.snapshot().config.ai.model, "new-model");
    }

    #[tokio::test]
    async fn invalid_patch_changes_nothing_on_disk_or_in_memory() {
        let (d, s) = open(EnvOverrides::default());
        let err = s
            .update(ConfigPatch {
                timeout_secs: Some(1),
                ..ConfigPatch::default()
            })
            .await
            .unwrap_err();
        assert!(matches!(err, SettingsError::Config(_)), "{err:?}");
        assert!(!d.path().join(CONFIG_FILE).exists());
        assert_eq!(s.snapshot().config.ai.timeout_secs, 120);
    }

    #[tokio::test]
    async fn empty_vision_model_clears_it() {
        let (_d, s) = open(EnvOverrides::default());
        s.update(ConfigPatch {
            vision_model: Some("vlm".into()),
            ..ConfigPatch::default()
        })
        .await
        .unwrap();
        assert_eq!(s.snapshot().config.ai.vision_model.as_deref(), Some("vlm"));
        s.update(ConfigPatch {
            vision_model: Some(String::new()),
            ..ConfigPatch::default()
        })
        .await
        .unwrap();
        assert_eq!(s.snapshot().config.ai.vision_model, None);
    }
}
