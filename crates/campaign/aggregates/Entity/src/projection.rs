//! An element's write, carried into the product's tables in the transaction
//! that applies it (`ProjectPg`).
//!
//! A save of an element (`campaign.saveEntity`, `campaign.restoreEntityVersion`)
//! writes its partition, `Entity/<its id>`, whole: [`Tables`] upserts its
//! `entity` row and its sheet — or drops the sheet of an element made a place
//! or an event — makes its `entity_alias` rows its aliases, and adds to
//! `entity_version` the version it carries, settled after the element's last
//! one there ([`crate::version::settled`]). It then records the element on
//! the note proposals that created it (`note_proposal`, NoteImport's table,
//! when the tables hold it): the element is campaign's, as the proposals are.
//! The tables' CHECKs, foreign keys and triggers judge the write a second
//! time, at once: one they refuse rolls the apply back and rejects the
//! command, so the element's terms and versions are recorded only once the
//! tables hold them ([`crate::plugin`]'s projection observes applied writes
//! alone).
//!
//! When the store starts ([`Tables`]'s `warm`), every element the engine
//! holds whose version the tables do not — one applied before its projection
//! ran in the transaction — is projected again, each in a transaction of its
//! own; then the index of names and aliases starts from every element the
//! tables hold, the fixtures' among them, and the history from every version
//! they number. Tables not migrated yet hold none.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::types::ToSql;
use harness_dataguard::tokio_postgres::{Client, Error, Row as Record, Transaction};
use serde_json::{Map, Value};

use crate::aliases::{Aliases, Row};
use crate::history::History;
use crate::version::{Kept, settled};

/// An element's row, `$1` the row as JSON text: its `entity` row upserted by
/// its id.
const UPSERT_ENTITY: &str = "INSERT INTO entity
        (id, campaign_id, type, name, description, notes, created_at, updated_at)
    SELECT (v ->> 'entityId')::uuid, (v ->> 'entityCampaignId')::uuid, v ->> 'type',
        v ->> 'name', v ->> 'description', v ->> 'notes',
        coalesce((v ->> 'entityCreatedAt')::timestamptz, now()),
        coalesce((v ->> 'updatedAt')::timestamptz, now())
    FROM (SELECT $1::text::jsonb AS v) saved
    ON CONFLICT (id) DO UPDATE SET
        campaign_id = EXCLUDED.campaign_id,
        type = EXCLUDED.type,
        name = EXCLUDED.name,
        description = EXCLUDED.description,
        notes = EXCLUDED.notes,
        updated_at = EXCLUDED.updated_at";

/// Its sheet, when the row holds one, upserted by the element's id.
const UPSERT_CHARACTER: &str = "INSERT INTO character
        (entity_id, player_id, initial_prompt, background, behavior,
         strength, dexterity, constitution, intelligence, wisdom, charisma,
         armor_class, hp_max, spells, actions, portrait_media_id, token_media_id,
         token_crop, token_color, dndbeyond_id, dndbeyond_synced_at)
    SELECT (v ->> 'characterEntityId')::uuid, (v ->> 'playerId')::uuid,
        v ->> 'initialPrompt', v ->> 'background', v ->> 'behavior',
        (v ->> 'strength')::numeric::smallint, (v ->> 'dexterity')::numeric::smallint,
        (v ->> 'constitution')::numeric::smallint, (v ->> 'intelligence')::numeric::smallint,
        (v ->> 'wisdom')::numeric::smallint, (v ->> 'charisma')::numeric::smallint,
        (v ->> 'armorClass')::numeric::smallint, (v ->> 'hpMax')::numeric::smallint,
        coalesce(nullif(v -> 'spells', 'null'), '[]'), coalesce(nullif(v -> 'actions', 'null'), '[]'),
        (v ->> 'portraitMediaId')::uuid, (v ->> 'tokenMediaId')::uuid,
        nullif(v -> 'tokenCrop', 'null'), v ->> 'tokenColor',
        v ->> 'dndbeyondId', (v ->> 'dndbeyondSyncedAt')::timestamptz
    FROM (SELECT $1::text::jsonb AS v) saved
    WHERE v ->> 'characterEntityId' IS NOT NULL
    ON CONFLICT (entity_id) DO UPDATE SET
        player_id = EXCLUDED.player_id,
        initial_prompt = EXCLUDED.initial_prompt,
        background = EXCLUDED.background,
        behavior = EXCLUDED.behavior,
        strength = EXCLUDED.strength,
        dexterity = EXCLUDED.dexterity,
        constitution = EXCLUDED.constitution,
        intelligence = EXCLUDED.intelligence,
        wisdom = EXCLUDED.wisdom,
        charisma = EXCLUDED.charisma,
        armor_class = EXCLUDED.armor_class,
        hp_max = EXCLUDED.hp_max,
        spells = EXCLUDED.spells,
        actions = EXCLUDED.actions,
        portrait_media_id = EXCLUDED.portrait_media_id,
        token_media_id = EXCLUDED.token_media_id,
        token_crop = EXCLUDED.token_crop,
        token_color = EXCLUDED.token_color,
        dndbeyond_id = EXCLUDED.dndbeyond_id,
        dndbeyond_synced_at = EXCLUDED.dndbeyond_synced_at";

