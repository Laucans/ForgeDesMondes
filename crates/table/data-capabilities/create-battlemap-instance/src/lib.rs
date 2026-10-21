//! DataCapability `table.createBattlemapInstance` (`data-capability.json`):
//! from a scene, the DM opens a battlemap of the campaign as that scene's own
//! working copy — its instance, which carries what happens during play while
//! the original battlemap and its grid stay untouched for the next scene
//! (`instance-isolated`).
//!
//! The engine writes one partition per command: a creation is one insert on
//! `BattlemapInstance/<the instance's id>`, carrying the instance's id, its
//! battlemap, its session, its scene and when it was created — nothing of the
//! battlemap itself. The instance's id is derived from the scene's
//! ([`instance_id`]): a scene has one partition, so a second creation for it
//! lands where the first did, and the scene is the command's idempotency key.
//!
//! What a creation names is judged before it is queued ([`Placement`]), the
//! engine judging a row alone: the session, the scene and the battlemap exist
//! in the tables, the scene is the session's, the battlemap is a battlemap of
//! the session's campaign, and the scene has no instance yet — neither in the
//! tables nor on its partition, applied or queued (`one-instance-per-scene`).
//! [`prepare`] turns the DM's request into the row written, or into the
//! [`Refusal`] that says why it is not.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.createBattlemapInstance";

/// The aggregate an instance is a row of.
const AGGREGATE: &str = "BattlemapInstance";

/// The namespace every instance's id is derived in, from its scene's. It
/// never changes: an instance created before keeps its id.
const NAMESPACE: Uuid = Uuid::from_u128(0x6f1c_2b7e_4a53_5d0e_9b8a_3c41_d2e5_f607);

/// What the tables hold of the session `$1`, the scene `$2` and the
/// battlemap `$3`: the session's campaign, the scene's session, the
/// battlemap's campaign — a media of another kind is no battlemap — and the
/// scene's instance, each null when there is none or the engine tombstoned
/// its partition; and the engine's clock `$4` (milliseconds since the epoch)
/// as the table writes a time.
pub const PLACEMENT: &str = "WITH gone AS (SELECT partition FROM dataguard_rows WHERE tombstoned)
    SELECT
        (SELECT campaign_id::text FROM session WHERE id = $1::text::uuid
            AND 'Session/' || id::text NOT IN (SELECT partition FROM gone)) AS session_campaign,
        (SELECT session_id::text FROM scene WHERE id = $2::text::uuid
            AND 'Session/' || id::text NOT IN (SELECT partition FROM gone)) AS scene_session,
        (SELECT campaign_id::text FROM media WHERE id = $3::text::uuid AND kind = 'battlemap'
            AND 'Media/' || id::text NOT IN (SELECT partition FROM gone)) AS battlemap_campaign,
        (SELECT id::text FROM battlemap_instance WHERE scene_id = $2::text::uuid
            AND 'BattlemapInstance/' || id::text NOT IN (SELECT partition FROM gone)
            LIMIT 1) AS instance,
        to_timestamp($4::int8 / 1000.0) AS now";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a creation names: three ids, lowercase and hyphenated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The session the scene belongs to.
    pub session: String,
    /// The scene given its instance.
    pub scene: String,
    /// The battlemap the instance is a copy of.
    pub battlemap: String,
}

/// What the tables and the engine say of what a creation names — each
/// `None` when they hold no such row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// The campaign of the session.
    pub session_campaign: Option<String>,
    /// The session of the scene.
    pub scene_session: Option<String>,
    /// The campaign of the battlemap, when the media is a battlemap.
    pub battlemap_campaign: Option<String>,
    /// The instance the scene has already, applied or queued.
    pub instance: Option<String>,
}

/// Why a creation is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field the request names is not an id: `session`, `scene` or
    /// `battlemap`.
    NotAnId(&'static str),
    /// The command names a target other than the scene's instance.
    NotTheScenesInstance,
    /// The tables hold no such session.
    UnknownSession,
    /// The tables hold no such scene.
    UnknownScene,
    /// The tables hold no battlemap of this id.
    UnknownBattlemap,
    /// The scene is another session's.
    SceneOfAnotherSession,
    /// The battlemap is another campaign's than the session's.
    BattlemapOfAnotherCampaign,
    /// The scene has an instance already (`one-instance-per-scene`).
    SceneHasAnInstance,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(
            self,
            Self::UnknownSession | Self::UnknownScene | Self::UnknownBattlemap
        )
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotTheScenesInstance => {
                f.write_str("la cible n'est pas l'instance de cette scène")
            }
            Self::UnknownSession => f.write_str("cette séance n'existe pas"),
            Self::UnknownScene => f.write_str("cette scène n'existe pas"),
            Self::UnknownBattlemap => f.write_str("cette battlemap n'existe pas"),
            Self::SceneOfAnotherSession => f.write_str("cette scène n'est pas de cette séance"),
            Self::BattlemapOfAnotherCampaign => {
                f.write_str("cette battlemap n'est pas de la campagne de la séance")
            }
            Self::SceneHasAnInstance => {
                f.write_str("one-instance-per-scene: cette scène a déjà son instance de battlemap")
            }
        }
    }
}

