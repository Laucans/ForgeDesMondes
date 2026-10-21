//! The names ids go by on the wire, where they are not the ones the store
//! holds (D8): the mock-up's fixtures name their rows by slug
//! (`cendres-valombre`, `elowen`) while the store holds a uuid for each
//! ([`crate::Fixtures::names`]).
//!
//! It is the product's own, around the engine's reads: on
//! `POST /capabilities/{id}`, every text of the request's `variables` that
//! is a wire name becomes its stored id before the query runs, and every
//! text of a served answer's `data` that is a stored id becomes the name the
//! caller knows. A whole text is renamed, nested or not; a part of one never
//! is. Nothing else is touched: no other route, no refusal, no other field.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

/// The routes whose requests and answers are renamed.
const READS: &str = "/capabilities/";

/// The largest body renamed: axum's own limit on a JSON body. A larger one
/// passes as it is, for the route to refuse.
const LIMIT: usize = 2 * 1024 * 1024;

/// Wire name ↔ stored id, both ways.
#[derive(Debug, Default)]
pub struct Names {
    /// Wire name → stored id.
    stored: BTreeMap<String, String>,
    /// Stored id → wire name.
    wire: BTreeMap<String, String>,
}

impl Names {
    /// These `(wire name, stored id)` pairs as the names ids go by on the
    /// wire.
    #[must_use]
    pub fn new(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut names = Self::default();
        for (wire, stored) in pairs {
            names.wire.insert(stored.clone(), wire.clone());
            names.stored.insert(wire, stored);
        }
        names
    }

    /// `value` with every text that is a wire name turned into its stored id.
    fn to_stored(&self, value: &Value) -> Value {
        Self::rename(&self.stored, value)
    }

    /// `value` with every text that is a stored id turned into its wire name.
    fn to_wire(&self, value: &Value) -> Value {
        Self::rename(&self.wire, value)
    }

    fn rename(by: &BTreeMap<String, String>, value: &Value) -> Value {
        match value {
            Value::String(text) => by
                .get(text)
                .map_or_else(|| value.clone(), |to| to.clone().into()),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| Self::rename(by, v)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::rename(by, v)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}

/// `app` with its reads renamed by `names`; `app` itself when no id has
/// another name.
pub fn renaming(app: Router, names: Names) -> Router {
    if names.stored.is_empty() {
        return app;
    }
    app.layer(middleware::from_fn_with_state(Arc::new(names), renamed))
}

/// One request through the renaming: a read's variables in, its data out.
async fn renamed(State(names): State<Arc<Names>>, request: Request, next: Next) -> Response {
    if !request.uri().path().starts_with(READS) {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, LIMIT).await else {
        // Too large or broken: what the route would have refused anyway.
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let bytes = field(
        &bytes,
        "variables",
        |v| names.to_stored(v),
        &mut parts.headers,
    );
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if !response.status().is_success() {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return Response::from_parts(parts, Body::empty());
    };
    let bytes = field(&bytes, "data", |v| names.to_wire(v), &mut parts.headers);
    Response::from_parts(parts, Body::from(bytes))
}

/// `bytes` with the field `name` of its JSON object turned by `turn`, and
/// `headers` no longer stating a length; `bytes` as they are when they are
/// no such object.
fn field(
    bytes: &Bytes,
    name: &str,
    turn: impl Fn(&Value) -> Value,
    headers: &mut HeaderMap,
) -> Bytes {
    let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(bytes) else {
        return bytes.clone();
    };
    let Some(value) = object.get_mut(name) else {
        return bytes.clone();
    };
    *value = turn(value);
    headers.remove(header::CONTENT_LENGTH);
    Bytes::from(Value::Object(object).to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::http::Method;
    use axum::routing::post;
    use serde_json::json;
    use tower::ServiceExt;

    use super::*;

    fn names() -> Names {
        Names::new([
            ("elowen".to_string(), "uuid-elowen".to_string()),
            ("julie".to_string(), "uuid-julie".to_string()),
        ])
    }

    #[test]
    fn names_turn_the_callers_names_into_stored_ids_and_back() {
        let names = names();
        let sent = json!({ "id": "elowen", "other": "unnamed", "n": 3 });
        assert_eq!(
            names.to_stored(&sent),
            json!({ "id": "uuid-elowen", "other": "unnamed", "n": 3 })
        );
        let answer = json!([{ "id": "uuid-elowen", "player": "uuid-julie",
            "aliases": ["elowen", "uuid-x"], "note": "uuid-elowen is a name inside a text" }]);
        assert_eq!(
            names.to_wire(&answer),
            json!([{ "id": "elowen", "player": "julie",
                "aliases": ["elowen", "uuid-x"], "note": "uuid-elowen is a name inside a text" }]),
            "a whole text is renamed, nested or not; a part of one never is"
        );
    }

    /// What the route behind the renaming was sent, body by body.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A server whose every route keeps the body it is sent and answers
    /// `status` with `answer`.
    fn echo(seen: &Seen, status: StatusCode, answer: &Value) -> Router {
        let handler = {
            let (seen, answer) = (seen.clone(), answer.to_string());
            move |body: String| {
                let (seen, answer) = (seen.clone(), answer.clone());
                async move {
                    seen.lock().expect("unpoisoned").push(body);
                    (status, [(header::CONTENT_TYPE, "application/json")], answer)
                }
            }
        };
        Router::new()
            .route("/capabilities/{id}", post(handler.clone()))
            .route("/commands", post(handler))
    }

    /// What `app` answers to `body` on `uri`: its status and body.
    async fn send(app: Router, uri: &str, body: &str) -> (StatusCode, String) {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = app.oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8(bytes.to_vec()).expect("UTF-8"))
    }

    fn json_of(text: &str) -> Value {
        serde_json::from_str(text).expect("JSON")
    }

    #[tokio::test]
    async fn a_read_runs_on_the_stored_ids_and_answers_the_callers_names() {
        let seen = Seen::default();
        let answer = json!({ "asOf": 7, "data": [{ "id": "uuid-elowen", "player": "uuid-julie" }],
            "uuid-elowen": "uuid-elowen" });
        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let sent = json!({ "sha256": "elowen", "variables": { "id": "elowen", "n": 1 } });
        let (status, body) = send(app, "/capabilities/campaign.getEntity", &sent.to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json_of(&body),
            json!({ "asOf": 7, "data": [{ "id": "elowen", "player": "julie" }],
                "uuid-elowen": "uuid-elowen" }),
            "the data renamed, nothing else"
        );
        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            json_of(&seen[0]),
            json!({ "sha256": "elowen", "variables": { "id": "uuid-elowen", "n": 1 } }),
            "the variables renamed, nothing else"
        );
    }

    #[tokio::test]
    async fn another_route_a_refusal_and_a_body_that_is_no_object_pass_as_they_are() {
        let seen = Seen::default();
        let answer = json!({ "data": "uuid-elowen" });
        let sent = json!({ "variables": { "id": "elowen" }, "payload": "elowen" }).to_string();

        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/commands", &sent).await;
        assert_eq!(json_of(&body), answer, "a write is no read");

        let refusing = renaming(echo(&seen, StatusCode::NOT_FOUND, &answer), names());
        let (status, body) = send(refusing, "/capabilities/x.y", &sent).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json_of(&body), answer, "a refusal is not renamed");

        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/capabilities/x.y", "\"elowen\"").await;
        assert_eq!(json_of(&body), json!({ "data": "elowen" }));
        let app = renaming(
            echo(&seen, StatusCode::OK, &json!(["uuid-elowen"])),
            names(),
        );
        let (_, body) = send(app, "/capabilities/x.y", "{\"id\":\"elowen\"}").await;
        assert_eq!(json_of(&body), json!(["uuid-elowen"]), "no data field");

        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(
            seen.iter().map(|s| json_of(s)).collect::<Vec<_>>(),
            [
                json_of(&sent),
                json!({ "variables": { "id": "uuid-elowen" }, "payload": "elowen" }),
                json!("elowen"),
                json!({ "id": "elowen" }),
            ],
            "only a read's variables are renamed"
        );
    }

