//! Capability `live.getTranscriptionVocabulary`: the words the microphone listens for in a
//! campaign — its entities' names and aliases and its sounds' names — for the DM, and for the
//! live detection that primes the local transcription with them.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/live/queries/get-transcription-vocabulary.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/live.getTranscriptionVocabulary.json`, comes out.
//! The filter is the SQL's, every request variable bound there: [`answer`]
//! never filters the rows again.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "live.getTranscriptionVocabulary";

/// What the persisted query selects: the terms of `detection_vocabulary_view` (the names and
/// aliases of the entities, a blank one left out), then the names of the sounds (a blank one
/// left out as well, `alias_match_normalized` being AliasMatch@1), of the campaign `$1` alone,
/// each term as written and once.
pub const SQL: &str = r#"SELECT t.term
FROM (
    SELECT v.campaign_id, v.term, 0 AS source
        FROM detection_vocabulary_view v
    UNION ALL
    SELECT s.campaign_id, s.name AS term, 1 AS source
        FROM sound s
        WHERE alias_match_normalized(s.name) <> ''
) t
WHERE t.campaign_id::text = $1
GROUP BY t.term
ORDER BY min(t.source), t.term"#;

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["campaign"];

/// The answer to a read, as the registry serves it: the `rows` [`SQL`]
/// selected, already filtered by the request's variables.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

