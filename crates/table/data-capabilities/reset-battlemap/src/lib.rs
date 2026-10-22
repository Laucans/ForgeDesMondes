//! DataCapability `table.resetBattlemap` (`data-capability.json`):
//! « Réinitialiser la carte » — the DM wipes a scene's map clean to replay
//! it, without losing the scene's instance or the original battlemap.
//!
//! The engine writes one partition per command, and an instance's tokens,
//! combats and initiative entries are rows of their own,
//! `BattlemapInstance/<their id>`: the reset is one deletion of each, which
//! the engine keeps as a tombstone (a soft delete, reversible through the
//! command journal). The first is the command itself, the others its
//! follow-ups ([`plugins::Prepared::then`]), entries first, then combats,
//! then tokens — a child before what it belongs to, as the aggregate's
//! relations cascade. They are not atomic: a follow-up refused leaves the
//! ones after it unsent, and a second reset takes what is left. The instance
//! row is never written — one command has one effect, and this one deletes
//! — nor its battlemap, nor its grid, nor any row of another instance
//! (`instance-isolated`). Each deletion carries when the reset was asked,
//! `resetAt`, and the instance it is of, `of`, into the command journal: the
//! instance's last reset (`state-persisted-reset`).
//!
//! What the DM sends names the instance, as the command's target or under
//! `instance`. [`request`] reads it; the instance's rows are listed among
//! the engine's and the tables' ([`LISTING`]), a tombstone being a row gone,
//! and [`reset`] makes the deletions:
//!
//! - a command naming no uuid is refused before it is queued ([`Refusal`]);
//! - an instance neither the engine nor the tables hold is none to reset;
//! - an instance with no token, no combat and no entry has nothing to reset.
//!
//! A follow-up names its row as its target, the instance under `of` and
//! the reset's stamp under `resetAt`; it is listed again, as any submission
//! is, and refused when the row is not the instance's, or is gone, or when
//! no reset of the instance carries that stamp — a deletion is a reset's,
//! never one sent alone (`table.removeToken` and `table.removeParticipant`
//! are those). A row still queued for insertion is listed too: its deletion
//! lands behind it.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "table.resetBattlemap";

/// The aggregate an instance, its tokens, combats and entries are rows of.
const AGGREGATE: &str = "BattlemapInstance";

/// What the instance `$1` holds, at `$2` milliseconds since the epoch:
/// whether the tables hold it, and the ids of its entries, combats and
/// tokens, each sorted, among the engine's rows, the inserts still queued
/// and the tables' — a row of the tables the engine tombstoned is gone. An
/// entry is the instance's by its combat or by its token, either one gone
/// or not. `now` is the moment, in UTC; `under_way`, whether a reset of the
/// instance stamped `$3` was submitted.
pub const LISTING: &str = "WITH held AS (
        SELECT partition, fields FROM dataguard_rows
        WHERE partition LIKE 'BattlemapInstance/%' AND NOT tombstoned
        UNION ALL
        SELECT partition, entry -> 'payload' FROM dataguard_queue
        WHERE partition LIKE 'BattlemapInstance/%'
            AND NOT (state = ANY (ARRAY['applied', 'rejected', 'cancelled', 'expired']))
            AND entry ->> 'effect' IS DISTINCT FROM 'delete'
    ),
    tokens AS (
        SELECT lower(fields ->> 'tokenId') AS id, true AS live FROM held
        WHERE partition = 'BattlemapInstance/' || lower(fields ->> 'tokenId')
            AND lower(fields ->> 'tokenInstanceId') = $1::text
        UNION ALL SELECT t.id::text, false FROM token t WHERE t.instance_id::text = $1::text
    ),
    combats AS (
        SELECT lower(fields ->> 'combatId') AS id, true AS live FROM held
        WHERE partition = 'BattlemapInstance/' || lower(fields ->> 'combatId')
            AND lower(fields ->> 'combatInstanceId') = $1::text
        UNION ALL SELECT c.id::text, false FROM combat c WHERE c.instance_id::text = $1::text
    ),
    entries AS (
        SELECT lower(fields ->> 'initiativeEntryId') AS id, true AS live FROM held
        WHERE partition = 'BattlemapInstance/' || lower(fields ->> 'initiativeEntryId')
            AND (lower(fields ->> 'initiativeEntryCombatId') IN (SELECT id FROM combats)
                OR lower(fields ->> 'initiativeEntryTokenId') IN (SELECT id FROM tokens))
        UNION ALL SELECT e.id::text, false FROM initiative_entry e
        WHERE e.combat_id::text IN (SELECT id FROM combats)
            OR e.token_id::text IN (SELECT id FROM tokens)
    ),
    standing AS (
        SELECT DISTINCT l.kind, l.id FROM (
            SELECT 'entries' AS kind, id, live FROM entries
            UNION ALL SELECT 'combats', id, live FROM combats
            UNION ALL SELECT 'tokens', id, live FROM tokens
        ) l
        WHERE l.live OR NOT EXISTS (SELECT 1 FROM dataguard_rows g
            WHERE g.partition = 'BattlemapInstance/' || l.id AND g.tombstoned)
    )
    SELECT
        EXISTS (SELECT 1 FROM battlemap_instance i WHERE i.id::text = $1::text
            AND NOT EXISTS (SELECT 1 FROM dataguard_rows g
                WHERE g.partition = 'BattlemapInstance/' || i.id::text AND g.tombstoned))
            AS instance,
        (SELECT coalesce(jsonb_agg(id ORDER BY id), '[]') FROM standing
            WHERE kind = 'entries') AS entries,
        (SELECT coalesce(jsonb_agg(id ORDER BY id), '[]') FROM standing
            WHERE kind = 'combats') AS combats,
        (SELECT coalesce(jsonb_agg(id ORDER BY id), '[]') FROM standing
            WHERE kind = 'tokens') AS tokens,
        ($3::text IS NOT NULL AND EXISTS (SELECT 1 FROM dataguard_queue
            WHERE data_capability = 'table.resetBattlemap'
                AND entry -> 'payload' ->> 'of' = $1::text
                AND entry -> 'payload' ->> 'resetAt' = $3::text)) AS under_way,
        to_char(to_timestamp($2::int8 / 1000.0) AT TIME ZONE 'UTC',
            'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS now";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What a submission asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The DM's: the map of the instance, lowercase and hyphenated, reset.
    Reset {
        /// The instance.
        instance: String,
    },
    /// A follow-up's: one row of the instance `of` tombstoned.
    Delete {
        /// The instance the reset is of, lowercase and hyphenated.
        of: String,
        /// The entry, combat or token, lowercase and hyphenated.
        row: String,
        /// When the reset was asked, as the command that made the
        /// follow-up stamped it.
        reset_at: String,
    },
}

