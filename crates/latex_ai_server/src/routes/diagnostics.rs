//! `GET /api/diagnostics`: a short plain-text report to paste into a bug
//! report. It says what the app is running on and how it is set up, never what
//! is in the project, and never a key or the user's name.

use crate::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use latex_ai_core::config::KeySource;
use std::fmt::Write as _;

pub async fn report(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        render(&state),
    )
}

/// The report text. Paths are shortened so the account name does not travel.
fn render(state: &AppState) -> String {
    let snapshot = state.settings.snapshot();
    let ai = &snapshot.config.ai;
    let user_home = latex_ai_core::config::user_home();
    let shorten = |text: &str| match &user_home {
        Some(h) => text.replace(&*h.to_string_lossy(), "~"),
        None => text.to_owned(),
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "Renkeu {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = match crate::bootstrap::check_pdfium() {
        Ok(source) => writeln!(out, "PDF rendering: ok ({})", shorten(&source)),
        Err(e) => writeln!(out, "PDF rendering: unavailable ({e})"),
    };
    let key = match snapshot.key_source {
        KeySource::Env => "from the environment",
        KeySource::File => "stored on this computer",
        KeySource::None => "none",
    };
    let _ = writeln!(
        out,
        "Assistant: {} ({}), vision model {}, timeout {} s, key {key}",
        ai.model,
        match (snapshot.ai_enabled(), snapshot.ai_is_local()) {
            (true, true) => "enabled, local model, no key",
            (true, false) => "enabled",
            (false, _) => "disabled",
        },
        ai.vision_model.as_deref().unwrap_or("none"),
        ai.timeout_secs,
    );
    let _ = writeln!(
        out,
        "TeX bundle: {}",
        bundle_label(snapshot.config.compile.bundle.as_deref())
    );
    let _ = writeln!(
        out,
        "Project folder: {}",
        state
            .workspace_root
            .file_name()
            .map_or_else(|| "(unnamed)".into(), |n| n.to_string_lossy())
    );
    out
}

/// Which bundle is in use, without a query string or a local path.
fn bundle_label(bundle: Option<&str>) -> String {
    match bundle {
        None => "the default".to_owned(),
        Some(b) if b.starts_with("https://") || b.starts_with("http://") => {
            let host = b.split('/').nth(2).unwrap_or("?");
            format!("downloaded from {host}")
        }
        Some(_) => "a file on this computer".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::bundle_label;

    #[test]
    fn a_bundle_is_described_without_its_path_or_query() {
        assert_eq!(bundle_label(None), "the default");
        assert_eq!(
            bundle_label(Some("https://example.org/b/tl2025.ttb?token=secret")),
            "downloaded from example.org"
        );
        assert_eq!(
            bundle_label(Some("/home/ada/bundles/x.ttb")),
            "a file on this computer"
        );
    }
}