/// Its sheet, when it holds none: a place or an event has no sheet.
const DROP_CHARACTER: &str = "DELETE FROM character
    WHERE entity_id = ($1::text::jsonb ->> 'entityId')::uuid
        AND $1::text::jsonb ->> 'characterEntityId' IS NULL";

/// The aliases it no longer has, gone.
const DROP_ALIASES: &str = "DELETE FROM entity_alias
    WHERE entity_id = ($1::text::jsonb ->> 'entityId')::uuid
        AND alias <> ALL (ARRAY(SELECT jsonb_array_elements_text(
            coalesce(nullif($1::text::jsonb -> 'alias', 'null'), '[]'))))";

/// The aliases it did not have yet, added.
const ADD_ALIASES: &str = "INSERT INTO entity_alias (entity_id, alias)
    SELECT DISTINCT (v ->> 'entityId')::uuid, given.term
    FROM (SELECT $1::text::jsonb AS v) saved,
        jsonb_array_elements_text(coalesce(nullif(v -> 'alias', 'null'), '[]')) AS given (term)
    WHERE NOT EXISTS (SELECT 1 FROM entity_alias a
        WHERE a.entity_id = (v ->> 'entityId')::uuid AND a.alias = given.term)";

/// The version the row carries, as it was settled, unless the table holds it
/// already: a version written is never written again.
const ADD_VERSION: &str = "INSERT INTO entity_version
        (id, entity_id, version_number, source, snapshot, diff, restored_from_version_id, created_at)
    SELECT (v ->> 'entityVersionId')::uuid, (v ->> 'entityId')::uuid,
        (v ->> 'versionNumber')::integer,
        v ->> 'source', v -> 'snapshot', v -> 'diff', (v ->> 'restoredFromVersionId')::uuid,
        coalesce((v ->> 'entityVersionCreatedAt')::timestamptz, now())
    FROM (SELECT $1::text::jsonb AS v) saved
    WHERE NOT EXISTS (SELECT 1 FROM entity_version
        WHERE id = (v ->> 'entityVersionId')::uuid)";

/// The last version the table holds of the element `$1` names: its id, its
/// number, its snapshot.
const LAST_VERSION: &str = "SELECT id::text, version_number, snapshot::text
    FROM entity_version WHERE entity_id = ($1::text)::uuid
    ORDER BY version_number DESC LIMIT 1";

