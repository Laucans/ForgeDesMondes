//! What both API clients share: one HTTP exchange, and what it can fail with.
//!
//! The key goes in a header and nowhere else. A transport error is never
//! formatted (the client's own message carries the URL, and a URL may carry
//! userinfo), and whatever a provider says back is scrubbed of the key before
//! it becomes a reason, so no failure the DM reads or a log keeps can hold it.

use std::time::Duration;

use ports_llm::Secret;
use reqwest::{RequestBuilder, Url};
use serde_json::Value;

/// How much of a provider's explanation a reason keeps.
const DETAIL_LIMIT: usize = 200;

/// Why an exchange with a provider failed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Nothing answered: refused connection, no route, timeout.
    Unreachable(&'static str),
    /// The provider answered, but not to us: 401 or 403.
    Refused(u16),
    /// The provider does not know the model.
    UnknownModel(String),
    /// Any other error status, with the provider's own explanation.
    Rejected { status: u16, detail: String },
    /// The provider answered 2xx with an answer that has no text in it.
    Malformed,
}

impl Failure {
    /// What the DM reads.
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::Unreachable(why) => format!("injoignable : {why}"),
            Self::Refused(status) => {
                format!("refusé par le fournisseur (HTTP {status}) : clé API absente ou invalide")
            }
            Self::UnknownModel(model) => format!("modèle inconnu : {model}"),
            Self::Rejected { status, detail } if detail.is_empty() => {
                format!("erreur du fournisseur (HTTP {status})")
            }
            Self::Rejected { status, detail } => {
                format!("erreur du fournisseur (HTTP {status}) : {detail}")
            }
            Self::Malformed => "réponse du fournisseur illisible".to_owned(),
        }
    }
}

/// A provider's answer, whatever its status.
pub(crate) struct Reply {
    pub(crate) status: u16,
    /// The body as JSON; `Null` when it was not.
    pub(crate) json: Value,
    /// The provider's explanation of an error: scrubbed, and short.
    detail: String,
    /// Whether the body was an error object (`{"error": ...}`) and not a
    /// page: a proxy's 404 is not a provider's.
    structured: bool,
}

impl Reply {
    pub(crate) const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// The failure an error status stands for. `model` is the one the request
    /// asked for, if it asked for one.
    pub(crate) fn failure(&self, model: Option<&str>) -> Failure {
        let lower = self.detail.to_lowercase();
        let unknown = model.filter(|_| {
            (self.status == 404 && self.structured)
                || (self.status == 400
                    && lower.contains("model")
                    && ["not found", "does not exist", "unknown"]
                        .iter()
                        .any(|phrase| lower.contains(phrase)))
        });
        if let Some(model) = unknown {
            Failure::UnknownModel(model.to_owned())
        } else if self.status == 401 || self.status == 403 {
            Failure::Refused(self.status)
        } else {
            Failure::Rejected {
                status: self.status,
                detail: self.detail.clone(),
            }
        }
    }
}

/// Sends `request` and reads the whole answer.
pub(crate) async fn send(
    request: RequestBuilder,
    key: Option<&Secret>,
    timeout: Duration,
) -> Result<Reply, Failure> {
    let response = request
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| transport(&error))?;
    let status = response.status().as_u16();
    let body = response.bytes().await.map_err(|error| transport(&error))?;
    let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let structured = explanation(&json).is_some();
    let said = explanation(&json).map_or_else(
        || String::from_utf8_lossy(&body).into_owned(),
        str::to_owned,
    );
    Ok(Reply {
        status,
        json,
        detail: scrub(&said, key),
        structured,
    })
}

fn transport(error: &reqwest::Error) -> Failure {
    Failure::Unreachable(if error.is_timeout() {
        "délai dépassé"
    } else if error.is_connect() {
        "connexion impossible"
    } else {
        "échange interrompu"
    })
}

/// The message of an error body, in the Anthropic and the OpenAI shapes.
fn explanation(json: &Value) -> Option<&str> {
    json["error"]["message"]
        .as_str()
        .or_else(|| json["error"].as_str())
        .or_else(|| json["message"].as_str())
}

/// `text` without the key, cut to [`DETAIL_LIMIT`] characters. The key goes
/// first: cutting first could leave half of it in.
fn scrub(text: &str, key: Option<&Secret>) -> String {
    let text = match key.map(Secret::expose).filter(|key| !key.is_empty()) {
        Some(key) => text.replace(key, "***"),
        None => text.to_owned(),
    };
    text.trim().chars().take(DETAIL_LIMIT).collect()
}

/// The URL of `tail` in the API of `typed`, the URL the DM entered: under its
/// `/v1`, whether they typed the server's root, the `/v1`, or an endpoint they
/// have seen in the documentation. A query or a fragment is dropped.
pub(crate) fn endpoint(typed: &str, tail: &[&str]) -> Result<Url, Failure> {
    let invalid = || Failure::Unreachable("adresse invalide");
    let mut url = Url::parse(typed.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
        return Err(invalid());
    }
    url.set_query(None);
    url.set_fragment(None);
    let mut path: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    for suffix in [&["chat", "completions"][..], &["messages"], &["models"]] {
        if path.ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
        }
    }
    if path.last() != Some(&"v1") {
        path.push("v1");
    }
    let tail: Vec<String> = tail.iter().map(|segment| encode(segment)).collect();
    path.extend(tail.iter().map(String::as_str));
    url.set_path(&path.join("/"));
    Ok(url)
}

