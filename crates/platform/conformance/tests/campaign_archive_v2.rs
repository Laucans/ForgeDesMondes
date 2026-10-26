//! `CampaignArchive@2` against `CampaignArchive@1`: the one rule it changes —
//! the copy's campaign is created at the import — and nothing else. The
//! copy itself is proved against v1 by `campaign_archive.rs`; here every
//! expectation of v2 is v1's, with that one change made, so that v2 cannot
//! drift from the version it replaces.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::Value;

fn concept(version: u64) -> Value {
    let path = repository_root().join(format!("concepts/CampaignArchive/v{version}/concept.json"));
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
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
        assert_ne!(
            new.input["archive"]["campaign"]["created_at"], *imported_at,
            "{}: a case where the import time is the original's tells nothing",
            new.name
        );

        let mut expected = new.output.clone();
        expected["campaign"]["created_at"] = old.output["campaign"]["created_at"].clone();
        assert_eq!(expected, old.output, "{}: nothing else changes", new.name);
    }
}

// AC4: v1 is as it was — its copy keeps the original's created_at — so that
// what holds the archive's shape to it still holds.
#[test]
fn v1_is_unchanged_its_copy_keeps_the_originals_created_at() {
    let v1 = concept(1);
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["deprecates"], Value::Null);
    for case in fixtures_of("CampaignArchive@1").cases {
        assert_eq!(
            case.output["campaign"]["created_at"], case.input["archive"]["campaign"]["created_at"],
            "{}",
            case.name
        );
    }
}
