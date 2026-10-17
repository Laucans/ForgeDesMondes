//! The OpenAI-compatible endpoint of the DM's home opencode server
//! (`ProviderApiType@1` = `opencode`).
//!
//! `GET {base}/models` lists the models (and is the answer the API type is
//! detected from), `POST {base}/chat/completions` answers a text. The server
//! may need no key: when there is one it goes in `Authorization: Bearer`.

use std::time::Duration;

use ports_llm::Secret;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};

use crate::http::{Failure, endpoint, send};

fn authorised(request: RequestBuilder, key: Option<&Secret>) -> RequestBuilder {
    match key {
        Some(key) => request.bearer_auth(key.expose()),
        None => request,
    }
}

/// The server's answer to its list of models.
pub(crate) async fn list_models(
    http: &Client,
    base: &str,
    key: Option<&Secret>,
    timeout: Duration,
) -> Result<Value, Failure> {
    let url = endpoint(base, &["models"])?;
    let reply = send(authorised(http.get(url), key), key, timeout).await?;
    if reply.is_success() {
        Ok(reply.json)
    } else {
        Err(reply.failure(None))
    }
}

/// Whether the list `models` (`{"data": [{"id": ...}]}`) holds `id`.
pub(crate) fn lists(models: &Value, id: &str) -> bool {
    models["data"]
        .as_array()
        .is_some_and(|data| data.iter().any(|model| model["id"] == id))
}

/// Asks `model` for `text`; returns the text it answered.
pub(crate) async fn complete(
    http: &Client,
    base: &str,
    key: Option<&Secret>,
    model: &str,
    text: &str,
    max_tokens: u32,
    timeout: Duration,
) -> Result<String, Failure> {
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{"role": "user", "content": text}],
    });
    let url = endpoint(base, &["chat", "completions"])?;
    let reply = send(authorised(http.post(url), key).json(&body), key, timeout).await?;
    if !reply.is_success() {
        return Err(reply.failure(Some(model)));
    }
    answer_text(&reply.json).ok_or(Failure::Malformed)
}

/// The content of the first choice; `None` when it is absent or blank.
fn answer_text(completion: &Value) -> Option<String> {
    let text = completion["choices"][0]["message"]["content"].as_str()?;
    (!text.trim().is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_first_choice() {
        let completion = json!({"choices": [
            {"message": {"role": "assistant", "content": "ok"}},
            {"message": {"role": "assistant", "content": "autre"}},
        ]});
        assert_eq!(answer_text(&completion).as_deref(), Some("ok"));
    }

    #[test]
    fn a_completion_without_content_has_no_answer() {
        assert_eq!(answer_text(&json!({"choices": []})), None);
        assert_eq!(
            answer_text(&json!({"choices": [{"message": {"content": null}}]})),
            None
        );
        assert_eq!(
            answer_text(&json!({"choices": [{"message": {"content": "  "}}]})),
            None
        );
    }

    #[test]
    fn a_model_is_listed_by_its_exact_id() {
        let models = json!({"object": "list", "data": [{"id": "qwen2.5-14b", "object": "model"}]});
        assert!(lists(&models, "qwen2.5-14b"));
        assert!(!lists(&models, "qwen2.5"));
        assert!(!lists(&json!({"object": "list"}), "qwen2.5-14b"));
    }
}