    #[tokio::test]
    async fn with_no_name_nothing_is_renamed() {
        let seen = Seen::default();
        let answer = json!({ "data": "uuid-elowen" });
        let app = renaming(echo(&seen, StatusCode::OK, &answer), Names::default());
        let sent = json!({ "variables": { "id": "elowen" } }).to_string();
        let (_, body) = send(app, "/capabilities/x.y", &sent).await;
        assert_eq!(json_of(&body), answer);
        assert_eq!(seen.lock().expect("unpoisoned").clone(), [sent]);
    }

    #[tokio::test]
    async fn a_body_of_two_mebibytes_is_renamed() {
        // Axum's own limit on a JSON body, written out: the renaming takes
        // all of it.
        const TWO_MIB: usize = 2_097_152;
        let seen = Seen::default();
        let head = r#"{"variables":{"id":"elowen"},"pad":""#;
        let pad = TWO_MIB - head.len() - r#""}"#.len();
        let body = format!(r#"{head}{}"}}"#, " ".repeat(pad));
        assert_eq!(body.len(), TWO_MIB);
        // A name longer than its id: the renamed body stays within the
        // route's own limit.
        let shorter = Names::new([("elowen".to_string(), "e".to_string())]);
        let app = renaming(echo(&seen, StatusCode::OK, &json!({})), shorter);
        let (status, _) = send(app, "/capabilities/x.y", &body).await;
        assert_eq!(status, StatusCode::OK);
        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(json_of(&seen[0])["variables"], json!({ "id": "e" }));
    }

    #[tokio::test]
    async fn a_body_too_large_to_rename_is_refused() {
        let seen = Seen::default();
        let app = renaming(echo(&seen, StatusCode::OK, &json!({})), names());
        let (status, _) = send(app, "/capabilities/x.y", &" ".repeat(LIMIT + 1)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(seen.lock().expect("unpoisoned").is_empty());
    }
}
