//! Submission checks: `GET /api/preflight/profiles` lists the profiles,
//! `GET /api/preflight?build=&profile=` applies one to the project and to a
//! finished build. Nothing is changed in the project.

use super::visual::{build_is_current, resolve_build};
use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::instrument;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightQuery {
    pub build: String,
    /// A profile id; paths are for the command line, not for a browser.
    pub profile: String,
}

#[derive(Debug, Serialize)]
pub struct PreflightResponse {
    pub build: String,
    /// The project has changed since this build was compiled.
    pub stale: bool,
    #[serde(flatten)]
    pub report: preflight::Report,
}

/// `GET /api/preflight/profiles`
pub async fn profiles(State(state): State<AppState>) -> Json<Value> {
    let list: Vec<Value> = crate::profiles::all(Some(state.settings.home()))
        .into_iter()
        .map(|p| {
            let rules: Vec<Value> = p
                .rules
                .iter()
                .map(|r| json!({ "check": r.check.id(), "severity": r.severity }))
                .collect();
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "source": p.source,
                "rules": rules,
            })
        })
        .collect();
    Json(json!({ "profiles": list }))
}

/// `GET /api/preflight?build=&profile=`
#[instrument(skip(state))]
pub async fn run(
    State(state): State<AppState>,
    Query(q): Query<PreflightQuery>,
) -> Result<Json<PreflightResponse>, ApiError> {
    // An id only: a path here would let a page read profile files from anywhere.
    let profile = crate::profiles::all(Some(state.settings.home()))
        .into_iter()
        .find(|p| p.id == q.profile)
        .ok_or_else(|| ApiError::NotFound {
            message: format!("no profile `{}`", q.profile),
        })?;
    let build = resolve_build(&state, Some(&q.build), None).await?;
    let outdated = !build_is_current(&state, &build).await?;

    let (root, main) = (state.workspace_root.clone(), build.manifest.main.clone());
    let project = tokio::task::spawn_blocking(move || preflight::Project::read(&root, &main))
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .map_err(|e| ApiError::Internal {
            message: format!("reading the project: {e}"),
        })?;
    let facts = crate::check::facts_of(&build).await;
    let report = tokio::task::spawn_blocking(move || preflight::run(&profile, &project, &facts))
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;

    Ok(Json(PreflightResponse {
        build: build.id().to_owned(),
        stale: outdated,
        report,
    }))
}
