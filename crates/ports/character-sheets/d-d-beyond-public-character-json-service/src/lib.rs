//! The adapter of the `character-sheets` port over D&D Beyond's public
//! character JSON service.
//!
//! The service is unofficial and may change without notice: it answers
//! `GET <base>/<id>` — by default
//! `https://character-service.dndbeyond.com/character/v5/character/<id>` —
//! with the character's sheet as JSON when the character is shared as public.
//! There is no key: the service is public, and so nothing here holds a secret.
//!
//! [`DndBeyond`] builds the URL from a [`CharacterId`] and from nothing the DM
//! typed, follows no redirect (so no answer can send it to another host),
//! and reads the answer as follows:
//!
//! - **403 or 401**: [`CharacterSheetsError::NotPublic`]; **404**:
//!   [`CharacterSheetsError::NotFound`]. A 2xx whose body says
//!   `"success": false` is read by its message the same way.
//! - **429, 5xx, no answer, a timeout**: [`CharacterSheetsError::Unreachable`].
//! - **Any other status, a body that is not JSON, a body without what the
//!   [`map`] needs**: [`CharacterSheetsError::ServiceChanged`], naming what was
//!   missing: the unofficial endpoint changed, and that is an answer, not a
//!   crash.
//!
//! The mapping of the response to a sheet is in [`map`]. This crate passes
//! [`ports_character_sheets::contract`] on a recorded response served from
//! 127.0.0.1: no test reaches the service.

use std::time::Duration;

use ports_character_sheets::{CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError};
use reqwest::redirect::Policy;
use serde_json::Value;

mod map;

/// Where the public service answers.
pub const DEFAULT_BASE: &str = "https://character-service.dndbeyond.com/character/v5/character";

/// How long the service has to answer one request.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// How much of an answer is read: a character sheet is some hundred kilobytes.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// How much of the service's own message a reason keeps.
const MESSAGE_LIMIT: usize = 100;

/// The public character JSON service.
#[derive(Clone, Debug)]
pub struct DndBeyond {
    client: reqwest::Client,
    base: String,
}

impl DndBeyond {
    /// The adapter on the public service at [`DEFAULT_BASE`].
    ///
    /// # Errors
    ///
    /// [`CharacterSheetsError::Unreachable`] when no HTTP client can be built.
    pub fn new() -> Result<Self, CharacterSheetsError> {
        Self::at(DEFAULT_BASE)
    }

    /// The adapter on a service at `base` (what comes before `/<id>`): a
    /// recorded response served locally, for the tests.
    ///
    /// # Errors
    ///
    /// [`CharacterSheetsError::Unreachable`] when no HTTP client can be built.
    pub fn at(base: impl Into<String>) -> Result<Self, CharacterSheetsError> {
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(FETCH_TIMEOUT)
            .build()
            .map_err(|_| CharacterSheetsError::Unreachable("client HTTP indisponible".into()))?;
        Ok(Self {
            client,
            base: base.into().trim_end_matches('/').to_owned(),
        })
    }
}

impl CharacterSheets for DndBeyond {
    async fn fetch(&self, id: CharacterId) -> Result<CharacterSheet, CharacterSheetsError> {
        let response = self
            .client
            .get(format!("{}/{id}", self.base))
            .send()
            .await
            .map_err(|error| unreachable_because(&error))?;
        let status = response.status().as_u16();
        let body = read(response).await?;
        answer(id, status, &body)
    }
}

/// The body of `response`, as long as it stays under [`MAX_BODY`].
async fn read(mut response: reqwest::Response) -> Result<Vec<u8>, CharacterSheetsError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| unreachable_because(&error))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            return Err(CharacterSheetsError::ServiceChanged(
                "réponse démesurée".into(),
            ));
        }
    }
    Ok(body)
}

/// Why nothing answered, in words for the DM. The transport's own message
/// carries the URL, so it is not repeated.
fn unreachable_because(error: &reqwest::Error) -> CharacterSheetsError {
    let reason = if error.is_timeout() {
        "délai dépassé"
    } else {
        "aucune réponse du service"
    };
    CharacterSheetsError::Unreachable(reason.to_owned())
}

