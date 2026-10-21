//! DataCapability `session.reorderScenes` (`data-capability.json`): the DM
//! drags a session's scenes into the order they will play them, and the new
//! order is saved in one go.
//!
//! A reorder is one command on the session's scene order, a partition of
//! its own, `Session/<order id>` ([`aggregate_session::order_id`]): two
//! reorders of one session queue there, and the second waits for its author
//! to confirm it overwrites the first. What the screen sends is
//! `{ "session", "order": [sceneId] }`, and it becomes the order's row in
//! two steps:
//!
//! - [`asked`] reads the request: the session is a uuid that is no order's
//!   id — no session has any other ([`Refusal::UnknownSession`]); the order
//!   a list of uuids,
//!   lowercase as the tables write them, none repeated
//!   ([`Refusal::Repeated`]). Any other field is refused
//!   ([`Refusal::Unknown`]).
//! - [`ordered`] judges it against the session's scenes as the store holds
//!   them: one it omits ([`Refusal::Omitted`]) or one it names that is no
//!   scene of the session ([`Refusal::Foreign`]) breaks
//!   `scene-order-contiguous`. Nothing is queued.
//!
//! The ranks are not written here: the Session aggregate rewrites them all
//! in one statement when the order is applied
//! ([`aggregate_session::SceneOrder`]), 1 to n, so a scene added or removed
//! while it was queued leaves no gap nor duplicate.
//!
//! The session's scenes are those its ranks are written on: over
//! PostgreSQL, the rows of `scene` — the fixtures' among them; a scene still
//! queued has none yet, and an order naming it is refused. Over memory no
//! table lists them: the scenes the order names are the session's by their
//! engine row's `sceneSessionId`, and only an order that repeats a scene or
//! names another session's is refused there.

use std::collections::BTreeMap;

use aggregate_session::{SCENE_POSITION, is_uuid, order_id};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.reorderScenes";

/// The payload's fields.
const SESSION: &str = "session";
const ORDER: &str = "order";
const FIELDS: [&str; 2] = [SESSION, ORDER];

/// The session a scene's engine row is of, under the aggregate's field.
pub const SCENE_SESSION_ID: &str = "sceneSessionId";

/// The partitions of the Session aggregate: `Session/<id>`.
const PARTITION: &str = "Session/";

/// The invariant an order breaks when it is not every scene once.
const CONTIGUOUS: &str = "scene-order-contiguous";

/// The session `$1` names, when the tables hold it live.
const SESSION_ROW: &str = "SELECT s.id::text AS id
    FROM session s
    WHERE s.id = $1::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// The scenes of the session `$1` the table ranks: each one's id.
const SCENE_ROWS: &str = "SELECT sc.id::text AS id FROM scene sc WHERE sc.session_id = $1::uuid";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: the scenes of `session` in `order`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session's id, lowercase.
    pub session: String,
    /// The scenes' ids, lowercase, in the order they play.
    pub order: Vec<String>,
}

/// Why a reorder is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    Unknown(String),
    /// The store holds no such session.
    UnknownSession,
    /// The order is no list of uuids, or an empty one.
    NoOrder,
    /// The order names this scene twice.
    Repeated(String),
    /// The order omits this scene of the session.
    Omitted(String),
    /// The order names this id, which is no scene of the session.
    Foreign(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : un réordonnancement se décrit par session et order"
            ),
            Self::UnknownSession => write!(f, "séance introuvable"),
            Self::NoOrder => write!(
                f,
                "order : la liste des identifiants des scènes de la séance"
            ),
            Self::Repeated(scene) => {
                write!(f, "{CONTIGUOUS} : la scène {scene} est rangée deux fois")
            }
            Self::Omitted(scene) => write!(f, "{CONTIGUOUS} : l'ordre omet la scène {scene}"),
            Self::Foreign(scene) => {
                write!(f, "{CONTIGUOUS} : {scene} n'est pas une scène de la séance")
            }
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Reorder)
}

/// A reorder made the row it writes on its session's scene order, judged
/// against the session's scenes ([`scenes`]): one of a session there is not
/// is refused, 404; one malformed, or that is not every scene of the
/// session once (`scene-order-contiguous`), 422 — neither is queued.
struct Reorder;

