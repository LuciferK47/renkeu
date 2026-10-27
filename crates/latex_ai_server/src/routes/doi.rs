//! Look up a citation by DOI. The only request this app makes for the user's
//! benefit rather than the AI's: it sends the DOI, and nothing else, to
//! doi.org, which answers with BibTeX (content negotiation, `application/x-bibtex`).

use crate::error::ApiError;
use axum::Json;
use axum::extract::Query;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::instrument;

const MAX_DOI_BYTES: usize = 200;
const MAX_BODY_BYTES: usize = 200 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Overrides the resolver, so tests can use a local stand-in.
const BASE_ENV: &str = "LATEX_AI_DOI_BASE";
const DEFAULT_BASE: &str = "https://doi.org";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoiQuery {
    pub doi: String,
}

#[derive(Serialize)]
pub struct Bibtex {
    pub doi: String,
    pub bibtex: String,
}

/// A DOI as people paste it: `10.1000/xyz`, or with a `doi:` prefix or a
/// `https://doi.org/` URL. Returns the bare DOI, or `None` if it is not one.
#[must_use]
pub fn clean_doi(input: &str) -> Option<String> {
    let t = input.trim();
    let t = t
        .strip_prefix("https://doi.org/")
        .or_else(|| t.strip_prefix("http://doi.org/"))
        .or_else(|| t.strip_prefix("https://dx.doi.org/"))
        .or_else(|| t.strip_prefix("doi:"))
        .unwrap_or(t)
        .trim();
    let (prefix, suffix) = t.split_once('/')?;
    let registrant = prefix.strip_prefix("10.")?;
    let ok_prefix = (4..=9).contains(&registrant.split('.').next()?.len())
        && registrant.chars().all(|c| c.is_ascii_digit() || c == '.');
    let ok_suffix = !suffix.is_empty()
        && suffix
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | '"' | '\\' | '?' | '#'));
    // `..` would let a URL parser walk out of the DOI's path.
    (ok_prefix && ok_suffix && !suffix.contains("..") && t.len() <= MAX_DOI_BYTES)
        .then(|| t.to_owned())
}

/// `GET /api/doi?doi=`
#[instrument]
pub async fn lookup(Query(q): Query<DoiQuery>) -> Result<Json<Bibtex>, ApiError> {
    let base = std::env::var(BASE_ENV).unwrap_or_else(|_| DEFAULT_BASE.to_owned());
    lookup_at(&base, &q.doi).await.map(Json)
}

/// [`lookup`] against a given resolver, so a test can use a local one.
///
/// # Errors
/// 400 for a malformed DOI, 404 when the resolver has no record, 503 when it
/// cannot be reached or does not answer with BibTeX.
pub async fn lookup_at(base: &str, input: &str) -> Result<Bibtex, ApiError> {
    let doi = clean_doi(input).ok_or_else(|| ApiError::BadRequest {
        message: "that does not look like a DOI (for example 10.1145/3292500.3330701)".to_owned(),
    })?;
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("Renkeu/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?;
    let res = client
        .get(format!("{}/{doi}", base.trim_end_matches('/')))
        .header("Accept", "application/x-bibtex")
        .send()
        .await
        .map_err(|e| ApiError::Unavailable {
            message: format!("could not reach the DOI service: {e}"),
        })?;
    match res.status().as_u16() {
        200 => {}
        404 => {
            return Err(ApiError::NotFound {
                message: format!("no record found for {doi}"),
            });
        }
        s => {
            return Err(ApiError::Unavailable {
                message: format!("the DOI service answered {s}"),
            });
        }
    }
    let bytes = res.bytes().await.map_err(|e| ApiError::Unavailable {
        message: format!("reading the answer: {e}"),
    })?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err(ApiError::Unavailable {
            message: "the DOI service sent an unexpectedly large answer".to_owned(),
        });
    }
    let text = String::from_utf8_lossy(&bytes).trim().to_owned();
    if !text.starts_with('@') {
        return Err(ApiError::Unavailable {
            message: "the DOI service did not return BibTeX for this DOI".to_owned(),
        });
    }
    Ok(Bibtex { doi, bibtex: text })
}

#[cfg(test)]
mod tests {
    use super::clean_doi;

    #[test]
    fn dois_are_found_in_the_forms_people_paste() {
        for input in [
            "10.1145/3292500.3330701",
            " doi:10.1145/3292500.3330701 ",
            "https://doi.org/10.1145/3292500.3330701",
            "https://dx.doi.org/10.1145/3292500.3330701",
        ] {
            assert_eq!(
                clean_doi(input).as_deref(),
                Some("10.1145/3292500.3330701"),
                "{input}"
            );
        }
        assert!(
            clean_doi("10.1002/(SICI)1097-4571(199806)49:8<693::AID-ASI4>3.0.CO;2-O").is_none()
        );
        assert_eq!(
            clean_doi("10.1000/xyz-123_(a)").as_deref(),
            Some("10.1000/xyz-123_(a)")
        );
    }

    #[test]
    fn things_that_are_not_dois_are_refused() {
        for bad in [
            "",
            "hello",
            "10.1/x",
            "11.1234/x",
            "10.1234/",
            "10.1234/a b",
            "10.1234/a?x=1",
            "10.1234/../../etc",
            "file:///etc/passwd",
        ] {
            assert_eq!(clean_doi(bad), None, "{bad:?}");
        }
    }
}