/// What the service's answer `status` and `body` to the request for `id` say.
fn answer(
    id: CharacterId,
    status: u16,
    body: &[u8],
) -> Result<CharacterSheet, CharacterSheetsError> {
    match status {
        401 | 403 => Err(CharacterSheetsError::NotPublic),
        404 => Err(CharacterSheetsError::NotFound),
        429 | 500..=599 => Err(CharacterSheetsError::Unreachable(format!(
            "le service a répondu HTTP {status}"
        ))),
        200..=299 => {
            let Ok(json) = serde_json::from_slice::<Value>(body) else {
                return Err(CharacterSheetsError::ServiceChanged(
                    "la réponse n'est pas du JSON".into(),
                ));
            };
            if json.get("success") == Some(&Value::Bool(false)) {
                return Err(refusal(&json));
            }
            map::sheet(id, &json)
        }
        other => Err(CharacterSheetsError::ServiceChanged(format!(
            "réponse HTTP {other} inattendue"
        ))),
    }
}

/// The error of a 2xx answer that says `"success": false`, by its message.
fn refusal(json: &Value) -> CharacterSheetsError {
    let message = json
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let lower = message.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    if says(&["not found", "does not exist", "doesn't exist"]) {
        CharacterSheetsError::NotFound
    } else if says(&[
        "private",
        "permission",
        "forbidden",
        "not public",
        "not shared",
    ]) {
        CharacterSheetsError::NotPublic
    } else {
        let message: String = message.chars().take(MESSAGE_LIMIT).collect();
        CharacterSheetsError::ServiceChanged(format!("success: false « {message} »"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> CharacterId {
        CharacterId::new(1).unwrap()
    }

    fn said(status: u16, body: &str) -> Result<CharacterSheet, CharacterSheetsError> {
        answer(id(), status, body.as_bytes())
    }

    #[test]
    fn the_statuses_are_read_as_what_they_say() {
        assert_eq!(said(401, ""), Err(CharacterSheetsError::NotPublic));
        assert_eq!(said(403, "{}"), Err(CharacterSheetsError::NotPublic));
        assert_eq!(said(404, "<html>"), Err(CharacterSheetsError::NotFound));
        for status in [429, 500, 502, 503, 599] {
            assert!(
                matches!(said(status, ""), Err(CharacterSheetsError::Unreachable(m)) if m.contains(&status.to_string())),
                "{status}"
            );
        }
        for status in [301, 302, 400, 418, 600] {
            assert_eq!(
                said(status, ""),
                Err(CharacterSheetsError::ServiceChanged(format!(
                    "réponse HTTP {status} inattendue"
                ))),
                "{status}"
            );
        }
    }

    #[test]
    fn a_success_that_is_not_json_is_a_change_of_shape() {
        for status in [200, 204, 299] {
            assert_eq!(
                said(status, "<html>maintenance</html>"),
                Err(CharacterSheetsError::ServiceChanged(
                    "la réponse n'est pas du JSON".into()
                )),
                "{status}"
            );
        }
    }

    #[test]
    fn a_success_that_says_it_failed_is_read_by_its_message() {
        let failed = |message: &str| {
            said(
                200,
                &format!(r#"{{"success": false, "message": "{message}", "data": null}}"#),
            )
        };
        for message in [
            "Character Not Found",
            "this character does not exist",
            "doesn't exist",
        ] {
            assert_eq!(
                failed(message),
                Err(CharacterSheetsError::NotFound),
                "{message}"
            );
        }
        for message in [
            "The character is PRIVATE",
            "No permission",
            "Forbidden",
            "Character not public",
            "Character is not shared",
        ] {
            assert_eq!(
                failed(message),
                Err(CharacterSheetsError::NotPublic),
                "{message}"
            );
        }
        assert_eq!(
            failed("Teapot"),
            Err(CharacterSheetsError::ServiceChanged(
                "success: false « Teapot »".into()
            ))
        );
        assert_eq!(
            said(200, r#"{"success": false}"#),
            Err(CharacterSheetsError::ServiceChanged(
                "success: false «  »".into()
            ))
        );
    }

    #[test]
    fn a_long_message_is_cut() {
        let long = "x".repeat(300);
        let Err(CharacterSheetsError::ServiceChanged(reason)) = said(
            200,
            &format!(r#"{{"success": false, "message": "{long}"}}"#),
        ) else {
            panic!("a change of shape")
        };
        assert_eq!(reason.matches('x').count(), MESSAGE_LIMIT);
    }

    #[test]
    fn the_base_loses_its_trailing_slash() {
        let adapter = DndBeyond::at("http://127.0.0.1:1/x//").unwrap();
        assert_eq!(adapter.base, "http://127.0.0.1:1/x");
        assert_eq!(DndBeyond::new().unwrap().base, DEFAULT_BASE);
    }
}