/// The answer `rows` make: `{ terms }`, the `term` of each row in the order of the rows; no row
/// is no term.
fn answer_rows(rows: &Value) -> Value {
    let rows = rows.as_array().map(Vec::as_slice).unwrap_or_default();
    let terms: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get("term").and_then(Value::as_str))
        .collect();
    json!({ "terms": terms })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_test_kit::{Scratch, read};
    use serde_json::json;

    #[test]
    fn no_row_is_no_term() {
        assert_eq!(answer_rows(&json!([])), json!({ "terms": [] }));
        assert_eq!(answer_rows(&Value::Null), json!({ "terms": [] }));
    }

    #[test]
    fn the_terms_are_the_rows_terms_in_order_as_written() {
        let rows = json!([
            { "term": "La Taverne du Sanglier" },
            { "term": "la taverne" },
            { "term": "Porte qui grince" },
        ]);
        assert_eq!(
            answer_rows(&rows),
            json!({ "terms": ["La Taverne du Sanglier", "la taverne", "Porte qui grince"] })
        );
    }

    #[test]
    fn a_row_with_no_text_term_names_nothing() {
        let rows = json!([{ "term": null }, { "other": "x" }, "bare", { "term": "Morlox" }]);
        assert_eq!(answer_rows(&rows), json!({ "terms": ["Morlox"] }));
    }

    #[test]
    fn the_sql_is_one_read_statement() {
        read::one_read_statement(SQL);
    }

    #[test]
    fn every_variable_is_bound_in_order() {
        read::binds_in_order(SQL, VARIABLES);
    }

    #[test]
    fn the_query_file_names_what_the_sql_selects() {
        let query = read::query("live", "get-transcription-vocabulary");
        read::query_file_matches(SQL, &query, VARIABLES);
    }

    #[test]
    fn the_registry_s_answer_is_the_answer_of_its_rows() {
        let rows = json!([{ "term": "Morlox" }]);
        assert_eq!(answer(&rows, &BTreeMap::new()), answer_rows(&rows));
    }

    #[test]
    fn the_answer_has_the_mock_up_s_shape() {
        let fixture = read::fixture(CAPABILITY);
        let terms = fixture["terms"].as_array().expect("terms is a list");
        assert!(!terms.is_empty());
        let rows: Vec<Value> = terms.iter().map(|t| json!({ "term": t })).collect();
        assert_eq!(answer_rows(&Value::Array(rows)), fixture);
    }

    // The Concept's own computation, local to this Capability (rule 3): the vocabulary of a
    // campaign and the detection of a phrase, held to the fixtures of DetectionVocabulary@1.
    mod concept {
        use std::collections::BTreeSet;

        use serde_json::{Value, json};
        use unicode_normalization::UnicodeNormalization;

        /// AliasMatch@1: trimmed, lowercase, accents (U+0300 to U+036F) removed from the
        /// decomposed letter, recomposed.
        fn normalised(term: &str) -> String {
            term.trim()
                .to_lowercase()
                .nfd()
                .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
                .nfc()
                .collect()
        }

        /// The normalised words of `text`: maximal runs of letters, digits and combining marks.
        fn words(text: &str) -> Vec<String> {
            text.split(|c: char| !(c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c)))
                .filter(|word| !word.is_empty())
                .map(normalised)
                .collect()
        }

        struct Entry {
            term: String,
            kind: &'static str,
            id: String,
            words: Vec<String>,
        }

        fn strings<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
            value[key]
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default()
        }

        fn vocabulary(input: &Value) -> Vec<Entry> {
            let campaign = &input["campaign"];
            let mut entries: Vec<Entry> = Vec::new();
            let mut push = |term: &str, kind: &'static str, id: &str| {
                let own = normalised(term);
                let twice = entries
                    .iter()
                    .any(|e| e.kind == kind && e.id == id && normalised(&e.term) == own);
                if !own.is_empty() && !twice {
                    entries.push(Entry {
                        term: term.to_string(),
                        kind,
                        id: id.to_string(),
                        words: words(term),
                    });
                }
            };
            let of_campaign = |list: &'static str| {
                input[list]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["campaign"] == *campaign)
                    .cloned()
                    .collect::<Vec<Value>>()
            };
            for entity in of_campaign("entities") {
                let id = entity["id"].as_str().unwrap_or_default();
                push(entity["name"].as_str().unwrap_or_default(), "entity", id);
                for alias in strings(&entity, "aliases") {
                    push(alias, "entity", id);
                }
            }
            for sound in of_campaign("sounds") {
                let id = sound["id"].as_str().unwrap_or_default();
                push(sound["name"].as_str().unwrap_or_default(), "sound", id);
            }
            entries
        }

        /// A candidate: where the entry occurs in the phrase.
        struct Candidate {
            start: usize,
            length: usize,
            entry: usize,
        }

        fn detections(entries: &[Entry], phrase: &str) -> Vec<Value> {
            let heard = words(phrase);
            let mut candidates: Vec<Candidate> = Vec::new();
            for (entry, e) in entries.iter().enumerate() {
                let n = e.words.len();
                if n == 0 || n > heard.len() {
                    continue;
                }
                for start in 0..=heard.len() - n {
                    if heard[start..start + n] == e.words[..] {
                        candidates.push(Candidate {
                            start,
                            length: e.words.join(" ").chars().count(),
                            entry,
                        });
                    }
                }
            }
            candidates.sort_by_key(|c| (std::cmp::Reverse(c.length), c.start, c.entry));
            let mut used: BTreeSet<usize> = BTreeSet::new();
            let mut named: BTreeSet<(&str, &str)> = BTreeSet::new();
            let mut kept: Vec<&Candidate> = Vec::new();
            for c in &candidates {
                let e = &entries[c.entry];
                let span = c.start..c.start + e.words.len();
                if span.clone().any(|w| used.contains(&w)) || named.contains(&(e.kind, &e.id)) {
                    continue;
                }
                used.extend(span);
                named.insert((e.kind, &e.id));
                kept.push(c);
            }
            kept.sort_by_key(|c| c.start);
            kept.iter()
                .map(|c| {
                    let e = &entries[c.entry];
                    json!({ "kind": e.kind, "id": e.id, "via": e.term })
                })
                .collect()
        }

        pub fn compute(input: &Value) -> Value {
            let entries = vocabulary(input);
            let detected = input
                .get("phrase")
                .and_then(Value::as_str)
                .map_or(Value::Null, |phrase| {
                    Value::Array(detections(&entries, phrase))
                });
            let vocabulary: Vec<Value> = entries
                .iter()
                .map(|e| json!({ "term": e.term, "kind": e.kind, "id": e.id }))
                .collect();
            json!({ "vocabulary": vocabulary, "detections": detected })
        }
    }

    #[test]
    fn conformance() {
        platform_conformance::check("DetectionVocabulary@1", |input| concept::compute(&input));
    }

    /// Two campaigns, each with elements and sounds; the answer of [`SQL`] for `campaign`.
    async fn terms_of(db: &Scratch, campaign: &str) -> Vec<String> {
        db.rows(SQL, &[json!(campaign)])
            .await
            .iter()
            .map(|row| row["term"].as_str().expect("a text term").to_string())
            .collect()
    }

    const A: &str = "a1000000-0000-4000-8000-000000000001";
    const B: &str = "b1000000-0000-4000-8000-000000000002";

    /// An element of `campaign` as the sheet's save writes it: the row, its character sheet
    /// when a `pj` or a `pnj`, its aliases and its first version, in the same transaction.
    fn element(id: &str, campaign: &str, kind: &str, name: &str, aliases: &[&str]) -> String {
        let sheet = if kind == "lieu" || kind == "evenement" {
            String::new()
        } else {
            format!("INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888');")
        };
        let aliases: String = aliases
            .iter()
            .map(|alias| {
                format!("INSERT INTO entity_alias (entity_id, alias) VALUES ('{id}', '{alias}');")
            })
            .collect();
        format!(
            "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}');
             {sheet} {aliases}
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
             VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
        )
    }

    async fn seeded(label: &str) -> Scratch {
        let db = Scratch::new(label).await;
        let elements = [
            element(
                "a2000000-0000-4000-8000-000000000001",
                A,
                "lieu",
                "La Taverne du Sanglier",
                &["la taverne"],
            ),
            element(
                "a2000000-0000-4000-8000-000000000002",
                A,
                "pnj",
                "Gaspard Grosmollet",
                &["Gaspard"],
            ),
            element(
                "a2000000-0000-4000-8000-000000000003",
                A,
                "evenement",
                "L’incendie du moulin",
                &["l’incendie"],
            ),
            element(
                "a2000000-0000-4000-8000-000000000004",
                A,
                "pj",
                "Elowen Ventargent",
                &["la rôdeuse"],
            ),
            element(
                "b2000000-0000-4000-8000-000000000001",
                B,
                "pnj",
                "Mordred l’Autre",
                &["le traître"],
            ),
        ]
        .concat();
        db.seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{A}', 'Valombre', 'campagne'), ('{B}', 'Autre', 'campagne');
             {elements}
             INSERT INTO sound (campaign_id, name, file_path) VALUES
               ('{A}', 'Porte qui grince', 'porte.ogg'),
               ('{A}', 'Cri de corbeau', 'corbeau.ogg'),
               ('{B}', 'Tonnerre lointain', 'tonnerre.ogg');"
        ))
        .await;
        db
    }

    fn has(terms: &[String], wanted: &str) -> bool {
        terms.iter().any(|t| t == wanted)
    }

    #[tokio::test]
    async fn an_alias_is_a_term_beside_the_name_it_belongs_to() {
        let db = seeded("vocabulary_alias").await;
        let terms = terms_of(&db, A).await;
        assert!(has(&terms, "La Taverne du Sanglier"));
        assert!(has(&terms, "la taverne"));
    }

    #[tokio::test]
    async fn every_name_and_alias_of_the_characters_places_and_events_is_a_term() {
        let db = seeded("vocabulary_entities").await;
        let terms: Vec<String> = terms_of(&db, A)
            .await
            .iter()
            .map(|t| t.to_lowercase())
            .collect();
        for wanted in [
            "gaspard grosmollet",
            "gaspard",
            "elowen ventargent",
            "la rôdeuse",
            "l’incendie du moulin",
            "l’incendie",
            "la taverne du sanglier",
            "la taverne",
        ] {
            assert!(has(&terms, wanted), "{wanted} is a term: {terms:?}");
        }
    }

    #[tokio::test]
    async fn the_names_of_the_campaign_s_sounds_are_terms() {
        let db = seeded("vocabulary_sounds").await;
        let terms = terms_of(&db, A).await;
        assert!(has(&terms, "Porte qui grince"));
        assert!(has(&terms, "Cri de corbeau"));
    }

    #[tokio::test]
    async fn nothing_of_another_campaign_is_a_term() {
        let db = seeded("vocabulary_other_campaign").await;
        let terms = terms_of(&db, A).await;
        assert_eq!(terms.len(), 10, "{terms:?}");
        for other in ["Mordred l’Autre", "le traître", "Tonnerre lointain"] {
            assert!(!has(&terms, other), "{other} belongs to another campaign");
        }
        let mut own = terms_of(&db, B).await;
        own.sort();
        assert_eq!(own, ["Mordred l’Autre", "Tonnerre lointain", "le traître"]);
        assert!(terms_of(&db, "not-a-uuid").await.is_empty());
    }

    #[tokio::test]
    async fn a_term_is_listed_once_as_written_the_entities_before_the_sounds() {
        let db = seeded("vocabulary_once").await;
        db.seed(&format!(
            "INSERT INTO sound (campaign_id, name, file_path) VALUES
               ('{A}', 'Gaspard', 'gaspard.ogg'), ('{A}', 'Gaspard', 'gaspard2.ogg'), ('{A}', '   ', 'blank.ogg');"
        ))
        .await;
        let terms = terms_of(&db, A).await;
        assert_eq!(terms.iter().filter(|t| *t == "Gaspard").count(), 1);
        assert!(!has(&terms, "   "));
        let first_sound = terms.iter().position(|t| t == "Cri de corbeau").unwrap();
        let last_entity = terms.iter().position(|t| t == "la taverne").unwrap();
        assert!(last_entity < first_sound, "{terms:?}");
    }
}