/// What a creation becomes before it is submitted: the partition it writes
/// and the row it writes there.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The instance's id: the command's target.
    pub target_id: String,
    /// The fields written.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Creates)
}

/// The id of the instance of the scene `scene` — lowercase and hyphenated,
/// as [`request`] gives it.
#[must_use]
pub fn instance_id(scene: &str) -> String {
    Uuid::new_v5(&NAMESPACE, format!("battlemap_instance:{scene}").as_bytes()).to_string()
}

/// The ids `wire` names, in the form the tables write them.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for the first of `session`, `scene` and `battlemap`
/// that is absent or no uuid.
pub fn request(wire: &Row) -> Result<Request, Refusal> {
    let id = |field: &'static str| {
        wire.get(field)
            .and_then(Value::as_str)
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
            .map(|id| id.to_string())
            .ok_or(Refusal::NotAnId(field))
    };
    Ok(Request {
        session: id("session")?,
        scene: id("scene")?,
        battlemap: id("battlemap")?,
    })
}

/// The row creating the instance `request` asks for, `placement` being what
/// the tables and the engine say of what it names, `now` the time (RFC 3339).
///
/// # Errors
///
/// [`Refusal`] for a session, a scene or a battlemap the tables do not hold,
/// a scene of another session, a battlemap of another campaign, or a scene
/// that has its instance already.
pub fn prepare(request: &Request, placement: &Placement, now: &str) -> Result<Prepared, Refusal> {
    let campaign = placement
        .session_campaign
        .as_deref()
        .ok_or(Refusal::UnknownSession)?;
    let session = placement
        .scene_session
        .as_deref()
        .ok_or(Refusal::UnknownScene)?;
    let battlemap_campaign = placement
        .battlemap_campaign
        .as_deref()
        .ok_or(Refusal::UnknownBattlemap)?;
    if !session.eq_ignore_ascii_case(&request.session) {
        return Err(Refusal::SceneOfAnotherSession);
    }
    if !battlemap_campaign.eq_ignore_ascii_case(campaign) {
        return Err(Refusal::BattlemapOfAnotherCampaign);
    }
    if placement.instance.is_some() {
        return Err(Refusal::SceneHasAnInstance);
    }
    let id = instance_id(&request.scene);
    let payload = [
        ("battlemapInstanceId", id.as_str()),
        ("battlemapMediaId", &request.battlemap),
        ("sessionId", &request.session),
        ("sceneId", &request.scene),
        ("createdAt", now),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_owned(), Value::from(value)))
    .collect();
    Ok(Prepared {
        target_id: id,
        payload,
    })
}

/// A creation made the instance it writes on its scene's partition
/// ([`prepare`]), judged against the tables ([`PLACEMENT`]) and the
/// partition as the queue leaves it: one naming no id is refused, 422; one
/// naming a session, a scene or a battlemap that is not there, 404; a scene
/// of another session, a battlemap of another campaign, or a scene that has
/// its instance, 422 — nothing queued.
struct Creates;

#[async_trait]
impl Prepare for Creates {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload).map_err(refused)?;
        let id = instance_id(&request.scene);
        if target_id.is_some_and(|target| !target.trim().eq_ignore_ascii_case(&id)) {
            return Err(refused(Refusal::NotTheScenesInstance));
        }
        // Over memory the tables hold nothing: no session is found.
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::UnknownSession));
        };
        let held = rows
            .rows(
                PLACEMENT,
                &[
                    json!(request.session),
                    json!(request.scene),
                    json!(request.battlemap),
                    json!(context.now_millis),
                ],
            )
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        let text = |column: &str| held[0][column].as_str().map(str::to_owned);
        let queued = context.row(&format!("{AGGREGATE}/{id}")).await?;
        let placement = Placement {
            session_campaign: text("session_campaign"),
            scene_session: text("scene_session"),
            battlemap_campaign: text("battlemap_campaign"),
            instance: text("instance").or_else(|| {
                queued
                    .get("sceneId")
                    .is_some_and(|scene| !scene.is_null())
                    .then(|| id.clone())
            }),
        };
        let now = text("now")
            .ok_or_else(|| plugins::Refusal::unprocessable("l'heure n'a pas pu être lue"))?;
        let prepared = prepare(&request, &placement, &now).map_err(refused)?;
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            ..plugins::Prepared::default()
        })
    }
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
