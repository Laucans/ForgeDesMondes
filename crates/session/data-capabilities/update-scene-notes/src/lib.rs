//! DataCapability `session.updateSceneNotes` (`data-capability.json`): the DM
//! writes the notes of a scene already in the session they are preparing,
//! and the scene keeps them.
//!
//! The engine writes one partition per command, `Session/<scene id>`, and
//! judges the row it leaves. [`request`] reads the wire payload, [`prepare`]
//! turns the request and the scene's row into the row written, or into the
//! [`Refusal`] that says why not — before anything is queued.
//!
//! - The notes are a text, kept as written: a blank text is a text, not the
//!   absence of notes ([`Refusal::InvalidNotes`] for null or any other type).
//! - The scene must be the session's: one the store does not hold is
//!   [`Refusal::Unknown`] (404), one of another session
//!   [`Refusal::OtherSession`] (422), and no notes change.
//!
//! The scene's row is the engine's when it holds one (a scene added through
//! `session.addScene`), else the `scene` table's (a scene the server loaded
//! from its fixtures). The notes are the one field it changes (`touches`);
//! it still writes the scene's session, title and links as they are, which
//! the projection of the scenes needs to find the row in its tables
//! whichever of the two held it — a scene written again takes its row's
//! title, notes and links.

use std::collections::BTreeMap;

use aggregate_session::{LINKS, NOTES, PARTITION, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.updateSceneNotes";

/// The payload's fields naming the session and the scene.
const SESSION: &str = "session";
const SCENE: &str = "scene";

/// The scene `$1` names in its table: its session, its title and the ids of
/// the elements it links, in order.
const SCENE_ROW: &str =
    "SELECT sc.id::text AS id, sc.session_id::text AS session, sc.title AS title,
    coalesce((SELECT jsonb_agg(se.entity_id::text ORDER BY se.position)
        FROM scene_entity se WHERE se.scene_id = sc.id), '[]'::jsonb) AS links
    FROM scene sc WHERE sc.id = $1::text::uuid";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the command asks for, read from the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The scene it writes, lowercase and hyphenated.
    pub id: String,
    /// The session the scene must belong to, lowercase and hyphenated.
    pub session: String,
    /// The notes, as written.
    pub notes: String,
}

/// Why the command is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one.
    NotAnId(&'static str),
    /// The notes are no text.
    InvalidNotes,
    /// The store holds no such scene.
    Unknown,
    /// The scene belongs to another session.
    OtherSession,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub const fn not_found(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} : un identifiant"),
            Self::InvalidNotes => f.write_str("notes : un texte, même vide"),
            Self::Unknown => f.write_str("scène introuvable"),
            Self::OtherSession => f.write_str("scene : cette scène n'est pas de la séance"),
        }
    }
}

/// What the command becomes before it is submitted: the partition it
/// writes and the fields it writes there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// The row's id: the partition the command lands on.
    pub target_id: String,
    /// The fields written.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Prepares)
}

/// The command `wire` asks for on `target`, the scene the command names.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for a session or a scene that is no uuid;
/// [`Refusal::InvalidNotes`] for notes that are no text.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let id = target
        .or_else(|| wire.get(SCENE).and_then(Value::as_str))
        .and_then(uuid)
        .ok_or(Refusal::NotAnId(SCENE))?;
    let session = wire
        .get(SESSION)
        .and_then(Value::as_str)
        .and_then(uuid)
        .ok_or(Refusal::NotAnId(SESSION))?;
    let Some(Value::String(notes)) = wire.get(NOTES) else {
        return Err(Refusal::InvalidNotes);
    };
    Ok(Request {
        id,
        session,
        notes: notes.clone(),
    })
}

/// The row `request` writes, `row` being the scene it names as the engine
/// or the table holds it.
///
/// # Errors
///
/// [`Refusal::Unknown`] when the row is no scene's;
/// [`Refusal::OtherSession`] when the scene is not the request's session's.
pub fn prepare(request: &Request, row: &Row) -> Result<Prepared, Refusal> {
    if !row.contains_key(SCENE_ID) {
        return Err(Refusal::Unknown);
    }
    let session = row
        .get(SCENE_SESSION_ID)
        .and_then(Value::as_str)
        .and_then(uuid);
    if session.as_deref() != Some(request.session.as_str()) {
        return Err(Refusal::OtherSession);
    }
    let held = |field: &str, none: Value| row.get(field).cloned().unwrap_or(none);
    Ok(Prepared {
        target_id: request.id.clone(),
        payload: Row::from([
            (SCENE_ID.to_owned(), json!(request.id)),
            (SCENE_SESSION_ID.to_owned(), json!(request.session)),
            (SCENE_TITLE.to_owned(), held(SCENE_TITLE, Value::Null)),
            (LINKS.to_owned(), held(LINKS, json!([]))),
            (NOTES.to_owned(), json!(request.notes)),
        ]),
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// The scene's row as the engine holds it, from the `scene` table's `found`
/// (`SCENE_ROW`): the fields a scene's row holds.
fn from_table(found: &Value) -> Row {
    Row::from([
        (SCENE_ID.to_owned(), found["id"].clone()),
        (SCENE_SESSION_ID.to_owned(), found["session"].clone()),
        (SCENE_TITLE.to_owned(), found["title"].clone()),
        (LINKS.to_owned(), found["links"].clone()),
    ])
}

/// The command made the row it writes ([`prepare`]), judged against the
/// scene's row as the queue leaves it: refused 404 for what is not there,
/// 422 otherwise — nothing queued.
struct Prepares;

#[async_trait]
impl Prepare for Prepares {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let held = context.row(&format!("{PARTITION}{}", request.id)).await?;
        let row = if held.contains_key(SCENE_ID) {
            held
        } else {
            in_table(context, &request.id).await?
        };
        let prepared = prepare(&request, &row).map_err(refused)?;
        Ok(plugins::Prepared {
            target_id: Some(prepared.target_id),
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}

/// The scene `id` as its table holds it; empty when the table does not, or
/// when there is no table to read (over memory). A failed read is
/// unprocessable.
async fn in_table(context: &Context<'_>, id: &str) -> Result<Row, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Row::new());
    };
    let found = rows
        .rows(SCENE_ROW, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .and_then(|rows| rows.first())
        .map(from_table)
        .unwrap_or_default())
}

/// The engine's refusal of `refusal`: 404 for what is not there, 422
/// otherwise.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal.not_found() {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}
