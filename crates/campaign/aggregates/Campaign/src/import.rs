//! An import's copy in the product's tables: what `campaign.importCampaign`
//! wrote on the partition `Campaign/<the copy's id>` — the campaign's own
//! columns as fields (`id`, `name`, `kind`, `pitch`, `art`, `glyph`,
//! `createdAt`, `importedAt`, `originCampaignId`, `soundMasterVolume`, and
//! `coverMediaId`) and the sections of the copy (CampaignArchive@1) as the
//! archive spells them: `players`, `elements` (each with its `character`,
//! `aliases` and `versions`), `media` (with its `battlemap_grid`), `sounds`
//! and `sessions` (with `session_present_character`, `scenes` — each with
//! `scene_entity` and `scene_media` — and `journal_lines`), rows under their
//! columns' names, ids already new. A section the row lacks is an empty
//! list.
//!
//! [`copy`] writes them all, once, in the order the foreign keys ask: the
//! campaign without its cover, its players, its elements, their media (a
//! medium's place is an element), the sheets, aliases and versions, the
//! grids, the cover, then the sounds, the sessions and what hangs from
//! them. A campaign the table holds already is never written over, and
//! nothing else of the copy is written for it. The deferred constraints of
//! the tables (a battlemap and its grid, an element and its version, a PJ's
//! player) are judged before it returns, so that a row they refuse is a
//! refusal of this step and not of the commit after it.
//!
//! The campaign's `last_exported_at` is never written: a copy has never
//! been exported.

use harness_dataguard::tokio_postgres::{Error, GenericClient};
use serde_json::Value;

/// The partitions an import wrote, live, that the `campaign` table does not
/// hold — the one `$1` names, every one when it is null: a row under a uuid
/// naming its own partition, which the engine's queue says an import was
/// submitted on. Each with its fields.
pub(crate) const UNIMPORTED: &str = "SELECT r.partition, r.fields FROM dataguard_rows r
    WHERE r.partition LIKE 'Campaign/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'id', 'uuid')
        AND r.partition = 'Campaign/' || lower(r.fields ->> 'id')
        AND EXISTS (SELECT 1 FROM dataguard_queue q
            WHERE q.partition = r.partition AND q.data_capability = 'campaign.importCampaign')
        AND NOT EXISTS (SELECT 1 FROM campaign c WHERE 'Campaign/' || c.id::text = r.partition)
    ORDER BY r.partition";

