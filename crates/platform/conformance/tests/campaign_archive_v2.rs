//! `CampaignArchive@2` against `CampaignArchive@1`: the one rule it changes —
//! the copy's campaign is created at the import — and nothing else. The
//! copy itself is proved against v1 by `campaign_archive.rs`; here every
//! expectation of v2 is v1's, with that one change made, so that v2 cannot
//! drift from the version it replaces.

use platform_conformance::{concept_of, fixtures_of};
use serde_json::Value;

fn concept(version: u64) -> Value {
    concept_of(&format!("CampaignArchive@{version}"))
}

/// An RFC 3339 instant as milliseconds since the Unix epoch.
fn instant(text: &str) -> i64 {
    let number = |range: std::ops::Range<usize>| -> i64 {
        text.get(range)
            .and_then(|digits| digits.parse().ok())
            .unwrap_or_else(|| panic!("{text:?} is not an RFC 3339 date"))
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let (hour, minute, second) = (number(11..13), number(14..16), number(17..19));
    // Days since 1970-01-01 of a civil date (proleptic Gregorian).
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;

    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        let padded = format!("{:0<3}", &fraction[..digits]);
        millis = padded[..3].parse::<i64>().expect("fraction digits");
        rest = &fraction[digits..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let hours: i64 = rest[1..3].parse().expect("offset hours");
            let minutes: i64 = rest[4..6].parse().expect("offset minutes");
            sign * (hours * 60 + minutes)
        }
    };
    (((days * 24 + hour) * 60 + minute - offset_minutes) * 60 + second) * 1000 + millis
}

/// The paragraph of the definition that opens with rule 5.
fn rule_5(concept: &Value) -> String {
    let definition = concept["definition"].as_str().expect("definition");
    let start = definition.find("\n5. ").expect("a rule 5");
    let rest = &definition[start + 1..];
    rest[..rest.find("\n\n").expect("rule 5 ends")].to_owned()
}

// AC1: concept.json of v2 is a derived CampaignArchive, version 2, that
// deprecates version 1 and points at its own fixtures (the contract's
// schema itself is the manifests-conform-to-contracts gate's).
#[test]
fn v2_is_a_concept_that_deprecates_version_1() {
    let (v1, v2) = (concept(1), concept(2));
    assert_eq!(v2["concept"], "CampaignArchive");
    assert_eq!(v2["version"], 2);
    assert_eq!(v2["kind"], "derived");
    assert_eq!(v2["conformance"], "./fixtures.json");
    assert_eq!(v2["deprecates"], 1);
    let keys = |concept: &Value| -> Vec<String> {
        concept
            .as_object()
            .expect("a concept")
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(keys(&v2), keys(&v1), "the same document, member for member");
    assert_eq!(v2["inputs"], v1["inputs"]);
    assert_eq!(
        v2["output"]
            .as_object()
            .expect("output")
            .keys()
            .collect::<Vec<_>>(),
        v1["output"]
            .as_object()
            .expect("output")
            .keys()
            .collect::<Vec<_>>(),
        "the copy has the same sections"
    );
}

// AC2: rule 5 of the copy says the campaign's created_at is the time of the
// import, as imported_at is; v1's rule 5 does not.
#[test]
fn rule_5_creates_the_copys_campaign_at_the_import() {
    let rule = rule_5(&concept(2));
    assert!(
        rule.contains("`created_at` and `imported_at` both set to the time of the import"),
        "{rule}"
    );
    assert!(!rule_5(&concept(1)).contains("created_at"));
    let output = &concept(2)["output"];
    for field in ["imported_at", "campaign"] {
        let text = output[field]["rule"].as_str().expect("a rule");
        assert!(text.contains("created_at"), "output.{field}: {text}");
    }
}

// AC2, AC3: every case's expected copy is created at its import, and is v1's
// expected copy in every other respect.
#[test]
fn every_case_of_v2_is_v1s_but_for_the_copys_created_at() {
    let (v1, v2) = (
        fixtures_of("CampaignArchive@1"),
        fixtures_of("CampaignArchive@2"),
    );
    assert_eq!(v2.cases.len(), v1.cases.len());
    assert!(!v2.cases.is_empty());
    for (old, new) in v1.cases.iter().zip(&v2.cases) {
        assert_eq!(new.name, old.name);
        assert_eq!(new.input, old.input, "{}: same input", new.name);

        let imported_at = &new.input["imported_at"];
        assert_eq!(new.output["imported_at"], *imported_at, "{}", new.name);
        assert_eq!(
            new.output["campaign"]["created_at"], *imported_at,
            "{}: the copy is created at its import",
            new.name
        );
        // The case shows the change: the original was created before.
        let text = |value: &Value| value.as_str().expect("a date").to_owned();
        assert_ne!(
            instant(&text(&new.input["archive"]["campaign"]["created_at"])),
            instant(&text(imported_at)),
            "{}: a case where the import time is the original's tells nothing",
            new.name
        );

        let mut expected = new.output.clone();
        expected["campaign"]["created_at"] = old.output["campaign"]["created_at"].clone();
        assert_eq!(expected, old.output, "{}: nothing else changes", new.name);
    }
}

// The parser behind the instants: offsets, fractions and the epoch.
#[test]
fn instants_are_compared_as_instants_not_as_text() {
    assert_eq!(instant("1970-01-01T00:00:00Z"), 0);
    assert_eq!(instant("2026-03-01T10:00:00+00:00"), 1_772_359_200_000);
    assert_eq!(
        instant("2026-03-01T11:00:00.000+01:00"),
        instant("2026-03-01T10:00:00Z")
    );
    assert_eq!(instant("2026-03-01T10:00:00.5Z"), 1_772_359_200_500);
    assert_eq!(instant("2026-03-01T04:30:00-05:30"), 1_772_359_200_000);
}

/// A concept with what version 2 changes taken out: the intro sentence that
/// says so, rule 5, the two output rules that mention `created_at`, and the
/// version numbers.
fn without_the_change(mut concept: Value) -> Value {
    let intro = "This version replaces CampaignArchive@1 in one rule only: the copy's campaign is \
                 created when it is imported (rule 5). ";
    let rule = rule_5(&concept);
    let definition = concept["definition"].as_str().expect("definition");
    concept["definition"] = definition.replace(intro, "").replace(&rule, "").into();
    for field in ["imported_at", "campaign"] {
        concept["output"][field]["rule"] = Value::Null;
    }
    concept["version"] = Value::Null;
    concept["deprecates"] = Value::Null;
    concept
}

// AC4: v1 is as it was — its copy keeps the original's created_at — so that
// what holds the archive's shape to it still holds. v2 is v1 but for the one
// rule, so every member of v1 (definition, inputs, output) is pinned by it.
#[test]
fn v1_is_unchanged_its_copy_keeps_the_originals_created_at() {
    let v1 = concept(1);
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["deprecates"], Value::Null);
    assert_eq!(
        without_the_change(v1),
        without_the_change(concept(2)),
        "v2 differs from v1 by more than rule 5"
    );
    for case in fixtures_of("CampaignArchive@1").cases {
        assert_eq!(
            case.output["campaign"]["created_at"], case.input["archive"]["campaign"]["created_at"],
            "{}",
            case.name
        );
    }
}
