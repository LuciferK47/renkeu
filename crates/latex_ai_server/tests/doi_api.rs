//! DOI lookup against a local stand-in for doi.org.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::Router;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::get;
use latex_ai_server::error::ApiError;
use latex_ai_server::routes::doi::lookup_at;

async fn resolver() -> String {
    async fn record(Path(doi): Path<String>) -> (StatusCode, String) {
        // Real resolvers get `Accept: application/x-bibtex`; a stand-in has no need to check.
        match doi.as_str() {
            "10.1000/ok" => (
                StatusCode::OK,
                "@article{Knuth_1984,\n title={Literate},\n}\n".to_owned(),
            ),
            "10.1000/html" => (StatusCode::OK, "<html>not bibtex</html>".to_owned()),
            _ => (StatusCode::NOT_FOUND, String::new()),
        }
    }
    let app = Router::new().route("/{*doi}", get(record));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

#[tokio::test]
async fn a_known_doi_returns_its_bibtex_and_a_pasted_url_works_too() {
    let base = resolver().await;
    let found = lookup_at(&base, "https://doi.org/10.1000/ok")
        .await
        .unwrap();
    assert_eq!(found.doi, "10.1000/ok");
    assert!(found.bibtex.starts_with("@article{Knuth_1984"));
}

#[tokio::test]
async fn unknown_malformed_and_non_bibtex_answers_are_told_apart() {
    let base = resolver().await;
    assert!(matches!(
        lookup_at(&base, "10.1000/missing").await,
        Err(ApiError::NotFound { .. })
    ));
    assert!(matches!(
        lookup_at(&base, "not a doi").await,
        Err(ApiError::BadRequest { .. })
    ));
    assert!(matches!(
        lookup_at(&base, "10.1000/html").await,
        Err(ApiError::Unavailable { .. })
    ));
    // Nothing listening: unreachable is "unavailable", not a crash.
    assert!(matches!(
        lookup_at("http://127.0.0.1:9", "10.1000/ok").await,
        Err(ApiError::Unavailable { .. })
    ));
}