/// The campaign, `$1` the row: inserted if the table holds none under its
/// id, and then only, without its cover (a medium the table does not hold
/// yet) and never exported.
const CAMPAIGN: &str = "INSERT INTO campaign
        (id, name, kind, pitch, art, glyph, created_at, imported_at, origin_campaign_id,
         sound_master_volume)
    SELECT c.id, c.name, c.kind, c.pitch, c.art, c.glyph, c.\"createdAt\", c.\"importedAt\",
        c.\"originCampaignId\", c.\"soundMasterVolume\"
    FROM jsonb_to_record($1::jsonb) AS c(id uuid, name text, kind text, pitch text, art text,
        glyph text, \"createdAt\" timestamptz, \"importedAt\" timestamptz,
        \"originCampaignId\" uuid, \"soundMasterVolume\" numeric)
    ON CONFLICT (id) DO NOTHING";

/// The cover of the campaign just inserted, `$1` the row, once the media
/// are there.
const COVER: &str = "UPDATE campaign SET cover_media_id = c.\"coverMediaId\"
    FROM jsonb_to_record($1::jsonb) AS c(id uuid, \"coverMediaId\" uuid)
    WHERE campaign.id = c.id AND c.\"coverMediaId\" IS NOT NULL";

/// The sections' statements read their rows from `$1`, the section of the
/// row they are named for.
const PLAYERS: &str = "INSERT INTO player (id, campaign_id, name, notes, created_at)
    SELECT p.id, p.campaign_id, p.name, p.notes, p.created_at
    FROM jsonb_to_recordset($1::jsonb) AS p(id uuid, campaign_id uuid, name text, notes text,
        created_at timestamptz)";

const ENTITIES: &str = "INSERT INTO entity
        (id, campaign_id, type, name, description, notes, created_at, updated_at)
    SELECT e.id, e.campaign_id, e.type, e.name, e.description, e.notes, e.created_at, e.updated_at
    FROM jsonb_to_recordset($1::jsonb) AS e(id uuid, campaign_id uuid, type text, name text,
        description text, notes text, created_at timestamptz, updated_at timestamptz)";

const MEDIA: &str = "INSERT INTO media
        (id, campaign_id, kind, title, file_path, width_px, height_px, place_id, uploaded_at)
    SELECT m.id, m.campaign_id, m.kind, m.title, m.file_path, m.width_px, m.height_px, m.place_id,
        m.uploaded_at
    FROM jsonb_to_recordset($1::jsonb) AS m(id uuid, campaign_id uuid, kind text, title text,
        file_path text, width_px integer, height_px integer, place_id uuid,
        uploaded_at timestamptz)";

/// The elements' sheets, `$1` the elements: an element with none has a null
/// `character`.
const CHARACTERS: &str = "INSERT INTO character
        (entity_id, player_id, initial_prompt, background, behavior, strength, dexterity,
         constitution, intelligence, wisdom, charisma, armor_class, hp_max, spells, actions,
         portrait_media_id, token_media_id, token_crop, token_color, dndbeyond_id,
         dndbeyond_synced_at)
    SELECT c.entity_id, c.player_id, c.initial_prompt, c.background, c.behavior, c.strength,
        c.dexterity, c.constitution, c.intelligence, c.wisdom, c.charisma, c.armor_class, c.hp_max,
        c.spells, c.actions, c.portrait_media_id, c.token_media_id, c.token_crop, c.token_color,
        c.dndbeyond_id, c.dndbeyond_synced_at
    FROM jsonb_to_recordset($1::jsonb) AS e(\"character\" jsonb),
        jsonb_to_record(e.\"character\") AS c(entity_id uuid, player_id uuid, initial_prompt text,
            background text, behavior text, strength smallint, dexterity smallint,
            constitution smallint, intelligence smallint, wisdom smallint, charisma smallint,
            armor_class smallint, hp_max smallint, spells jsonb, actions jsonb,
            portrait_media_id uuid, token_media_id uuid, token_crop jsonb, token_color text,
            dndbeyond_id text, dndbeyond_synced_at timestamptz)
    WHERE e.\"character\" IS NOT NULL";

/// The elements' aliases, `$1` the elements: texts, each given an id by the
/// table.
const ALIASES: &str = "INSERT INTO entity_alias (entity_id, alias)
    SELECT e.id, a.alias
    FROM jsonb_to_recordset($1::jsonb) AS e(id uuid, aliases jsonb),
        jsonb_array_elements_text(coalesce(nullif(e.aliases, 'null'), '[]')) AS a(alias)";

/// The elements' versions, `$1` the elements, in the order they were made:
/// the table numbers them one after the other.
const VERSIONS: &str = "INSERT INTO entity_version
        (id, entity_id, version_number, source, snapshot, diff, restored_from_version_id,
         created_at)
    SELECT v.id, v.entity_id, v.version_number, v.source, v.snapshot, v.diff,
        v.restored_from_version_id, v.created_at
    FROM jsonb_to_recordset($1::jsonb) AS e(versions jsonb),
        jsonb_to_recordset(coalesce(nullif(e.versions, 'null'), '[]')) AS v(id uuid,
            entity_id uuid, version_number integer, source text, snapshot jsonb, diff jsonb,
            restored_from_version_id uuid, created_at timestamptz)
    ORDER BY v.entity_id, v.version_number";

/// The media's grids, `$1` the media: a medium with none has a null
/// `battlemap_grid`. `feet_per_hex` is the table's, never written.
const GRIDS: &str = "INSERT INTO battlemap_grid
        (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
    SELECT g.media_id, g.hex_size_px, g.offset_x, g.offset_y, g.orientation, g.color, g.opacity
    FROM jsonb_to_recordset($1::jsonb) AS m(battlemap_grid jsonb),
        jsonb_to_record(m.battlemap_grid) AS g(media_id uuid, hex_size_px numeric,
            offset_x numeric, offset_y numeric, orientation text, color text, opacity numeric)
    WHERE m.battlemap_grid IS NOT NULL";

const SOUNDS: &str =
    "INSERT INTO sound (id, campaign_id, name, file_path, tags, volume, created_at)
    SELECT s.id, s.campaign_id, s.name, s.file_path,
        ARRAY(SELECT jsonb_array_elements_text(s.tags)), s.volume, s.created_at
    FROM jsonb_to_recordset($1::jsonb) AS s(id uuid, campaign_id uuid, name text, file_path text,
        tags jsonb, volume numeric, created_at timestamptz)";

const SESSIONS: &str = "INSERT INTO session
        (id, campaign_id, position, title, planned_on, status, started_at, closed_at,
         microphone_on, summary, summary_model, summary_edited_at)
    SELECT s.id, s.campaign_id, s.position, s.title, s.planned_on, s.status, s.started_at,
        s.closed_at, s.microphone_on, s.summary, s.summary_model, s.summary_edited_at
    FROM jsonb_to_recordset($1::jsonb) AS s(id uuid, campaign_id uuid, position integer,
        title text, planned_on date, status text, started_at timestamptz, closed_at timestamptz,
        microphone_on boolean, summary text, summary_model text, summary_edited_at timestamptz)";

const PRESENT: &str = "INSERT INTO session_present_character (session_id, character_id, added_at)
    SELECT p.session_id, p.character_id, p.added_at
    FROM jsonb_to_recordset($1::jsonb) AS s(session_present_character jsonb),
        jsonb_to_recordset(coalesce(nullif(s.session_present_character, 'null'), '[]'))
            AS p(session_id uuid, character_id uuid, added_at timestamptz)";

/// The scenes of the sessions, then what each links.
const SCENES: &str = "INSERT INTO scene (id, session_id, position, title, notes)
    SELECT c.id, c.session_id, c.position, c.title, c.notes
    FROM jsonb_to_recordset($1::jsonb) AS s(scenes jsonb),
        jsonb_to_recordset(coalesce(nullif(s.scenes, 'null'), '[]')) AS c(id uuid,
            session_id uuid, position integer, title text, notes text)";

const SCENE_ENTITIES: &str = "INSERT INTO scene_entity (scene_id, entity_id, position)
    SELECT l.scene_id, l.entity_id, l.position
    FROM jsonb_to_recordset($1::jsonb) AS s(scenes jsonb),
        jsonb_to_recordset(coalesce(nullif(s.scenes, 'null'), '[]')) AS c(scene_entity jsonb),
        jsonb_to_recordset(coalesce(nullif(c.scene_entity, 'null'), '[]')) AS l(scene_id uuid,
            entity_id uuid, position integer)";

const SCENE_MEDIA: &str = "INSERT INTO scene_media (scene_id, media_id)
    SELECT l.scene_id, l.media_id
    FROM jsonb_to_recordset($1::jsonb) AS s(scenes jsonb),
        jsonb_to_recordset(coalesce(nullif(s.scenes, 'null'), '[]')) AS c(scene_media jsonb),
        jsonb_to_recordset(coalesce(nullif(c.scene_media, 'null'), '[]')) AS l(scene_id uuid,
            media_id uuid)";

const JOURNAL: &str = "INSERT INTO session_journal_line (id, session_id, scene_id, spoken_at, text)
    SELECT j.id, j.session_id, j.scene_id, j.spoken_at, j.text
    FROM jsonb_to_recordset($1::jsonb) AS s(journal_lines jsonb),
        jsonb_to_recordset(coalesce(nullif(s.journal_lines, 'null'), '[]')) AS j(id uuid,
            session_id uuid, scene_id uuid, spoken_at timestamptz, text text)";

/// What the sections' statements read, in the order the foreign keys ask:
/// the section of the row each reads, then its statement.
const SECTIONS: [(&str, &str); 14] = [
    ("players", PLAYERS),
    ("elements", ENTITIES),
    ("media", MEDIA),
    ("elements", CHARACTERS),
    ("elements", ALIASES),
    ("elements", VERSIONS),
    ("media", GRIDS),
    ("sounds", SOUNDS),
    ("sessions", SESSIONS),
    ("sessions", PRESENT),
    ("sessions", SCENES),
    ("sessions", SCENE_ENTITIES),
    ("sessions", SCENE_MEDIA),
    ("sessions", JOURNAL),
];

/// Deferred constraints judged now, then deferred again for the rest of the
/// transaction.
const JUDGE_NOW: &str = "SET CONSTRAINTS ALL IMMEDIATE";
const DEFER_AGAIN: &str = "SET CONSTRAINTS ALL DEFERRED";

/// A section of the row as a list: a section it lacks, or holds as null, is
/// empty. Anything else it holds is handed to PostgreSQL as it is, which
/// refuses what is no list.
fn section(row: &Value, name: &str) -> Value {
    match row.get(name) {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(other) => other.clone(),
    }
}

/// Writes the copy `row` carries — a campaign's fields and sections — into
/// the tables, once. Whatever the tables refuse is the error.
pub(crate) async fn copy(client: &impl GenericClient, row: &Value) -> Result<(), Error> {
    if client.execute(CAMPAIGN, &[row]).await? == 0 {
        return Ok(());
    }
    for (name, statement) in SECTIONS {
        client.execute(statement, &[&section(row, name)]).await?;
    }
    client.execute(COVER, &[row]).await?;
    client.batch_execute(JUDGE_NOW).await?;
    client.batch_execute(DEFER_AGAIN).await
}
