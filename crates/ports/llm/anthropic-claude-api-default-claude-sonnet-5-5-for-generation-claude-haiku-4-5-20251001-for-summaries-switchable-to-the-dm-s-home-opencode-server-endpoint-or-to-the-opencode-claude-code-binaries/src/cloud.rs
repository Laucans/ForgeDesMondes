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

/// Asks `model` for `text`; returns the text it answered, which may be empty.
pub(crate) async fn complete(
    http: &Client,
    url: &str,
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
    let url = endpoint(url, &["messages"])?;
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

/// The text blocks of a message, joined; `None` when it is no message at all
/// (no `content` list).
fn answer_text(message: &Value) -> Option<String> {
    Some(
        message["content"]
            .as_array()?
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect(),
    )
}

/// Whether the API knows `model`: `Ok` if so, [`Failure::UnknownModel`] if not.
pub(crate) async fn check_model(
    http: &Client,
    url: &str,
    key: &Secret,
    model: &str,
    timeout: Duration,
) -> Result<(), Failure> {
    let url = endpoint(url, &["models", model])?;
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
    fn a_message_without_text_answers_nothing_and_a_page_is_no_message() {
        assert_eq!(answer_text(&json!({"content": []})).as_deref(), Some(""));
        assert_eq!(answer_text(&json!({"error": "x"})), None);
        assert_eq!(answer_text(&Value::Null), None);
    }
}
