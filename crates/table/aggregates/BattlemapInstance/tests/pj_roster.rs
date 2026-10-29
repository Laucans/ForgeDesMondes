//! The BattlemapInstance invariants `pj-hp-not-tracked` and `token-roster`
//! that read what one row cannot — the type of the token's character, the
//! other tokens of the instance — as migration
//! `0015_battlemap_token_character_type.sql` has PostgreSQL judge them
//! (`DATABASE_URL`): a PC that tracks HP, an NPC that does not, a PC's second
//! token on an instance are refused by the tables whatever the order two writes
//! land in — SQLSTATE 23514 and 23505, which the projection turns into a
//! rejected command, never a failed store. Each test works in a scratch
//! database of its own (`platform-test-kit`), migrated as the server migrates
//! it, dropped when it ends.

use harness_dataguard::command::State;
use platform_test_kit::{Bench, Scratch, Served, planter};
use serde_json::{Value, json};

const AGGREGATE: &str = include_str!("../aggregate.json");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const OTHER_SCENE: &str = "5c000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const OTHER_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const TOKEN_1: &str = "0a000000-0000-4000-8000-000000000001";
const TOKEN_2: &str = "0a000000-0000-4000-8000-000000000002";
const TOKEN_3: &str = "0a000000-0000-4000-8000-000000000003";
const TOKEN_4: &str = "0a000000-0000-4000-8000-000000000004";
const TOKEN_5: &str = "0a000000-0000-4000-8000-000000000005";

/// A database holding Valombre, its session, two scenes, its battlemap of the
/// ford, Elowen (a PC) and the Bandit (an NPC), and an instance on each scene.
async fn valombre(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'live', false);
             INSERT INTO scene (id, session_id, position, title) VALUES
                 ('{SCENE}', '{SEANCE}', 1, 'Le gué'),
                 ('{OTHER_SCENE}', '{SEANCE}', 2, 'La forêt');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                 ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                 ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                 ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
             INSERT INTO character (entity_id, hp_max, token_color) VALUES
                 ('{ELOWEN}', 44, 'success'),
                 ('{BANDIT}', 11, 'danger');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;"
        ))
        .await;
    scratch
}

/// The aggregate beside two test-only DataCapabilities: a row written whole,
/// fields of a row changed.
fn bench() -> Bench {
    Bench::new(
        &planter("test.insert", "BattlemapInstance", "$id", "insert"),
        AGGREGATE,
        [aggregate_battlemap_instance::plugin()],
    )
    .planting(&planter(
        "test.update",
        "BattlemapInstance",
        "$id",
        "update",
    ))
}

fn instance(id: &str, scene: &str) -> Value {
    json!({
        "battlemapInstanceId": id, "battlemapMediaId": GUE, "sessionId": SEANCE,
        "sceneId": scene, "createdAt": "2026-10-21T10:00:00Z",
    })
}

/// A token of `character` on `instance`, which names no marker; its HP are
/// `hp` (a PC's are null).
fn token(id: &str, instance: &str, character: &str, number: Option<i64>, hp: Option<i64>) -> Value {
    json!({
        "tokenId": id, "tokenInstanceId": instance, "characterId": character,
        "number": number, "name": "Jeton", "q": 1, "r": 2, "visible": true,
        "hpCurrent": hp, "hpMax": hp, "conditions": [],
    })
}

