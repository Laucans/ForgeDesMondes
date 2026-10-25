//! DataCapability `campaign.exportCampaign` (`data-capability.json`): the DM
//! exports a campaign to keep a copy of it, and the home page shows when that
//! last happened. The export is an archive — the campaign, its players, its
//! elements with their sheets, their aliases and their versions, its media
//! with their battlemap grids, its sounds, its sessions with their present
//! characters, their scenes and their journal, as the tables hold them now,
//! every section `CampaignArchive@1` names — and one command on
//! `Campaign/<its id>` stamping `lastExportedAt`, which writes nothing else
//! (`export-stamps-date`).
//!
//! Its `Prepare` ([`plugin`]) reads what the tables hold of the campaign
//! ([`ARCHIVE`], over the engine's read-only SQL) and makes the archive the
//! command's attachment: the command's result answers it once the stamp is
//! applied — never before, never for an export rejected — and the shell
//! hands it to the DM as a download, `{file, content}`. [`prepare`] turns
//! what the tables hold of the campaign — one JSON document, `{campaign,
//! players, elements, media, sounds, sessions}`, each row under its columns'
//! names — into that
//! archive and the command's payload:
//!
//! - the archive is one self-contained JSON document: its [`FORMAT`] and
//!   [`FORMAT_VERSION`], the campaign's id as `origin_campaign_id` (what an
//!   import names the copy's original by), the export's time, to the
//!   millisecond, and the rows as the tables hold them — the campaign's own
//!   `last_exported_at` the one before this export;
//! - its file is named after the campaign and the time ([`file_name`]);
//! - the command targets the campaign by its id, lowercase as the table
//!   writes it — one partition per campaign whatever case the DM's request
//!   used — and its payload stamps `lastExportedAt` with the export's time,
//!   and nothing else;
//! - a campaign the tables do not hold — or none at all, over the engine's
//!   memory — is refused before anything is queued, 404; what they answered
//!   but cannot be an archive — a campaign without its id, a section that is
//!   no list — and tables that could not be read, 422
//!   ([`Refusal`]): the stamp is left as it was.
//!
//! The files a medium and a sound name are not in the archive, only their
//! rows; the import that reads it back is a DataCapability of its own.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Map, Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.exportCampaign";

/// What an archive says it is.
pub const FORMAT: &str = "campaign-archive";

/// The version of its shape: an import reads the versions it knows.
pub const FORMAT_VERSION: u64 = 1;

/// The one field an export writes: when it happened.
const STAMP: &str = "lastExportedAt";

/// The archive's lists, in the order the archive writes them.
const SECTIONS: [&str; 5] = ["players", "elements", "media", "sounds", "sessions"];

/// What a file is named after when the campaign has no name to give it.
const UNNAMED: &str = "campagne";

/// What an archive holds of the campaign `$1` names, in whatever case, as
/// the tables hold it — the campaign, its players, its elements each with
/// its sheet, its aliases and its versions, its media each with its battlemap
/// grid, its sounds, its sessions each with its present characters, its
/// scenes (with their elements and their media) and its journal — in one
/// statement, so one snapshot: each row under its columns' names, in a stable
/// order, in the one column `archive`. A section the campaign has none of is
/// an empty list. A campaign the engine deleted is none.
pub const ARCHIVE: &str = "SELECT jsonb_build_object(
        'campaign', to_jsonb(c),
        'players', coalesce((SELECT jsonb_agg(to_jsonb(p) ORDER BY p.created_at, p.id)
            FROM player p WHERE p.campaign_id = c.id), '[]'),
        'elements', coalesce((SELECT jsonb_agg(to_jsonb(e) || jsonb_build_object(
                'character', (SELECT to_jsonb(ch) FROM character ch WHERE ch.entity_id = e.id),
                'aliases', coalesce((SELECT jsonb_agg(a.alias ORDER BY a.alias, a.id)
                    FROM entity_alias a WHERE a.entity_id = e.id), '[]'),
                'versions', coalesce((SELECT jsonb_agg(to_jsonb(v) ORDER BY v.version_number)
                    FROM entity_version v WHERE v.entity_id = e.id), '[]'))
            ORDER BY e.created_at, e.id)
            FROM entity e WHERE e.campaign_id = c.id), '[]'),
        'media', coalesce((SELECT jsonb_agg(to_jsonb(m) || jsonb_build_object(
                'battlemap_grid', (SELECT to_jsonb(g) FROM battlemap_grid g WHERE g.media_id = m.id))
            ORDER BY m.uploaded_at, m.id)
            FROM media m WHERE m.campaign_id = c.id), '[]'),
        'sounds', coalesce((SELECT jsonb_agg(to_jsonb(so) ORDER BY so.created_at, so.id)
            FROM sound so WHERE so.campaign_id = c.id), '[]'),
        'sessions', coalesce((SELECT jsonb_agg(to_jsonb(s) || jsonb_build_object(
                'session_present_character', coalesce((SELECT jsonb_agg(to_jsonb(pc)
                        ORDER BY pc.added_at, pc.character_id)
                    FROM session_present_character pc WHERE pc.session_id = s.id), '[]'),
                'scenes', coalesce((SELECT jsonb_agg(to_jsonb(sc) || jsonb_build_object(
                        'scene_entity', coalesce((SELECT jsonb_agg(to_jsonb(se)
                                ORDER BY se.position, se.entity_id)
                            FROM scene_entity se WHERE se.scene_id = sc.id), '[]'),
                        'scene_media', coalesce((SELECT jsonb_agg(to_jsonb(sm) ORDER BY sm.media_id)
                            FROM scene_media sm WHERE sm.scene_id = sc.id), '[]'))
                    ORDER BY sc.position, sc.id)
                    FROM scene sc WHERE sc.session_id = s.id), '[]'),
                'journal_lines', coalesce((SELECT jsonb_agg(to_jsonb(j) ORDER BY j.spoken_at, j.id)
                    FROM session_journal_line j WHERE j.session_id = s.id), '[]'))
            ORDER BY s.position, s.id)
            FROM session s WHERE s.campaign_id = c.id), '[]')
    ) AS archive
    FROM campaign c WHERE c.id::text = lower($1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an export is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// What the tables answered holds no campaign with its id.
    NoCampaign,
    /// One of the campaign's sections (players, elements, media, sounds,
    /// sessions) is not a list: no archive holds it.
    Unreadable(&'static str),
}

impl From<Refusal> for plugins::Refusal {
    /// No campaign is a 404; the rest, 422.
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::NoCampaign => Self::not_found(refusal.to_string()),
            Refusal::Unreadable(_) => Self::unprocessable(refusal.to_string()),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("cette campagne n'existe pas"),
            Self::Unreadable(what) => {
                write!(
                    f,
                    "l'archive n'a pas pu être construite : {what} illisibles"
                )
            }
        }
    }
}