/// What an instance holds, as [`LISTING`] reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Whether the tables hold the instance, not gone.
    pub instance: bool,
    /// Its initiative entries' ids, sorted.
    pub entries: Vec<String>,
    /// Its combats' ids, sorted.
    pub combats: Vec<String>,
    /// Its tokens' ids, sorted.
    pub tokens: Vec<String>,
    /// Whether a reset of the instance carrying the stamp asked about was
    /// submitted.
    pub under_way: bool,
    /// Now, in UTC, as the database reads it.
    pub now: Option<String>,
}

/// Why a reset, or one of its deletions, is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The instance, or the row, is not named by a uuid.
    NotAnId(&'static str),
    /// The engine and the tables hold no such instance, or it is gone.
    UnknownInstance,
    /// The instance's map holds no token, no combat, no entry.
    NothingToReset,
    /// The row is no entry, combat or token of the instance, or is gone.
    NotOfTheInstance,
    /// A deletion carries no stamp, or one no reset of the instance was
    /// submitted with: it is sent alone, not by a reset.
    NotOfAReset,
    /// The instance's rows cannot be listed: the engine runs over memory.
    Unlisted,
}

impl Refusal {
    /// Whether the refusal is of something that is not there (404) rather
    /// than of a request that cannot be done (422).
    #[must_use]
    pub fn not_found(&self) -> bool {
        matches!(self, Self::UnknownInstance | Self::NotOfTheInstance)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::UnknownInstance => f.write_str("cette instance de battlemap n'existe pas"),
            Self::NothingToReset => {
                f.write_str("state-persisted-reset: la carte n'a ni token, ni combat, ni initiative")
            }
            Self::NotOfTheInstance => f.write_str(
                "instance-isolated: cette ligne n'est pas un token, un combat ou une initiative de l'instance",
            ),
            Self::NotOfAReset => f.write_str(
                "state-persisted-reset: cette suppression n'appartient à aucune réinitialisation de l'instance",
            ),
            Self::Unlisted => f.write_str("la carte de l'instance n'a pas pu être lue"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Resets)
}

/// The partition of the row of id `id`: the instance's, or an entry's, a
/// combat's or a token's, each its own.
#[must_use]
pub fn partition(id: &str) -> String {
    format!("{AGGREGATE}/{id}")
}

/// What `wire`, sent to `target`, asks: a deletion of `target` when `wire`
/// names the instance it is of, under `of`; otherwise the reset of the
/// instance `target`, or `wire.instance` when the command names no target.
///
/// # Errors
///
/// [`Refusal::NotAnId`] when the instance or the row is not a uuid;
/// [`Refusal::NotOfAReset`] when a deletion carries no stamp.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    // An `of` of null is none, as an absent one: the DM's reset.
    if let Some(of) = wire.get("of").filter(|of| !of.is_null()) {
        return Ok(Request::Delete {
            of: of.as_str().and_then(uuid).ok_or(Refusal::NotAnId("of"))?,
            row: target.and_then(uuid).ok_or(Refusal::NotAnId("row"))?,
            reset_at: wire
                .get("resetAt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|at| !at.is_empty())
                .ok_or(Refusal::NotOfAReset)?
                .to_owned(),
        });
    }
    let instance = target
        .map(str::to_owned)
        .or_else(|| {
            wire.get("instance")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        .and_then(uuid)
        .ok_or(Refusal::NotAnId("instance"))?;
    Ok(Request::Reset { instance })
}

/// [`LISTING`]'s answer, `rows` (its rows, a JSON array), as a [`Listing`]:
/// what it cannot read is not there.
#[must_use]
pub fn listing(rows: &Value) -> Listing {
    let held = &rows[0];
    let ids = |field: &str| {
        held[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(uuid)
            .collect()
    };
    Listing {
        instance: held["instance"].as_bool().unwrap_or(false),
        entries: ids("entries"),
        combats: ids("combats"),
        tokens: ids("tokens"),
        under_way: held["under_way"].as_bool().unwrap_or(false),
        now: held["now"].as_str().map(str::to_owned),
    }
}

/// The rows a reset of `listing`'s instance tombstones, in order: its
/// entries, then its combats, then its tokens, each once.
#[must_use]
pub fn deletions(listing: &Listing) -> Vec<String> {
    let mut seen = BTreeSet::new();
    listing
        .entries
        .iter()
        .chain(&listing.combats)
        .chain(&listing.tokens)
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

/// Whether `row` is one of `listing`'s entries, combats or tokens.
#[must_use]
pub fn holds(listing: &Listing, row: &str) -> bool {
    [&listing.entries, &listing.combats, &listing.tokens]
        .iter()
        .any(|ids| ids.iter().any(|id| id == row))
}

/// What every deletion of the reset of `instance`, asked at `reset_at`,
/// writes into the command journal: the instance it is of, and when.
#[must_use]
pub fn stamp(instance: &str, reset_at: &str) -> Row {
    Row::from([
        ("of".to_owned(), json!(instance)),
        ("resetAt".to_owned(), json!(reset_at)),
    ])
}

/// The reset of `instance`, asked at `reset_at`, whose map `listing` holds:
/// the first deletion as the command itself, the others as its follow-ups,
/// each stamped ([`stamp`]).
///
/// # Errors
///
/// [`Refusal::NothingToReset`] when the instance has no entry, no combat
/// and no token.
pub fn reset(
    instance: &str,
    listing: &Listing,
    reset_at: &str,
) -> Result<plugins::Prepared, Refusal> {
    let mut rows = deletions(listing).into_iter();
    let first = rows.next().ok_or(Refusal::NothingToReset)?;
    let payload = stamp(instance, reset_at);
    Ok(plugins::Prepared {
        target_id: Some(first),
        then: rows
            .map(|row| Submission {
                data_capability: DATA_CAPABILITY.to_owned(),
                payload: payload.clone(),
                target_id: Some(row),
                ..Submission::default()
            })
            .collect(),
        payload,
        ..plugins::Prepared::default()
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// A reset made the deletions of the instance's rows ([`reset`]), against
/// the engine's rows and the tables', or one of its follow-ups made the
/// deletion of its row. One naming no id is refused, 422; one naming an
/// instance that is not there, or a row that is not the instance's, 404;
/// an instance with nothing on its map, or a deletion of no reset under
/// way, 422 — nothing queued.
struct Resets;

#[async_trait]
impl Prepare for Resets {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let (instance, stamped) = match &request {
            Request::Reset { instance } => (instance, Value::Null),
            Request::Delete { of, reset_at, .. } => (of, Value::from(reset_at.as_str())),
        };
        // Over memory there are no rows to list: no map is reset.
        let Some(rows) = context.rows else {
            return Err(refused(Refusal::Unlisted));
        };
        let listed = rows
            .rows(
                LISTING,
                &[
                    Value::from(instance.as_str()),
                    json!(context.now_millis),
                    stamped,
                ],
            )
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
        let listing = listing(&listed);
        match request {
            Request::Delete { of, row, reset_at } => {
                if !holds(&listing, &row) {
                    return Err(refused(Refusal::NotOfTheInstance));
                }
                if !listing.under_way {
                    return Err(refused(Refusal::NotOfAReset));
                }
                Ok(plugins::Prepared {
                    payload: stamp(&of, &reset_at),
                    target_id: Some(row),
                    ..plugins::Prepared::default()
                })
            }
            Request::Reset { instance } => {
                // An instance the engine wrote may be queued yet, before the
                // tables hold it.
                let held = listing.instance
                    || context
                        .row(&partition(&instance))
                        .await?
                        .get("battlemapInstanceId")
                        .and_then(Value::as_str)
                        .and_then(uuid)
                        .is_some_and(|id| id == instance);
                if !held {
                    return Err(refused(Refusal::UnknownInstance));
                }
                let now = listing.now.as_deref().ok_or_else(|| {
                    plugins::Refusal::unprocessable("l'heure n'a pas pu être lue")
                })?;
                reset(&instance, &listing, now).map_err(refused)
            }
        }
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