/// The command `data_capability` writes, applied.
async fn applied(served: &Served, data_capability: &str, id: &str, payload: Value) {
    let result = served.applied(data_capability, payload, Some(id)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The command `data_capability` writes, rejected by the tables and not by
/// the judge, which does not know the character's type: the applier settles
/// it — a store that failed would have failed the run.
async fn rejected_by_the_tables(served: &Served, data_capability: &str, id: &str, payload: Value) {
    let result = served.applied(data_capability, payload, Some(id)).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result.violations.is_empty(),
        "the judge let it through: {result:?}"
    );
}

/// How many rows `from` selects.
async fn count(scratch: &Scratch, from: &str) -> i64 {
    scratch
        .rows(&format!("SELECT count(*) AS n FROM {from}"), &[])
        .await[0]["n"]
        .as_i64()
        .expect("a count")
}

/// Column `column` of token `id` in the tables.
async fn held(scratch: &Scratch, column: &str, id: &str) -> Value {
    scratch
        .rows(
            &format!("SELECT {column} AS v FROM token WHERE id = $1::text::uuid"),
            &[json!(id)],
        )
        .await[0]["v"]
        .clone()
}

/// The two instances, applied.
async fn instances(served: &Served) {
    applied(served, "test.insert", INSTANCE, instance(INSTANCE, SCENE)).await;
    applied(
        served,
        "test.insert",
        OTHER_INSTANCE,
        instance(OTHER_INSTANCE, OTHER_SCENE),
    )
    .await;
}

// AC3: a PC's token given HP, dropped or changed, is refused by the tables
// and its command rejected; the judge, which does not know the type, lets it
// through. The marker is its character's, given by the tables.
#[tokio::test]
async fn pj_hp_not_tracked_is_refused_by_the_tables_for_a_pc_with_hp() {
    let scratch = valombre("pj_hp_pc").await;
    let served = bench().postgres(&scratch).await;
    instances(&served).await;
    applied(
        &served,
        "test.insert",
        TOKEN_1,
        token(TOKEN_1, INSTANCE, ELOWEN, None, None),
    )
    .await;
    assert_eq!(held(&scratch, "character_type", TOKEN_1).await, "pj");

    rejected_by_the_tables(
        &served,
        "test.insert",
        TOKEN_2,
        token(TOKEN_2, OTHER_INSTANCE, ELOWEN, None, Some(10)),
    )
    .await;
    assert_eq!(count(&scratch, "token").await, 1);
    // (One HP alone, the judge refuses itself; the tables again, in
    // `crates/data/tests/`.)
    rejected_by_the_tables(
        &served,
        "test.update",
        TOKEN_1,
        json!({ "hpCurrent": 10, "hpMax": 10 }),
    )
    .await;
    assert_eq!(held(&scratch, "hp_current", TOKEN_1).await, Value::Null);
    assert_eq!(held(&scratch, "hp_max", TOKEN_1).await, Value::Null);
    // The marker the row names is its character's, or none: a PC cannot be
    // called an NPC to track HP.
    let mut liar = token(TOKEN_2, OTHER_INSTANCE, ELOWEN, None, Some(10));
    liar["characterType"] = json!("pnj");
    rejected_by_the_tables(&served, "test.insert", TOKEN_2, liar).await;
    let mut honest = token(TOKEN_2, OTHER_INSTANCE, ELOWEN, None, None);
    honest["characterType"] = json!("pj");
    applied(&served, "test.insert", TOKEN_2, honest).await;
    assert_eq!(held(&scratch, "character_type", TOKEN_2).await, "pj");
}

// AC4: an NPC's token that leaves its HP empty is refused by the tables and
// its command rejected; the judge, which does not know the type, accepts none.
// Below 0 or above the maximum, the judge refuses it itself
// (`tests/invariants.rs`) and the tables again (`crates/data/tests/`).
#[tokio::test]
async fn pj_hp_not_tracked_is_refused_by_the_tables_for_an_npc_without_hp() {
    let scratch = valombre("pj_hp_npc").await;
    let served = bench().postgres(&scratch).await;
    instances(&served).await;
    rejected_by_the_tables(
        &served,
        "test.insert",
        TOKEN_1,
        token(TOKEN_1, INSTANCE, BANDIT, Some(1), None),
    )
    .await;
    assert_eq!(count(&scratch, "token").await, 0);
    applied(
        &served,
        "test.insert",
        TOKEN_1,
        token(TOKEN_1, INSTANCE, BANDIT, Some(1), Some(11)),
    )
    .await;
    assert_eq!(held(&scratch, "character_type", TOKEN_1).await, "pnj");
    rejected_by_the_tables(
        &served,
        "test.update",
        TOKEN_1,
        json!({ "hpCurrent": null, "hpMax": null }),
    )
    .await;
    assert_eq!(held(&scratch, "hp_current", TOKEN_1).await, json!(11));
    // Down to 0: dead, not refused.
    applied(&served, "test.update", TOKEN_1, json!({ "hpCurrent": 0 })).await;
    assert_eq!(held(&scratch, "dead", TOKEN_1).await, json!(true));
}

// AC5, AC6, AC7: a PC's second token on an instance is refused by 23505 and
// rejected; the same PC stands on another instance; an NPC has several tokens
// on one instance.
#[tokio::test]
async fn token_roster_is_refused_by_the_tables_across_two_rows() {
    let scratch = valombre("token_roster").await;
    let served = bench().postgres(&scratch).await;
    instances(&served).await;
    applied(
        &served,
        "test.insert",
        TOKEN_1,
        token(TOKEN_1, INSTANCE, ELOWEN, None, None),
    )
    .await;

    // The judge sees the second token alone and lets it through: the tables
    // refuse it, and the command goes back to its author.
    for number in [None, Some(1)] {
        rejected_by_the_tables(
            &served,
            "test.insert",
            TOKEN_2,
            token(TOKEN_2, INSTANCE, ELOWEN, number, None),
        )
        .await;
    }
    assert_eq!(count(&scratch, "token").await, 1);
    assert_eq!(
        count(
            &scratch,
            &format!("dataguard_rows WHERE partition = 'BattlemapInstance/{TOKEN_2}'")
        )
        .await,
        0,
        "the engine holds no row of a rejected command"
    );

    // AC6: the same PC on another instance.
    applied(
        &served,
        "test.insert",
        TOKEN_3,
        token(TOKEN_3, OTHER_INSTANCE, ELOWEN, None, None),
    )
    .await;
    // AC7: two tokens of the same NPC on one instance, and another's.
    applied(
        &served,
        "test.insert",
        TOKEN_2,
        token(TOKEN_2, INSTANCE, BANDIT, Some(1), Some(11)),
    )
    .await;
    applied(
        &served,
        "test.insert",
        TOKEN_4,
        token(TOKEN_4, INSTANCE, BANDIT, Some(2), Some(11)),
    )
    .await;
    assert_eq!(count(&scratch, "token").await, 4);
}

/// The engine's row on `id`'s partition, written past the engine: what a
/// start finds in `dataguard_rows` that the tables do not hold.
async fn engine_only(scratch: &Scratch, id: &str, fields: &Value) {
    scratch
        .client()
        .execute(
            "INSERT INTO dataguard_rows (partition, fields) VALUES ($1, $2::text::jsonb)",
            &[&format!("BattlemapInstance/{id}"), &fields.to_string()],
        )
        .await
        .expect("the engine's row");
}

// AC3, AC4, AC5: rows only the engine holds that the tables refuse — a PC
// with HP, an NPC with none, a PC's second token on an instance — are logged
// and skipped by a start, which projects the others and opens.
#[tokio::test]
async fn a_start_logs_and_skips_the_tokens_the_tables_refuse() {
    let scratch = valombre("warm_tokens").await;
    // A first start makes the engine's own tables; the instance is applied,
    // in the engine and in the tables, and the engine closes.
    let served = bench().postgres(&scratch).await;
    applied(&served, "test.insert", INSTANCE, instance(INSTANCE, SCENE)).await;
    drop(served);
    // Five tokens the tables lost: a PC with HP, a PC, the same PC again, an
    // NPC with no HP, and an NPC.
    for (id, character, number, hp) in [
        (TOKEN_1, ELOWEN, None, Some(10)),
        (TOKEN_2, ELOWEN, None, None),
        (TOKEN_3, ELOWEN, None, None),
        (TOKEN_4, BANDIT, Some(1), None),
        (TOKEN_5, BANDIT, Some(2), Some(11)),
    ] {
        engine_only(&scratch, id, &token(id, INSTANCE, character, number, hp)).await;
    }

    // The start does not fail: each partition a refused write leaves is
    // logged, the others are projected.
    let served = bench().postgres(&scratch).await;
    let tokens: Vec<String> = scratch
        .rows("SELECT id::text FROM token ORDER BY id", &[])
        .await
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(tokens, [TOKEN_2, TOKEN_5]);
    // The engine goes on over the rows the start projected.
    applied(&served, "test.update", TOKEN_2, json!({ "q": 5 })).await;
}