/// The archive an export answers: its file's name, and its content.
#[derive(Clone, Debug, PartialEq)]
pub struct Archive {
    /// `<the campaign's name>-<the time>.json`.
    pub file: String,
    /// The JSON document itself.
    pub content: Value,
}

impl Archive {
    /// The archive as the command's result answers it once applied:
    /// `{file, content}`.
    #[must_use]
    pub fn attachment(self) -> Value {
        json!({ "file": self.file, "content": self.content })
    }
}

/// What an export becomes before it is submitted: what the command writes
/// on `Campaign/<target_id>`, and the archive it answers once applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the stamp alone.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
    /// The archive.
    pub archive: Archive,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Exporting)
}

/// The export's `Prepare`: what the tables hold of the campaign, made the
/// stamp the command writes and the archive it answers once applied.
struct Exporting;

#[async_trait]
impl Prepare for Exporting {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let id = target_id
            .map(str::to_owned)
            .or_else(|| campaign_id(payload))
            .unwrap_or_default();
        // Over the engine's memory there is no table: no campaign is held.
        let tables = context.rows.ok_or(Refusal::NoCampaign)?;
        let rows = tables
            .rows(ARCHIVE, &[Value::from(id)])
            .await
            .map_err(|e| plugins::Refusal::unprocessable(format!("campaign not read: {e}")))?;
        let archived = rows
            .get(0)
            .and_then(|row| row.get("archive"))
            .ok_or(Refusal::NoCampaign)?;
        let prepared = prepare(archived, &timestamp(context.now_millis))?;
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            attachment: Some(prepared.archive.attachment()),
            ..plugins::Prepared::default()
        })
    }
}

/// The campaign a request names in its payload, when it names one there —
/// `id`, `campaignId` or `campaign` — rather than as the command's target.
#[must_use]
pub fn campaign_id(wire: &Row) -> Option<String> {
    ["id", "campaignId", "campaign"]
        .iter()
        .find_map(|field| wire.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// The export of the campaign `tables` holds — `{campaign, players,
/// elements, media, sounds, sessions}`, as the tables hold them — at `now`,
/// RFC 3339.
///
/// # Errors
///
/// [`Refusal`] when `tables` holds no campaign with its id, or a section that
/// is not a list.
pub fn prepare(tables: &Value, now: &str) -> Result<Prepared, Refusal> {
    let campaign = tables
        .get("campaign")
        .and_then(Value::as_object)
        .ok_or(Refusal::NoCampaign)?;
    let id = campaign
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(Refusal::NoCampaign)?
        .to_ascii_lowercase();
    let list = |field: &'static str| {
        tables
            .get(field)
            .and_then(Value::as_array)
            .cloned()
            .ok_or(Refusal::Unreadable(field))
    };
    let sections = SECTIONS.map(|field| list(field).map(|rows| (field, Value::Array(rows))));
    let name = campaign.get("name").and_then(Value::as_str).unwrap_or("");
    let mut content = Map::new();
    for (field, value) in [
        ("format", Value::from(FORMAT)),
        ("version", FORMAT_VERSION.into()),
        ("origin_campaign_id", id.clone().into()),
        ("exported_at", now.into()),
        ("campaign", Value::Object(campaign.clone())),
    ] {
        content.insert(field.into(), value);
    }
    for section in sections {
        let (field, rows) = section?;
        content.insert(field.into(), rows);
    }
    let payload = Row::from([(STAMP.into(), now.into())]);
    Ok(Prepared {
        payload,
        archive: Archive {
            file: file_name(name, now),
            content: Value::Object(content),
        },
        target_id: id,
    })
}

/// The archive's file name: the campaign's name, lowercase, every run of
/// characters that are neither letters nor digits one dash — `campagne` when
/// nothing is left — then the time to the millisecond, so two exports in a
/// second are two files, its colons and its point dashes so every file
/// system takes it: `les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json`.
#[must_use]
pub fn file_name(name: &str, now: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { UNNAMED } else { slug };
    let time = now.trim_end_matches('Z').replace([':', '.'], "-");
    format!("{slug}-{time}Z.json")
}

/// `unix_millis` as RFC 3339 in UTC, to the millisecond
/// (`2026-10-18T09:30:00.123Z`): two exports in a second are two stamps,
/// and two files.
#[must_use]
pub fn timestamp(unix_millis: u64) -> String {
    let unix_seconds = unix_millis / 1000;
    // Howard Hinnant's civil_from_days.
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or_default() + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        unix_millis % 1000
    )
}
