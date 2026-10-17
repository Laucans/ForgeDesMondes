//! Capability `ai.getSettings`: the AI provider settings and the last
//! connection test, as the settings screen shows them.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/ai/queries/getSettings.graphql`, resolved by [`SQL`]) go in; the
//! answer, in the shape of `mock/fixtures/ai.getSettings.json`, comes out:
//!
//! - `provider`, `mode`, `binary`: the active provider — its API type
//!   (`cloud`, `opencode`) in API mode, its binary (`opencode`, `claude-code`)
//!   in binary mode;
//! - `providers`: each API provider keyed by type, with its label, URL,
//!   models and its key masked; `binaries`: the same for the binaries;
//! - `lastTest` and `testResults`: the most recent test, and each tested
//!   provider's outcome, the message copied as it was written;
//! - `local` and `sent`: what stays on this machine and what is sent to the
//!   provider, which do not depend on the store.
//!
//! The API key is read only to be masked ([`mask`]): the clear key is in no
//! answer. The API type is stored with the provider; [`detect`] is this
//! Capability's own reading of `ProviderApiType@1`, held to the Concept's
//! fixtures, for a row that holds none.

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "ai.getSettings";

/// What the persisted query `getSettings.graphql` selects: every provider with
/// the last test it holds. The test's time is a fixed-width UTC text, so that
/// the most recent sorts last as text.
pub const SQL: &str = r#"SELECT p.id::text AS id,
    p.mode,
    p.label,
    p.url,
    p.detected_api_type,
    p.api_key,
    p."binary",
    p.generation_model,
    p.summary_model,
    p.last_test_ok,
    to_char(p.last_test_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS last_test_at,
    p.last_test_message,
    p.last_test_latency_ms,
    coalesce(p.id = s.active_provider_id, false) AS active
FROM ai_provider p
LEFT JOIN ai_settings s ON s.id = 1
ORDER BY p.id"#;

/// What a caller is told when a URL is not recognised: the Concept's message.
const UNRECOGNISED_API_TYPE: &str = "type d'API non reconnu";

/// The only host told to be the cloud by the URL alone.
const CLOUD_HOST: &str = "api.anthropic.com";

/// What the DM is told stays on this machine.
const LOCAL: [&str; 3] = [
    "L’audio du micro, qui ne quitte jamais cette machine",
    "La transcription en français (Phonon, en local)",
    "La détection des noms et alias de la campagne et des noms de sons",
];

/// What the DM is told is sent to the provider.
const SENT: [&str; 3] = [
    "Les prompts initiaux des personnages, une fois transcrits",
    "Les briefs de séance transcrits et les notes importées",
    "Le journal de séance, pour en générer le résumé",
];

/// How many bullets stand for a masked key, whatever its length.
const BULLETS: usize = 16;

/// The settings `rows` — an array of the objects [`SQL`] selects — as the
/// screen reads them. No row is an empty, well-formed answer.
#[must_use]
pub fn answer(rows: &Value) -> Value {
    let rows: Vec<&Value> = rows
        .as_array()
        .map(|r| r.iter().collect())
        .unwrap_or_default();
    let mut providers = Map::new();
    let mut binaries = Map::new();
    let mut tests = Map::new();
    let mut active = None;
    let mut newest: Option<(&str, bool, &Value)> = None;
    for row in &rows {
        let Some(kind) = kind_of(row) else { continue };
        let name = kind.name();
        let models = |entry: &mut Map<String, Value>| {
            entry.insert("generationModel".into(), row["generation_model"].clone());
            entry.insert("summaryModel".into(), row["summary_model"].clone());
        };
        let mut entry = Map::new();
        entry.insert("label".into(), row["label"].clone());
        match kind {
            Kind::Api(_) => {
                entry.insert("url".into(), row["url"].clone());
                models(&mut entry);
                entry.insert("key".into(), mask(row["api_key"].as_str()).into());
                providers.insert(name.clone(), Value::Object(entry));
            }
            Kind::Binary(_) => {
                models(&mut entry);
                binaries.insert(name.clone(), Value::Object(entry));
            }
        }
        if row["active"] == true {
            active = Some((name.clone(), kind));
        }
        if let (Some(at), Some(ok)) = (row["last_test_at"].as_str(), row["last_test_ok"].as_bool())
        {
            tests.insert(
                name,
                json!({ "ok": ok, "message": row["last_test_message"] }),
            );
            // Same time: the failure is the one to show.
            if newest.is_none_or(|(latest, was_ok, _)| (at, !ok) > (latest, !was_ok)) {
                newest = Some((at, ok, row));
            }
        }
    }
    let last_test = newest.and_then(|(at, ok, row)| {
        let provider = kind_of(row)?.name();
        Some(json!({
            "ok": ok,
            "at": format!("{}Z", at.get(..19).unwrap_or_else(|| at.trim_end_matches('Z'))),
            "provider": provider,
            "latency": row["last_test_latency_ms"].as_i64().map(latency),
            "message": row["last_test_message"],
        }))
    });
    let (mode, provider, binary) = match &active {
        Some((name, Kind::Api(_))) => ("api", json!(name), Value::Null),
        Some((name, Kind::Binary(_))) => (
            "binary",
            json!(name),
            binaries.get(name).map_or(Value::Null, |models| {
                let mut chosen = Map::from_iter([("binary".to_string(), json!(name))]);
                chosen.insert("generationModel".into(), models["generationModel"].clone());
                chosen.insert("summaryModel".into(), models["summaryModel"].clone());
                Value::Object(chosen)
            }),
        ),
        None => ("api", Value::Null, Value::Null),
    };
    json!({
        "provider": provider,
        "mode": mode,
        "binary": binary,
        "providers": providers,
        "binaries": binaries,
        "lastTest": last_test,
        "testResults": tests,
        "local": LOCAL,
        "sent": SENT,
    })
}

/// What a provider row is: an API of a recognised type, or a binary.
enum Kind {
    Api(String),
    Binary(String),
}

impl Kind {
    fn name(&self) -> String {
        match self {
            Self::Api(name) | Self::Binary(name) => name.clone(),
        }
    }
}

fn kind_of(row: &Value) -> Option<Kind> {
    if row["mode"] == "binaire" {
        return row["binary"].as_str().map(|b| Kind::Binary(b.to_string()));
    }
    let stored = row["detected_api_type"].as_str().map(str::to_string);
    let detected = stored.or_else(|| {
        let found = detect(json!({ "url": row["url"] }));
        found["type"].as_str().map(str::to_string)
    });
    detected.map(Kind::Api)
}

/// `key` hidden but for what tells two keys apart: the leading characters of a
/// long key (its vendor prefix), always bullets of a fixed count, and its last
/// four characters. A key too short to show any of it is bullets alone; no key
/// is `""`.
#[must_use]
pub fn mask(key: Option<&str>) -> String {
    let chars: Vec<char> = key.unwrap_or_default().chars().collect();
    let n = chars.len();
    if n == 0 {
        return String::new();
    }
    let bullets = "•".repeat(BULLETS);
    if n < 8 {
        return bullets;
    }
    let head: String = if n >= 24 {
        chars[..7].iter().collect()
    } else {
        String::new()
    };
    let tail: String = chars[n - 4..].iter().collect();
    format!("{head}{bullets}{tail}")
}

/// A latency of `ms` milliseconds as the screen writes it: `0,84 s`.
fn latency(ms: i64) -> String {
    let hundredths = (ms.max(0) + 5) / 10;
    format!("{},{:02} s", hundredths / 100, hundredths % 100)
}

/// `ProviderApiType@1`, implemented locally: the type of the API behind
/// `input.url`, given what the endpoint answered to its list of models
/// (`input.answer`, absent or null when it did not answer).
///
/// The host `api.anthropic.com` is `cloud`, whatever the endpoint answered;
/// any other host is `opencode` when the endpoint answered its model list in
/// the OpenAI-compatible shape; everything else is not recognised.
#[must_use]
pub fn detect(input: Value) -> Value {
    let url = input["url"].as_str().unwrap_or_default();
    let found = host_of(url).and_then(|host| {
        if host == CLOUD_HOST {
            Some("cloud")
        } else if input["answer"]["object"] == "list" && input["answer"]["data"].is_array() {
            Some("opencode")
        } else {
            None
        }
    });
    json!({
        "type": found,
        "recognized": found.is_some(),
        "message": if found.is_some() { Value::Null } else { json!(UNRECOGNISED_API_TYPE) },
    })
}

/// The host of an absolute http(s) URL, lowercased, without userinfo or port.
fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    // A backslash ends the authority too, as it does for the clients that
    // will connect: `evil.example\@api.anthropic.com` is the host `evil.example`.
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if host_port.starts_with('[') {
        &host_port[..=host_port.find(']')?]
    } else {
        host_port
            .rsplit_once(':')
            .map_or(host_port, |(host, _)| host)
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
    const HOME: &str = "a1000000-0000-4000-8000-000000000002";

    /// A provider row as [`SQL`] selects it, seeded: no key, no test yet.
    fn api_row(id: &str, kind: &str, active: bool) -> Value {
        let (label, url, generation, summary) = if kind == "cloud" {
            (
                "API cloud",
                "https://api.anthropic.com/v1",
                "claude-sonnet-5-5",
                "claude-haiku-4-5-20251001",
            )
        } else {
            (
                "Endpoint opencode du serveur maison",
                "http://192.168.1.40:4096",
                "qwen2.5-32b-instruct",
                "qwen2.5-14b-instruct",
            )
        };
        json!({
            "id": id, "mode": "api", "label": label, "url": url,
            "detected_api_type": kind, "api_key": null, "binary": null,
            "generation_model": generation, "summary_model": summary,
            "last_test_ok": null, "last_test_at": null, "last_test_message": null,
            "last_test_latency_ms": null, "active": active,
        })
    }

    fn binary_row(id: &str, binary: &str, active: bool) -> Value {
        json!({
            "id": id, "mode": "binaire", "label": format!("Binaire {binary}"), "url": null,
            "detected_api_type": null, "api_key": null, "binary": binary,
            "generation_model": "g", "summary_model": "s",
            "last_test_ok": null, "last_test_at": null, "last_test_message": null,
            "last_test_latency_ms": null, "active": active,
        })
    }

    fn tested(mut row: Value, ok: bool, at: &str, message: &str, latency: Option<i64>) -> Value {
        row["last_test_ok"] = json!(ok);
        row["last_test_at"] = json!(at);
        row["last_test_message"] = json!(message);
        row["last_test_latency_ms"] = json!(latency);
        row
    }

    fn seeded() -> Value {
        json!([
            api_row(CLOUD, "cloud", true),
            api_row(HOME, "opencode", false),
            binary_row("a1000000-0000-4000-8000-000000000003", "opencode", false),
            binary_row("a1000000-0000-4000-8000-000000000004", "claude-code", false),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/ai.getSettings.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn conformance() {
        platform_conformance::check("ProviderApiType@1", detect);
    }

    #[test]
    fn the_answer_has_the_keys_of_the_fixture() {
        let answer = answer(&seeded());
        for key in fixture().as_object().expect("an object").keys() {
            assert!(answer.get(key).is_some(), "{key} is missing from {answer}");
        }
        let providers = answer["providers"].as_object().expect("an object");
        for (kind, expected) in fixture()["providers"].as_object().expect("an object") {
            let got = providers[kind].as_object().expect("a provider");
            for field in expected.as_object().expect("an object").keys() {
                assert!(got.contains_key(field), "{kind}.{field} is missing");
            }
        }
    }

    #[test]
    fn what_stays_local_and_what_is_sent_are_the_mock_ups() {
        let answer = answer(&json!([]));
        assert_eq!(answer["local"], fixture()["local"]);
        assert_eq!(answer["sent"], fixture()["sent"]);
    }

    #[test]
    fn the_seeded_store_shows_the_cloud_active_and_no_test() {
        let answer = answer(&seeded());
        assert_eq!(answer["provider"], "cloud");
        assert_eq!(answer["mode"], "api");
        assert_eq!(answer["binary"], Value::Null);
        assert_eq!(answer["lastTest"], Value::Null);
        assert_eq!(
            answer["providers"]["opencode"]["url"],
            "http://192.168.1.40:4096"
        );
        assert_eq!(answer["providers"]["cloud"]["key"], "");
        assert_eq!(answer["binaries"]["claude-code"]["generationModel"], "g");
    }

    #[test]
    fn the_key_is_masked_and_the_clear_key_is_in_no_answer() {
        let key = "sk-ant-api03-Zr8qXv0Lm2Nd5Tg7Uc9Ye3Wa1Sb4Hf6Jk3f9c";
        let mut cloud = api_row(CLOUD, "cloud", true);
        cloud["api_key"] = json!(key);
        let mut short = api_row(HOME, "opencode", false);
        short["api_key"] = json!("abc123");
        let answer = answer(&json!([cloud, short]));
        assert_eq!(
            answer["providers"]["cloud"]["key"],
            "sk-ant-••••••••••••••••3f9c"
        );
        assert_eq!(answer["providers"]["opencode"]["key"], "••••••••••••••••");
        let text = answer.to_string();
        assert!(!text.contains(key), "{text}");
        assert!(!text.contains("abc123"), "{text}");
        assert!(!text.contains("Zr8qXv0Lm2"), "{text}");
    }

    #[test]
    fn a_key_of_medium_length_shows_only_its_last_four_characters() {
        assert_eq!(mask(Some("0123456789ab")), "••••••••••••••••89ab");
        assert_eq!(mask(Some("1234567")), "••••••••••••••••");
        assert_eq!(mask(Some("")), "");
        assert_eq!(mask(None), "");
    }

    #[test]
    fn the_most_recent_test_is_the_last_test() {
        let rows = json!([
            tested(
                api_row(CLOUD, "cloud", true),
                true,
                "2026-10-17T10:00:01.000000Z",
                "Connexion réussie : cloud",
                Some(1200)
            ),
            tested(
                api_row(HOME, "opencode", false),
                true,
                "2026-10-17T10:00:07.250000Z",
                "Connexion réussie : opencode",
                Some(840)
            ),
        ]);
        let answer = answer(&rows);
        assert_eq!(
            answer["lastTest"],
            json!({
                "ok": true,
                "at": "2026-10-17T10:00:07Z",
                "provider": "opencode",
                "latency": "0,84 s",
                "message": "Connexion réussie : opencode",
            })
        );
        assert_eq!(answer["testResults"]["cloud"]["ok"], true);
        assert_eq!(
            answer["testResults"]["opencode"]["message"],
            "Connexion réussie : opencode"
        );
    }

    #[test]
    fn a_refused_test_shows_not_ok_with_its_message_and_no_latency() {
        let message = "type d'API non reconnu";
        let rows = json!([
            tested(
                api_row(CLOUD, "cloud", true),
                false,
                "2026-10-17T10:00:09.000000Z",
                message,
                None
            ),
            tested(
                api_row(HOME, "opencode", false),
                true,
                "2026-10-17T10:00:07.000000Z",
                "Connexion réussie",
                Some(840)
            ),
        ]);
        let last = &answer(&rows)["lastTest"];
        assert_eq!(last["ok"], false);
        assert_eq!(last["message"], message);
        assert_eq!(last["latency"], Value::Null);
        assert_eq!(last["provider"], "cloud");
    }

    #[test]
    fn at_the_same_instant_the_failure_is_shown() {
        let at = "2026-10-17T10:00:07.000000Z";
        let rows = json!([
            tested(api_row(CLOUD, "cloud", true), true, at, "ok", Some(10)),
            tested(api_row(HOME, "opencode", false), false, at, "ko", None),
        ]);
        assert_eq!(answer(&rows)["lastTest"]["ok"], false);
    }

    #[test]
    fn the_saved_urls_are_what_the_rows_hold_whatever_the_last_test() {
        let before = answer(&seeded());
        let rows = json!([
            tested(
                api_row(CLOUD, "cloud", true),
                false,
                "2026-10-17T10:00:09.000000Z",
                "type d'API non reconnu",
                None
            ),
            api_row(HOME, "opencode", false),
        ]);
        let after = answer(&rows);
        for kind in ["cloud", "opencode"] {
            assert_eq!(
                after["providers"][kind]["url"],
                before["providers"][kind]["url"]
            );
        }
    }

    #[test]
    fn a_binary_is_active_with_its_models_and_its_own_provider_name() {
        let mut rows = seeded();
        rows[0]["active"] = json!(false);
        rows[3]["active"] = json!(true);
        let answer = answer(&rows);
        assert_eq!(answer["mode"], "binary");
        assert_eq!(answer["provider"], "claude-code");
        assert_eq!(
            answer["binary"],
            json!({ "binary": "claude-code", "generationModel": "g", "summaryModel": "s" })
        );
    }

    #[test]
    fn a_row_without_a_stored_type_is_told_by_its_url() {
        let mut row = api_row(CLOUD, "cloud", true);
        row["detected_api_type"] = Value::Null;
        let answer = answer(&json!([row]));
        assert_eq!(answer["provider"], "cloud");
        let mut unknown = api_row(HOME, "opencode", false);
        unknown["detected_api_type"] = Value::Null;
        assert_eq!(super::answer(&json!([unknown]))["providers"], json!({}));
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        let answer = answer(&Value::Null);
        assert_eq!(answer["provider"], Value::Null);
        assert_eq!(answer["providers"], json!({}));
        assert_eq!(answer["lastTest"], Value::Null);
    }

    #[test]
    fn a_latency_is_written_in_seconds_with_a_comma() {
        assert_eq!(latency(840), "0,84 s");
        assert_eq!(latency(1200), "1,20 s");
        assert_eq!(latency(0), "0,00 s");
        assert_eq!(latency(12_345), "12,35 s");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/getSettings.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        for field in query
            .lines()
            .skip_while(|l| !l.starts_with("query "))
            .skip(1)
            .map(str::trim)
            .filter(|l| l.chars().all(|c| c.is_ascii_alphanumeric()) && !l.is_empty())
        {
            let column: String = field
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_uppercase() {
                        vec!['_', c.to_ascii_lowercase()]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            let selected = SQL.contains(&format!("p.{column}"))
                || SQL.contains(&format!("p.\"{column}\""))
                || SQL.contains(&format!("AS {column}"))
                || column == "id";
            assert!(
                selected,
                "the query names {field}, the SQL does not select it"
            );
        }
    }
}
