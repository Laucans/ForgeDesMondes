//! Capability `campaign.listImportedNotes`: a campaign's past note imports
//! and the proposals still awaiting the DM's validation.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-imported-notes.graphql`, resolved by
//! [`SQL`]) go in; the answer, in the shape the NoteImport Micro-UI reads —
//! `{ imports, proposals }`, the imports as in
//! `mock/fixtures/campaign.listImportedNotes.json`, the proposals as in
//! `mock/fixtures/NoteImport.json` — comes out.
//!
//! The raw note is always kept, so every note is an import, analysed or not.
//! What the store does not hold is derived at read time (nothing is stored
//! twice): the `title` is the note's first line, `chars` its length,
//! `proposed` and `created` count its proposals. A proposal that recognises
//! an element the campaign already has carries its id in `existing`; the
//! analysis left it unchecked, and so it is answered.

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listImportedNotes";

/// What the persisted query `list-imported-notes.graphql` selects: one row
/// per note of the campaign bound to `$1` (compared as text, so that an id
/// which is not a uuid lists nothing instead of failing), newest first.
/// `head` is the start of the raw note, enough for its title; `proposed` and
/// `created` count the note's proposals and those that created an element.
/// `proposals` is filled on one note only: the latest analysed one, as long
/// as none of its proposals has created an element — that is, while the DM
/// has still to validate. `campaign_id` is selected too, so that [`answer`]
/// keeps only the campaign's own notes whoever ran the statement.
pub const SQL: &str = r#"SELECT n.id::text AS id,
    n.campaign_id::text AS campaign_id,
    to_char(n.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS date,
    left(n.raw_text, 200) AS head,
    char_length(n.raw_text) AS chars,
    (SELECT count(*) FROM note_proposal p WHERE p.note_id = n.id) AS proposed,
    (SELECT count(*) FROM note_proposal p WHERE p.note_id = n.id AND p.created_entity_id IS NOT NULL) AS created,
    CASE
        WHEN n.id = (
            SELECT l.id FROM imported_note l
            WHERE l.campaign_id::text = $1 AND l.analyzed_at IS NOT NULL
            ORDER BY l.analyzed_at DESC, l.id DESC
            LIMIT 1
        ) AND NOT EXISTS (
            SELECT 1 FROM note_proposal p WHERE p.note_id = n.id AND p.created_entity_id IS NOT NULL
        ) THEN coalesce(
            (SELECT jsonb_agg(jsonb_build_object(
                'id', p.id::text,
                'type', p.type,
                'name', p.name,
                'excerpt', p.excerpt,
                'existing', p.existing_entity_id::text,
                'checked', p.checked
            ) ORDER BY p.type, p.name, p.id) FROM note_proposal p WHERE p.note_id = n.id),
            '[]'::jsonb
        )
        ELSE '[]'::jsonb
    END AS proposals
FROM imported_note n
WHERE n.campaign_id::text = $1
ORDER BY n.created_at DESC, n.id DESC"#;

/// The longest title, in characters; a longer first line is cut and ends
/// with an ellipsis.
const TITLE_MAX: usize = 80;

/// The title of a note without one: its first line is blank.
const UNTITLED: &str = "Notes importées";

/// The title of a note, from the start of its text: the first line that says
/// something, without the full stops and spaces it ends with, cut at
/// [`TITLE_MAX`] characters.
fn title(head: &str) -> String {
    let line = head
        .lines()
        .map(|line| line.trim().trim_end_matches(['.', ' ']))
        .find(|line| !line.is_empty())
        .unwrap_or(UNTITLED);
    if line.chars().count() > TITLE_MAX {
        let cut: String = line.chars().take(TITLE_MAX - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_string()
    }
}

/// How the screen words a proposal's type: the store's `evenement` is an
/// « événement », the other types read as they are stored.
fn kind(stored: &Value) -> Value {
    if stored == "evenement" {
        json!("événement")
    } else {
        stored.clone()
    }
}

/// A proposal as the screen reads it: the stored column `existing_entity_id`
/// is the `existing` key, `null` when the proposal recognises no element.
fn proposal(row: &Value) -> Value {
    json!({
        "id": row["id"],
        "type": kind(&row["type"]),
        "name": row["name"],
        "excerpt": row["excerpt"],
        "existing": row["existing"],
        "checked": row["checked"],
    })
}

/// The imports of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — and the proposals awaiting validation. The rows of another
/// campaign are ignored; the imports keep the order of the rows (newest
/// first), the proposals that of the row that carries them. A campaign with
/// no note answers two empty lists.
#[must_use]
pub fn answer(rows: &Value, campaign: &str) -> Value {
    let own: Vec<&Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .collect()
        })
        .unwrap_or_default();
    let imports: Vec<Value> = own
        .iter()
        .map(|row| {
            json!({
                "id": row["id"],
                "date": row["date"],
                "title": title(row["head"].as_str().unwrap_or_default()),
                "chars": row["chars"].as_u64().unwrap_or(0),
                "proposed": row["proposed"].as_u64().unwrap_or(0),
                "created": row["created"].as_u64().unwrap_or(0),
            })
        })
        .collect();
    let proposals: Vec<Value> = own
        .iter()
        .find_map(|row| row["proposals"].as_array().filter(|p| !p.is_empty()))
        .map(|proposals| proposals.iter().map(proposal).collect())
        .unwrap_or_default();
    json!({ "imports": imports, "proposals": proposals })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CENDRES: &str = "campagne-cendres";
    const OTHER: &str = "campagne-autre";

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures")
            .join(format!("{name}.json"));
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    fn note(id: &str, campaign: &str, head: &str, proposals: &Value) -> Value {
        json!({
            "id": id,
            "campaign_id": campaign,
            "date": "2026-09-20",
            "head": head,
            "chars": 1180,
            "proposed": 4,
            "created": 3,
            "proposals": proposals,
        })
    }

    /// The fixture's imports as the rows [`SQL`] would select, newest first;
    /// the proposals ride on the newest one.
    fn rows() -> Value {
        let imports = fixture("campaign.listImportedNotes");
        let imports = imports.as_array().expect("an array");
        let proposals = fixture("NoteImport")["proposals"].clone();
        Value::Array(
            imports
                .iter()
                .rev()
                .enumerate()
                .map(|(i, import)| {
                    json!({
                        "id": import["id"],
                        "campaign_id": CENDRES,
                        "date": import["date"],
                        "head": format!("{}.\nla suite", import["title"].as_str().expect("a title")),
                        "chars": import["chars"],
                        "proposed": import["proposed"],
                        "created": import["created"],
                        "proposals": if i == 0 { proposals.clone() } else { json!([]) },
                    })
                })
                .collect(),
        )
    }

    #[test]
    fn the_imports_are_the_fixtures_newest_first() {
        let answered = answer(&rows(), CENDRES);
        let mut expected = fixture("campaign.listImportedNotes")
            .as_array()
            .expect("an array")
            .clone();
        expected.reverse();
        assert_eq!(answered["imports"], Value::Array(expected));
    }

    #[test]
    fn the_proposals_are_the_fixtures_with_their_duplicates_unchecked() {
        let answered = answer(&rows(), CENDRES);
        let proposals = answered["proposals"].as_array().expect("an array");
        assert_eq!(
            proposals,
            fixture("NoteImport")["proposals"]
                .as_array()
                .expect("an array")
        );
        for p in proposals {
            assert_eq!(
                p["checked"],
                json!(p["existing"].is_null()),
                "{p}: a duplicate is unchecked"
            );
        }
    }

    #[test]
    fn a_stored_event_reads_as_an_evenement_with_an_accent() {
        let rows = json!([note(
            "n",
            CENDRES,
            "x",
            &json!([{ "id": "p", "type": "evenement", "name": "Le bal", "excerpt": "…",
                      "existing": null, "checked": true }]),
        )]);
        assert_eq!(answer(&rows, CENDRES)["proposals"][0]["type"], "événement");
    }

    #[test]
    fn another_campaigns_notes_do_not_appear() {
        let mut rows = rows();
        rows.as_array_mut().expect("an array").push(note(
            "imp-intrus",
            OTHER,
            "Intrus",
            &json!([{ "id": "np-intrus", "type": "pnj", "name": "Intrus", "excerpt": "…",
                      "existing": null, "checked": true }]),
        ));
        let answered = answer(&rows, CENDRES);
        assert!(!answered.to_string().contains("intrus"), "{answered}");
        assert_eq!(answered["imports"].as_array().expect("an array").len(), 2);
        let other = answer(&rows, OTHER);
        assert_eq!(other["imports"][0]["id"], "imp-intrus");
        assert_eq!(other["proposals"][0]["id"], "np-intrus");
    }

    #[test]
    fn a_campaign_with_no_note_has_two_empty_lists() {
        for rows in [json!([]), Value::Null, rows()] {
            let answered = answer(&rows, "campagne-vide");
            assert_eq!(answered, json!({ "imports": [], "proposals": [] }));
        }
    }

    #[test]
    fn a_note_with_nothing_to_validate_is_listed_without_proposals() {
        let rows = json!([note("n", CENDRES, "Séance 3", &json!([]))]);
        let answered = answer(&rows, CENDRES);
        assert_eq!(answered["imports"][0]["title"], "Séance 3");
        assert_eq!(answered["proposals"], json!([]));
    }

    #[test]
    fn a_title_is_the_first_line_that_says_something() {
        assert_eq!(
            title("\n  \nValombre — séance 2.\nLe moulin"),
            "Valombre — séance 2"
        );
        assert_eq!(title(" ... \n"), UNTITLED);
        assert_eq!(title(""), UNTITLED);
    }

    #[test]
    fn a_long_title_is_cut_with_an_ellipsis() {
        let long = "a".repeat(TITLE_MAX + 20);
        let cut = title(&long);
        assert_eq!(cut.chars().count(), TITLE_MAX);
        assert!(cut.ends_with('…'));
        assert_eq!(title(&"a".repeat(TITLE_MAX)), "a".repeat(TITLE_MAX));
    }
}