/// The checked state and the created element the engine holds of each
/// proposal that created the element `$1` names, where they differ from
/// the table's (`campaign.createEntitiesFromNote` applied before the element
/// was): the element recorded once the tables hold it, never in place of
/// another, nor on a proposal unchecked (`no-creation-before-validation`).
const STAMP_PROPOSALS: &str = "UPDATE note_proposal p
    SET checked = s.checked,
        created_entity_id = coalesce(e.id, p.created_entity_id)
    FROM (SELECT r.partition, lower(r.fields ->> 'createdEntityId') AS created,
            CASE r.fields ->> 'checked' WHEN 'true' THEN true WHEN 'false' THEN false END AS checked
        FROM dataguard_rows r
        WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned) s
    LEFT JOIN entity e ON e.id::text = s.created
    WHERE s.partition = 'NoteImport/' || p.id::text AND s.checked IS NOT NULL
        AND s.created = lower($1::text)
        AND (p.created_entity_id IS NULL OR e.id IS NULL OR e.id = p.created_entity_id)
        AND (s.checked OR coalesce(e.id, p.created_entity_id) IS NULL)
        AND (p.checked <> s.checked
            OR p.created_entity_id IS DISTINCT FROM coalesce(e.id, p.created_entity_id))";

/// Whether the relation `$1` names exists: a table not migrated yet does not.
const EXISTS: &str = "SELECT to_regclass($1::text) IS NOT NULL";

/// Every live element the engine holds whose version the tables do not, with
/// its row as JSON text. A row with no version is none the tables could hold.
const UNPROJECTED: &str = "SELECT r.partition, r.fields::text FROM dataguard_rows r
    WHERE r.partition LIKE 'Entity/%' AND NOT r.tombstoned
        AND r.fields ->> 'entityVersionId' IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM entity_version v
            WHERE v.id::text = lower(r.fields ->> 'entityVersionId'))
    ORDER BY r.partition";

/// Every element of the tables with its campaign, name and aliases, as the
/// engine's row holds them: what [`Aliases`] starts from.
const ELEMENTS: &str = "SELECT jsonb_build_object(
        'entityId', e.id, 'entityCampaignId', e.campaign_id, 'name', e.name,
        'alias', coalesce((SELECT jsonb_agg(a.alias) FROM entity_alias a WHERE a.entity_id = e.id), '[]')
    )::text
    FROM entity e";

/// Every version the tables hold, oldest first: what [`History`] starts
/// from.
const VERSIONS: &str =
    "SELECT entity_id::text, id::text, version_number, snapshot::text, diff::text
    FROM entity_version
    ORDER BY entity_id, version_number";

/// The Entity aggregate's projection into the product's tables, keeping the
/// index and the history it starts from.
pub struct Tables {
    /// The elements' terms, which a start reads from the tables.
    pub aliases: Arc<Aliases>,
    /// The elements' versions, which a start reads from the tables.
    pub history: Arc<History>,
}

/// What a write is projected through: the apply's transaction, or — at a
/// start — the client, inside a transaction it opened.
enum Link<'a> {
    Transaction(&'a Transaction<'a>),
    Client(&'a Client),
}

impl Link<'_> {
    async fn execute(&self, sql: &str, parameters: &[&(dyn ToSql + Sync)]) -> Result<u64, Error> {
        match self {
            Self::Transaction(tx) => tx.execute(sql, parameters).await,
            Self::Client(client) => client.execute(sql, parameters).await,
        }
    }

    async fn query_opt(
        &self,
        sql: &str,
        parameters: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Record>, Error> {
        match self {
            Self::Transaction(tx) => tx.query_opt(sql, parameters).await,
            Self::Client(client) => client.query_opt(sql, parameters).await,
        }
    }

    async fn batch_execute(&self, sql: &str) -> Result<(), Error> {
        match self {
            Self::Transaction(tx) => tx.batch_execute(sql).await,
            Self::Client(client) => client.batch_execute(sql).await,
        }
    }

    /// Whether the relation `name` exists.
    async fn holds(&self, name: &str) -> Result<bool, Error> {
        Ok(self
            .query_opt(EXISTS, &[&name])
            .await?
            .is_some_and(|row| row.get::<_, bool>(0)))
    }
}

/// `row` carrying the version it adds, settled after `element`'s last one in
/// the table — or as it is when it carries none. One the table holds
/// already is never written again ([`ADD_VERSION`]).
async fn settled_in(link: &Link<'_>, element: &str, row: &Row) -> Result<Row, Error> {
    let last = link
        .query_opt(LAST_VERSION, &[&element])
        .await?
        .and_then(|last| {
            let Ok(Value::Object(snapshot)) = serde_json::from_str(last.get::<_, &str>(2)) else {
                return None;
            };
            Some(Kept {
                id: last.get(0),
                number: u64::try_from(last.get::<_, i32>(1)).ok()?,
                snapshot,
                diff: Vec::new(),
            })
        });
    Ok(settled(last.as_ref(), row).map_or_else(|| row.clone(), |version| version.stamped(row)))
}