/// `segment` as one path segment: everything but the unreserved characters
/// percent-encoded.
fn encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn reply(status: u16, json: Value, key: Option<&Secret>) -> Reply {
        let said = explanation(&json).unwrap_or_default().to_owned();
        Reply {
            status,
            structured: explanation(&json).is_some(),
            json,
            detail: scrub(&said, key),
        }
    }

    #[test]
    fn an_endpoint_is_under_the_v1_of_whatever_was_typed() {
        for (typed, url) in [
            (
                "https://api.anthropic.com",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "https://api.anthropic.com/",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "https://api.anthropic.com/v1",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "https://api.anthropic.com/v1/",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "https://api.anthropic.com/v1/messages",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "http://192.168.1.40:4096",
                "http://192.168.1.40:4096/v1/messages",
            ),
            (
                " http://192.168.1.40:4096/v1/chat/completions ",
                "http://192.168.1.40:4096/v1/messages",
            ),
            (
                "http://hote/proxy/v1/models",
                "http://hote/proxy/v1/messages",
            ),
            ("http://hote:4096?x=1", "http://hote:4096/v1/messages"),
            ("https://hote/#ancre", "https://hote/v1/messages"),
        ] {
            assert_eq!(
                endpoint(typed, &["messages"]).unwrap().as_str(),
                url,
                "{typed:?}"
            );
        }
    }

    #[test]
    fn the_tail_is_appended_and_encoded_segment_by_segment() {
        let url = endpoint("http://hote:4096/v1", &["models", "a b/c"]).unwrap();
        assert_eq!(url.as_str(), "http://hote:4096/v1/models/a%20b%2Fc");
        let url = endpoint("http://hote", &["chat", "completions"]).unwrap();
        assert_eq!(url.as_str(), "http://hote/v1/chat/completions");
    }

    #[test]
    fn what_is_not_an_http_url_with_a_host_has_no_endpoint() {
        for typed in ["pas une url", "", "ftp://hote", "127.0.0.1"] {
            assert_eq!(
                endpoint(typed, &["models"]).unwrap_err(),
                Failure::Unreachable("adresse invalide"),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn a_404_with_an_error_object_on_a_request_for_a_model_is_that_model_unknown() {
        let body = json!({"error": {"type": "not_found_error", "message": "model: x"}});
        assert_eq!(
            reply(404, body.clone(), None).failure(Some("x")),
            Failure::UnknownModel("x".into())
        );
        // Without a model in the request, a 404 is only an error.
        assert_eq!(
            reply(404, body, None).failure(None),
            Failure::Rejected {
                status: 404,
                detail: "model: x".into()
            }
        );
    }

    #[test]
    fn a_404_that_is_not_an_error_object_is_a_wrong_address_not_a_wrong_model() {
        assert_eq!(
            reply(404, Value::Null, None).failure(Some("x")),
            Failure::Rejected {
                status: 404,
                detail: String::new()
            }
        );
    }
    #[test]
    fn a_400_is_an_unknown_model_only_when_it_says_so() {
        let said = |message: &str| reply(400, json!({"error": {"message": message}}), None);
        assert_eq!(
            said("The model `gpt-9` does not exist").failure(Some("gpt-9")),
            Failure::UnknownModel("gpt-9".into())
        );
        assert!(matches!(
            said("max_tokens: too large for this model").failure(Some("gpt-9")),
            Failure::Rejected { status: 400, .. }
        ));
    }

    #[test]
    fn a_401_and_a_403_are_a_refusal() {
        for status in [401, 403] {
            assert_eq!(
                reply(status, Value::Null, None).failure(Some("m")),
                Failure::Refused(status)
            );
        }
    }

    #[test]
    fn the_explanation_never_holds_the_key_even_cut_in_two() {
        let key = Secret::new("sk-ant-api03-très-secret");
        let long = format!("{} {}", "x".repeat(DETAIL_LIMIT - 5), key.expose());
        for said in [format!("clé {} refusée", key.expose()), long] {
            let kept = scrub(&said, Some(&key));
            assert!(!kept.contains("sk-ant"), "{kept}");
            assert!(!kept.contains("secret"), "{kept}");
            assert!(kept.chars().count() <= DETAIL_LIMIT);
        }
        assert_eq!(
            scrub("une clé sk-1 ici", Some(&Secret::new("sk-1"))),
            "une clé *** ici"
        );
    }

    #[test]
    fn an_empty_key_scrubs_nothing_and_no_key_scrubs_nothing() {
        assert_eq!(scrub("abc", Some(&Secret::new(""))), "abc");
        assert_eq!(scrub("abc", None), "abc");
    }

    #[test]
    fn only_a_2xx_is_a_success() {
        assert!(reply(200, Value::Null, None).is_success());
        assert!(reply(204, Value::Null, None).is_success());
        assert!(!reply(199, Value::Null, None).is_success());
        assert!(!reply(301, Value::Null, None).is_success());
        assert!(!reply(500, Value::Null, None).is_success());
    }

    #[test]
    fn a_reason_is_readable_and_names_what_failed() {
        assert_eq!(
            Failure::Unreachable("connexion impossible").reason(),
            "injoignable : connexion impossible"
        );
        assert!(Failure::Refused(401).reason().starts_with("refusé"));
        assert_eq!(
            Failure::UnknownModel("m".into()).reason(),
            "modèle inconnu : m"
        );
        assert_eq!(
            Failure::Rejected {
                status: 500,
                detail: String::new()
            }
            .reason(),
            "erreur du fournisseur (HTTP 500)"
        );
        assert_eq!(
            Failure::Rejected {
                status: 529,
                detail: "surchargé".into()
            }
            .reason(),
            "erreur du fournisseur (HTTP 529) : surchargé"
        );
    }
}
