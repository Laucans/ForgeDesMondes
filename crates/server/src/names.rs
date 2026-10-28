//! The names ids go by on the wire, where they are not the ones the store
//! holds (D8): the mock-up's fixtures name their rows by slug
//! (`cendres-valombre`, `elowen`) while the store holds a uuid for each
//! ([`crate::Fixtures::names`]).
//!
//! It is the product's own, around the engine's routes: every text of what a
//! caller sends that is a wire name becomes its stored id before the route
//! runs, and every text of what the route answers that is a stored id
//! becomes the name the caller knows. On `POST /capabilities/{id}`, a read's
//! `variables` in and its `data` out; on `POST /commands`, a command's
//! target (`target`, `targetId`), its `payload` and the values it was
//! `basedOn` in, and the whole answer out; on `GET /commands/{id}`, the
//! whole answer. A whole text is renamed, nested or not, as a value; as the
//! key of an object only in the maps keyed by a named thing (`pjInitiative`,
//! `startCombat`'s `initiative` by character and `rolls` by token), so that no other object can see two keys
//! collapse into one name. A part of a text never is. Nothing else is touched:
//! no other route, no refusal, no other field, and a body nothing renames
//! passes byte for byte.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

/// The routes whose requests and answers are renamed.
const READS: &str = "/capabilities/";

/// Where commands are submitted, and, below it, where one is followed.
const COMMANDS: &str = "/commands";

/// The fields of a command that name rows: its target, in either form, its
/// payload and what its author read. Never its DataCapability, its caller or
/// its idempotency key.
const COMMAND: &[&str] = &["target", "targetId", "payload", "basedOn"];

/// The fields whose object is keyed by a named thing (a character, an NPC
/// token): its keys are renamed too.
const KEYED: &[&str] = &["pjInitiative", "initiative", "rolls"];

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

    /// `value` with every text that is a wire name turned into its stored
    /// id, the keys of the maps keyed by character included.
    fn to_stored(&self, value: &Value) -> Value {
        Self::rename(&self.stored, value, false)
    }

    /// `value` with every text that is a stored id turned into its wire name,
    /// the keys of the maps keyed by character included.
    fn to_wire(&self, value: &Value) -> Value {
        Self::rename(&self.wire, value, false)
    }

    /// `value` renamed by `by`; its object keys too when `keyed`.
    fn rename(by: &BTreeMap<String, String>, value: &Value, keyed: bool) -> Value {
        match value {
            Value::String(text) => by
                .get(text)
                .map_or_else(|| value.clone(), |to| to.clone().into()),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| Self::rename(by, v, keyed)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| {
                        let key = if keyed { by.get(k).unwrap_or(k) } else { k };
                        (
                            key.clone(),
                            Self::rename(by, v, KEYED.contains(&k.as_str())),
                        )
                    })
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
}

/// `app` with its reads and commands renamed by `names`; `app` itself when
/// no id has another name.
pub fn renaming(app: Router, names: Names) -> Router {
    if names.stored.is_empty() {
        return app;
    }
    app.layer(middleware::from_fn_with_state(Arc::new(names), renamed))
}

/// What of a JSON body is renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// These fields of its object.
    Fields(&'static [&'static str]),
    /// All of it.
    Whole,
}

/// How one route is renamed: what of its request, when any, and what of its
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Route {
    request: Option<Part>,
    answer: Part,
}

/// How the route `method` `path` names is renamed; none when it is not.
fn route(method: &Method, path: &str) -> Option<Route> {
    if path.starts_with(READS) {
        return Some(Route {
            request: Some(Part::Fields(&["variables"])),
            answer: Part::Fields(&["data"]),
        });
    }
    if *method == Method::POST && path == COMMANDS {
        return Some(Route {
            request: Some(Part::Fields(COMMAND)),
            answer: Part::Whole,
        });
    }
    let followed = path
        .strip_prefix(COMMANDS)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|id| !id.is_empty() && !id.contains('/'));
    (*method == Method::GET && followed).then_some(Route {
        request: None,
        answer: Part::Whole,
    })
}