#[async_trait]
impl Prepare for Reorder {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload).map_err(refused)?;
        if !session(context, &asked.session).await? {
            return Err(refused(Refusal::UnknownSession));
        }
        let scenes = scenes(context, &asked).await?;
        Ok(plugins::Prepared {
            payload: ordered(&asked, &scenes).map_err(refused)?,
            target_id: order_id(&asked.session),
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a session there is not is not
/// found, anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal == Refusal::UnknownSession {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}

/// The ids the tables' read answers, in order; none over memory. A failed
/// read is unprocessable.
async fn ids(context: &Context<'_>, sql: &str, id: &str) -> Result<Vec<String>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Vec::new());
    };
    let found = rows
        .rows(sql, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect())
}

/// Whether the store holds the session `id`: the tables, live, or the
/// engine's row as the commands queued on it leave it.
async fn session(context: &Context<'_>, id: &str) -> Result<bool, plugins::Refusal> {
    Ok(!ids(context, SESSION_ROW, id).await?.is_empty()
        || !context.row(&format!("{PARTITION}{id}")).await?.is_empty())
}

/// The scenes of `asked`'s session, those its ranks are written on: over
/// PostgreSQL, the rows of `scene`; over memory, the scenes the order names
/// whose engine row is of the session.
async fn scenes(context: &Context<'_>, asked: &Asked) -> Result<Vec<String>, plugins::Refusal> {
    if context.rows.is_some() {
        return ids(context, SCENE_ROWS, &asked.session).await;
    }
    let mut scenes = Vec::new();
    for scene in &asked.order {
        let row = context.row(&format!("{PARTITION}{scene}")).await?;
        if row.get(SCENE_SESSION_ID).and_then(Value::as_str) == Some(asked.session.as_str()) {
            scenes.push(scene.clone());
        }
    }
    Ok(scenes)
}

/// An id as the tables write it: a uuid, lowercase.
fn uuid(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|id| is_uuid(id))
        .map(str::to_ascii_lowercase)
}

/// What `wire` asks.
///
/// # Errors
///
/// [`Refusal`] when `wire` carries another field, names its session by
/// something other than a session's uuid ([`Refusal::UnknownSession`]),
/// gives no
/// non-empty list of uuids as its order ([`Refusal::NoOrder`]), or names a
/// scene twice ([`Refusal::Repeated`]).
pub fn asked(wire: &Row) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| !FIELDS.contains(&field.as_str())) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let session = wire
        .get(SESSION)
        .and_then(uuid)
        .filter(|session| order_id(session).is_some())
        .ok_or(Refusal::UnknownSession)?;
    let listed = wire
        .get(ORDER)
        .and_then(Value::as_array)
        .filter(|listed| !listed.is_empty())
        .ok_or(Refusal::NoOrder)?;
    let mut order: Vec<String> = Vec::with_capacity(listed.len());
    for scene in listed {
        let scene = uuid(scene).ok_or(Refusal::NoOrder)?;
        if order.contains(&scene) {
            return Err(Refusal::Repeated(scene));
        }
        order.push(scene);
    }
    Ok(Asked { session, order })
}

/// `asked` judged against its session's scenes, as the store holds them:
/// the row its order writes, its scenes' ids under `scenePosition`.
///
/// # Errors
///
/// [`Refusal::Foreign`] when the order names an id that is no scene of the
/// session, [`Refusal::Omitted`] when it leaves one of them out.
pub fn ordered(asked: &Asked, scenes: &[String]) -> Result<Row, Refusal> {
    if let Some(stranger) = asked.order.iter().find(|scene| !scenes.contains(scene)) {
        return Err(Refusal::Foreign(stranger.clone()));
    }
    if let Some(left_out) = scenes.iter().find(|scene| !asked.order.contains(scene)) {
        return Err(Refusal::Omitted(left_out.clone()));
    }
    Ok(Row::from([(
        SCENE_POSITION.to_owned(),
        Value::from(asked.order.clone()),
    )]))
}