/// Writes `element`'s row, its sheet, its aliases and its settled version,
/// records it on the proposals that created it, then has the tables judge it
/// at once.
async fn write(link: &Link<'_>, element: &str, row: &Row) -> Result<(), Error> {
    let settled = settled_in(link, element, row).await?;
    let json = Value::Object(settled.into_iter().collect::<Map<_, _>>()).to_string();
    for sql in [
        UPSERT_ENTITY,
        UPSERT_CHARACTER,
        DROP_CHARACTER,
        DROP_ALIASES,
        ADD_ALIASES,
        ADD_VERSION,
    ] {
        link.execute(sql, &[&json]).await?;
    }
    if link.holds("note_proposal").await? {
        link.execute(STAMP_PROPOSALS, &[&element]).await?;
    }
    // The deferred triggers judge now: a refusal is the command's, not the
    // commit's.
    link.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await
}

/// A database error the write is to blame for — a value no column holds
/// (SQLSTATE class 22), a constraint or a trigger it breaks (class 23) — is
/// the tables refusing it; any other is the store failing.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db)
            if ["22", "23"]
                .iter()
                .any(|class| db.code().code().starts_with(class)) =>
        {
            ProjectionError::Refused(format!(
                "the tables refuse it: {} ({})",
                db.message(),
                db.code().code()
            ))
        }
        _ => ProjectionError::from(error),
    }
}

#[async_trait]
impl ProjectPg for Tables {
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        // A removal is the engine's tombstone: the tables keep the element.
        let Some(element) = write.partition.strip_prefix("Entity/") else {
            return Ok(());
        };
        if write.tombstone {
            return Ok(());
        }
        self::write(&Link::Transaction(tx), element, &write.values)
            .await
            .map_err(judged)
    }

    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        let link = Link::Client(client);
        if !link.holds("entity_version").await? {
            return Ok(());
        }
        for unprojected in client.query(UNPROJECTED, &[]).await? {
            let partition: &str = unprojected.get(0);
            let Ok(Value::Object(row)) = serde_json::from_str(unprojected.get::<_, &str>(1)) else {
                continue;
            };
            let row: Row = row.into_iter().collect();
            let element = partition.trim_start_matches("Entity/");
            client.batch_execute("BEGIN").await?;
            match self::write(&link, element, &row).await {
                Ok(()) => client.batch_execute("COMMIT").await?,
                Err(e) => {
                    client.batch_execute("ROLLBACK").await?;
                    // Its SQLSTATE alone: a value may be the DM's.
                    eprintln!(
                        "projection: {partition} not projected: {}",
                        e.as_db_error().map_or("-", |db| db.code().code())
                    );
                }
            }
        }
        for element in client.query(ELEMENTS, &[]).await? {
            let Ok(Value::Object(element)) = serde_json::from_str(element.get::<_, &str>(0)) else {
                continue;
            };
            let element: Row = element.into_iter().collect();
            if let Some(id) = element.get("entityId").and_then(Value::as_str) {
                self.aliases.record(id, &element);
            }
        }
        let mut versions: BTreeMap<String, Vec<Kept>> = BTreeMap::new();
        for version in client.query(VERSIONS, &[]).await? {
            let json = |i: usize| serde_json::from_str::<Value>(version.get::<_, &str>(i)).ok();
            let (Some(Value::Object(snapshot)), Some(Value::Array(diff))) = (json(3), json(4))
            else {
                continue;
            };
            let Ok(number) = u64::try_from(version.get::<_, i32>(2)) else {
                continue;
            };
            versions.entry(version.get(0)).or_default().push(Kept {
                id: version.get(1),
                number,
                snapshot,
                diff,
            });
        }
        for (element, versions) in versions {
            self.history.replace(&element, versions);
        }
        Ok(())
    }
}