/// One request through the renaming: what its route renames in, and out.
async fn renamed(State(names): State<Arc<Names>>, request: Request, next: Next) -> Response {
    let Some(route) = route(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let request = match route.request {
        None => request,
        Some(part) => {
            let (mut parts, body) = request.into_parts();
            let Ok(bytes) = to_bytes(body, LIMIT).await else {
                // Too large or broken: what the route would have refused anyway.
                return StatusCode::PAYLOAD_TOO_LARGE.into_response();
            };
            let bytes = turned(&bytes, part, |v| names.to_stored(v), &mut parts.headers);
            Request::from_parts(parts, Body::from(bytes))
        }
    };
    let response = next.run(request).await;
    if !response.status().is_success() {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return Response::from_parts(parts, Body::empty());
    };
    let bytes = turned(
        &bytes,
        route.answer,
        |v| names.to_wire(v),
        &mut parts.headers,
    );
    Response::from_parts(parts, Body::from(bytes))
}

/// `bytes` with `part` of their JSON turned by `turn`, and `headers` no
/// longer stating a length; `bytes` as they are when they are no JSON, when
/// `part` names fields of something that is no object, or when nothing in
/// them is turned.
fn turned(
    bytes: &Bytes,
    part: Part,
    turn: impl Fn(&Value) -> Value,
    headers: &mut HeaderMap,
) -> Bytes {
    let Ok(mut sent) = serde_json::from_slice::<Value>(bytes) else {
        return bytes.clone();
    };
    let changed = match part {
        Part::Whole => replaced(&mut sent, &turn),
        Part::Fields(names) => {
            let Value::Object(object) = &mut sent else {
                return bytes.clone();
            };
            let mut changed = false;
            for name in names {
                if let Some(value) = object.get_mut(*name) {
                    changed |= replaced(value, &turn);
                }
            }
            changed
        }
    };
    if !changed {
        return bytes.clone();
    }
    headers.remove(header::CONTENT_LENGTH);
    Bytes::from(sent.to_string())
}

/// `value` turned by `turn`, in place: whether that changed it.
fn replaced(value: &mut Value, turn: impl Fn(&Value) -> Value) -> bool {
    let turned = turn(value);
    if turned == *value {
        return false;
    }
    *value = turned;
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::routing::{get, post};
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

    #[test]
    fn a_key_that_is_a_name_is_renamed_as_a_value_is_and_a_part_of_one_never_is() {
        let names = names();
        let sent = json!({ "initiative": { "elowen": 18, "julie": 9, "elowen-x": 4 } });
        let stored = json!({ "initiative": { "uuid-elowen": 18, "uuid-julie": 9, "elowen-x": 4 } });
        assert_eq!(names.to_stored(&sent), stored);
        assert_eq!(
            names.to_wire(&stored),
            json!({
                "initiative": { "elowen": 18, "julie": 9, "elowen-x": 4 }
            })
        );
        // Nested in an array, and a key that is no name stays.
        let nested = json!([{ "pjInitiative": { "uuid-julie": 9, "kind": 1 } }]);
        assert_eq!(
            names.to_wire(&nested),
            json!([{ "pjInitiative": { "julie": 9, "kind": 1 } }])
        );
        // `startCombat`'s rolls are keyed by NPC token.
        let rolls = json!({ "initiative": {}, "rolls": { "julie": 12 } });
        assert_eq!(
            names.to_stored(&rolls),
            json!({ "initiative": {}, "rolls": { "uuid-julie": 12 } })
        );
        // Any other object keeps its keys: two of them could collapse into one name.
        let other =
            json!({ "counts": { "uuid-elowen": 1, "elowen": 2 }, "uuid-julie": "uuid-julie" });
        assert_eq!(
            names.to_wire(&other),
            json!({ "counts": { "uuid-elowen": 1, "elowen": 2 }, "uuid-julie": "julie" })
        );
    }

    /// What the route behind the renaming was sent, body by body.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A server whose every route keeps the body it is sent and answers
    /// `status` with `answer`; a body its stated length disagrees with is
    /// refused (400).
    fn echo(seen: &Seen, status: StatusCode, answer: &Value) -> Router {
        let handler = {
            let (seen, answer) = (seen.clone(), answer.to_string());
            move |headers: HeaderMap, body: String| {
                let (seen, answer) = (seen.clone(), answer.clone());
                async move {
                    let stated = headers
                        .get(header::CONTENT_LENGTH)
                        .map(|length| length.to_str().expect("ASCII").to_string());
                    if stated.is_some_and(|length| length != body.len().to_string()) {
                        return (
                            StatusCode::BAD_REQUEST,
                            [(header::CONTENT_TYPE, "text/plain")],
                            "stale length".to_string(),
                        );
                    }
                    seen.lock().expect("unpoisoned").push(body);
                    (status, [(header::CONTENT_TYPE, "application/json")], answer)
                }
            }
        };
        Router::new()
            .route("/capabilities/{id}", post(handler.clone()))
            .route("/commands", post(handler.clone()))
            .route("/commands/{id}", get(handler.clone()))
            .route("/commands/{id}/confirm", post(handler.clone()))
            .route("/holds", post(handler))
    }

    /// What `app` answers to `body` on `uri` with `POST`: its status and body.
    async fn send(app: Router, uri: &str, body: &str) -> (StatusCode, String) {
        sent(app, Method::POST, uri, body).await
    }

    /// What `app` answers to `body` on `uri` with `method`: its status and
    /// body, whose length the answer states truly when it states one.
    async fn sent(app: Router, method: Method, uri: &str, body: &str) -> (StatusCode, String) {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = app.oneshot(request).await.expect("an answer");
        let status = response.status();
        let stated = response.headers().get(header::CONTENT_LENGTH).cloned();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        if let Some(length) = stated {
            assert_eq!(
                length.to_str().expect("ASCII"),
                bytes.len().to_string(),
                "the length stated"
            );
        }
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
    async fn a_command_runs_on_the_stored_ids_and_answers_the_callers_names() {
        let seen = Seen::default();
        let answer = json!({ "commandId": "c_1", "partition": "Entity/uuid-elowen",
            "warnings": ["uuid-elowen", "uuid-elowen is ahead"],
            "result": { "commandId": "c_1", "attachment": { "id": "uuid-elowen", "player": "uuid-julie" } },
            "then": [{ "submitted": { "partition": "Entity/uuid-julie", "result": { "commandId": "uuid-julie" } } }] });
        let app = renaming(echo(&seen, StatusCode::ACCEPTED, &answer), names());
        let sent = json!({
            "dataCapability": "elowen",
            "target": { "aggregate": "Entity", "id": "elowen" },
            "targetId": "julie",
            "payload": { "campaign": "julie", "entity": { "id": "elowen", "aliases": ["elowen", "x"],
                "notes": "elowen is a name inside a text", "ca": 13 } },
            "basedOn": { "version": 3, "values": { "player": "julie" } },
            "idempotencyKey": "elowen",
            "caller": { "by": "julie", "from": "elowen", "roles": [] },
        });
        let (status, body) = send(app, "/commands", &sent.to_string()).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(
            json_of(&body),
            json!({ "commandId": "c_1", "partition": "Entity/uuid-elowen",
                "warnings": ["elowen", "uuid-elowen is ahead"],
                "result": { "commandId": "c_1", "attachment": { "id": "elowen", "player": "julie" } },
                "then": [{ "submitted": { "partition": "Entity/uuid-julie", "result": { "commandId": "julie" } } }] }),
            "the whole answer renamed, a whole text at a time"
        );
        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            json_of(&seen[0]),
            json!({
                "dataCapability": "elowen",
                "target": { "aggregate": "Entity", "id": "uuid-elowen" },
                "targetId": "uuid-julie",
                "payload": { "campaign": "uuid-julie", "entity": { "id": "uuid-elowen",
                    "aliases": ["uuid-elowen", "x"], "notes": "elowen is a name inside a text", "ca": 13 } },
                "basedOn": { "version": 3, "values": { "player": "uuid-julie" } },
                "idempotencyKey": "elowen",
                "caller": { "by": "julie", "from": "elowen", "roles": [] },
            }),
            "the target, the payload and what it was based on renamed, nothing else"
        );
    }

    #[tokio::test]
    async fn the_keys_of_what_a_read_or_a_command_carries_are_renamed_where_its_values_are() {
        let seen = Seen::default();
        let answer = json!({ "data": { "pjInitiative": { "uuid-elowen": 18 } },
            "uuid-julie": 1 });
        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/capabilities/table.getBattlemapInstance", "{}").await;
        assert_eq!(
            json_of(&body),
            json!({ "data": { "pjInitiative": { "elowen": 18 } }, "uuid-julie": 1 }),
            "the keys of the data, not of the envelope"
        );

        let app = renaming(echo(&seen, StatusCode::ACCEPTED, &json!({})), names());
        let sent = json!({ "payload": { "initiative": { "elowen": 18 } }, "elowen": 1 });
        send(app, "/commands", &sent.to_string()).await;
        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(
            json_of(seen.last().expect("a command")),
            json!({ "payload": { "initiative": { "uuid-elowen": 18 } }, "elowen": 1 }),
            "the keys of the payload, not of the command"
        );
    }

    #[tokio::test]
    async fn a_command_followed_answers_the_callers_names_and_its_other_routes_pass_as_they_are() {
        let seen = Seen::default();
        let answer = json!({ "result": { "commandId": "c_1", "attachment": ["uuid-elowen", "uuid-elowen!"] } });
        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (status, body) = sent(app, Method::GET, "/commands/c_1", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json_of(&body),
            json!({ "result": { "commandId": "c_1", "attachment": ["elowen", "uuid-elowen!"] } })
        );

        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/commands/c_1/confirm", r#"{"by":"elowen"}"#).await;
        assert_eq!(json_of(&body), answer, "a confirmation is not renamed");

        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(seen, ["", r#"{"by":"elowen"}"#]);
    }

    #[test]
    fn the_routes_renamed_are_a_read_a_submission_and_a_command_followed() {
        let read = Some(Route {
            request: Some(Part::Fields(&["variables"])),
            answer: Part::Fields(&["data"]),
        });
        let submitted = Some(Route {
            request: Some(Part::Fields(COMMAND)),
            answer: Part::Whole,
        });
        let followed = Some(Route {
            request: None,
            answer: Part::Whole,
        });
        for (method, path, renamed) in [
            (Method::POST, "/capabilities/campaign.getEntity", read),
            (Method::POST, "/commands", submitted),
            (Method::GET, "/commands/c_1", followed),
            (Method::GET, "/commands", None),
            (Method::POST, "/commands/c_1", None),
            (Method::GET, "/commands/c_1/confirm", None),
            (Method::POST, "/commands/c_1/confirm", None),
            (Method::GET, "/commandsc_1", None),
            (Method::POST, "/holds", None),
        ] {
            assert_eq!(route(&method, path), renamed, "{method} {path}");
        }
    }

    #[tokio::test]
    async fn another_route_a_refusal_and_a_body_that_is_no_object_pass_as_they_are() {
        let seen = Seen::default();
        let answer = json!({ "data": "uuid-elowen" });
        let sent = json!({ "variables": { "id": "elowen" }, "payload": "elowen" }).to_string();

        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/holds", &sent).await;
        assert_eq!(json_of(&body), answer, "a hold is no read and no command");

        let refusing = renaming(echo(&seen, StatusCode::NOT_FOUND, &answer), names());
        let (status, body) = send(refusing, "/capabilities/x.y", &sent).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json_of(&body), answer, "a refusal is not renamed");
        let refusing = renaming(
            echo(&seen, StatusCode::UNPROCESSABLE_ENTITY, &answer),
            names(),
        );
        let (status, body) = send(refusing, "/commands", &sent).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(json_of(&body), answer, "a command refused is not renamed");

        let app = renaming(echo(&seen, StatusCode::OK, &answer), names());
        let (_, body) = send(app, "/capabilities/x.y", "\"elowen\"").await;
        assert_eq!(json_of(&body), json!({ "data": "elowen" }));
        let app = renaming(
            echo(&seen, StatusCode::OK, &json!(["uuid-elowen"])),
            names(),
        );
        let (_, body) = send(app, "/capabilities/x.y", "{\"id\":\"elowen\"}").await;
        assert_eq!(json_of(&body), json!(["uuid-elowen"]), "no data field");
        let app = renaming(echo(&seen, StatusCode::ACCEPTED, &answer), names());
        let (_, body) = send(app, "/commands", "[\"elowen\"]").await;
        assert_eq!(json_of(&body), json!({ "data": "elowen" }));

        let seen = seen.lock().expect("unpoisoned").clone();
        assert_eq!(
            seen.iter().map(|s| json_of(s)).collect::<Vec<_>>(),
            [
                json_of(&sent),
                json!({ "variables": { "id": "uuid-elowen" }, "payload": "elowen" }),
                json!({ "variables": { "id": "elowen" }, "payload": "uuid-elowen" }),
                json!("elowen"),
                json!({ "id": "elowen" }),
                json!(["elowen"]),
            ],
            "a read's variables and a command's payload are renamed, nothing else"
        );
    }

    #[tokio::test]
    async fn a_body_nothing_renames_passes_byte_for_byte() {
        let seen = Seen::default();
        let answer = r#"{ "result": {"status":"applied", "commandId": "c_1"} }"#;
        let app = Router::new().route(
            "/commands",
            post({
                let seen = seen.clone();
                move |body: String| {
                    let seen = seen.clone();
                    async move {
                        seen.lock().expect("unpoisoned").push(body);
                        (
                            StatusCode::ACCEPTED,
                            [(header::CONTENT_TYPE, "application/json")],
                            answer,
                        )
                    }
                }
            }),
        );
        let app = renaming(app, names());
        let sent = r#"{ "payload": {"z": "zed", "a": "uuid-elowen"},  "target": {"id": "x"} }"#;
        let (status, body) = send(app, "/commands", sent).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body, answer, "the answer as the route wrote it");
        assert_eq!(
            seen.lock().expect("unpoisoned").clone(),
            [sent],
            "the command as its author wrote it"
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
