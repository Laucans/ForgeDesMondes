//! The Anthropic Messages API (`ProviderApiType@1` = `cloud`).
//!
//! `POST {base}/messages` for text, `GET {base}/models/{id}` to learn whether
//! a model exists. Every request carries the key in `x-api-key` and the API
//! version in `anthropic-version`.

use std::time::Duration;

use ports_llm::Secret;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};

use crate::http::{Failure, endpoint, send};

/// The API version every request pins.
pub(crate) const VERSION: &str = "2023-06-01";

fn authorised(request: RequestBuilder, key: &Secret) -> RequestBuilder {
    request
        .header("x-api-key", key.expose())
        .header("anthropic-version", VERSION)
}

/// Asks `model` for `text`; returns the text it answered.
pub(crate) async fn complete(
    http: &Client,
    base: &str,
    key: &Secret,
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
    let url = endpoint(base, &["messages"])?;
    let reply = send(
        authorised(http.post(url), key).json(&body),
        Some(key),
        timeout,
    )
    .await?;
    if !reply.is_success() {
        return Err(reply.failure(Some(model)));
    }
    answer_text(&reply.json).ok_or(Failure::Malformed)
}

/// The text blocks of a message, joined; `None` when there is none.
fn answer_text(message: &Value) -> Option<String> {
    let text: String = message["content"]
        .as_array()?
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

/// Whether the API knows `model`: `Ok` if so, [`Failure::UnknownModel`] if not.
pub(crate) async fn check_model(
    http: &Client,
    base: &str,
    key: &Secret,
    model: &str,
    timeout: Duration,
) -> Result<(), Failure> {
    let url = endpoint(base, &["models", model])?;
    let reply = send(authorised(http.get(url), key), Some(key), timeout).await?;
    if reply.is_success() {
        Ok(())
    } else {
        Err(reply.failure(Some(model)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_text_blocks_and_only_them() {
        let message = json!({"content": [
            {"type": "thinking", "thinking": "hmm"},
            {"type": "text", "text": "ok"},
            {"type": "text", "text": " !"},
        ]});
        assert_eq!(answer_text(&message).as_deref(), Some("ok !"));
    }

    #[test]
    fn a_message_without_text_has_no_answer() {
        assert_eq!(answer_text(&json!({"content": []})), None);
        assert_eq!(
            answer_text(&json!({"content": [{"type": "text", "text": " "}]})),
            None
        );
        assert_eq!(answer_text(&json!({"error": "x"})), None);
    }
}
