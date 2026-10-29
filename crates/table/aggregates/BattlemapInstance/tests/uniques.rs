//! The BattlemapInstance invariants that read several rows, as migration
//! `0014_battlemap_instance_uniques.sql` has PostgreSQL judge them
//! (`DATABASE_URL`): a second instance of a scene, a second combat running on
//! an instance, a number taken twice by a character on an instance are
//! refused by the tables whatever the order two writes land in — SQLSTATE
//! 23505, which the projection turns into a rejected command, never a failed
//! store; and a start over rows only the engine holds that the indexes refuse
//! logs them and goes on. Each test works in a scratch database of its own
//! (`platform-test-kit`), migrated as the server migrates it, dropped when it
//! ends.

use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::tokio_postgres::error::SqlState;
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
const ARCHER: &str = "e1000000-0000-4000-8000-000000000004";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const SECOND_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const OTHER_INSTANCE: &str = "b1000000-0000-4000-8000-000000000003";
const COMBAT: &str = "cb000000-0000-4000-8000-000000000001";
const SECOND_COMBAT: &str = "cb000000-0000-4000-8000-000000000002";
const TOKEN_1: &str = "0a000000-0000-4000-8000-000000000001";
const TOKEN_2: &str = "0a000000-0000-4000-8000-000000000002";
const TOKEN_3: &str = "0a000000-0000-4000-8000-000000000003";
const TOKEN_4: &str = "0a000000-0000-4000-8000-000000000004";

/// A database holding Valombre, its session, two scenes, its battlemap of the
/// ford, Elowen (a PC), the Bandit and the Archer (NPCs).
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
                 ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit'),
                 ('{ARCHER}', '{VALOMBRE}', 'pnj', 'Archer');
             INSERT INTO character (entity_id, hp_max, token_color) VALUES
                 ('{ELOWEN}', 44, 'success'),
                 ('{BANDIT}', 11, 'danger'),
                 ('{ARCHER}', 9, 'danger');
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

/// A token of `character` on `instance`, numbered `number`.
fn token(id: &str, instance: &str, character: &str, number: i64) -> Value {
    json!({
        "tokenId": id, "tokenInstanceId": instance, "characterId": character,
        "number": number, "name": format!("Jeton {number}"), "q": 1, "r": 2, "visible": true,
        "hpCurrent": 11, "hpMax": 11, "conditions": [],
    })
}

/// A combat on `instance`, running.
fn combat(id: &str, instance: &str) -> Value {
    json!({
        "combatId": id, "combatInstanceId": instance, "round": 1,
        "startedAt": "2026-10-21T10:05:00Z",
    })
}

/// The command `data_capability` writes, applied.
async fn applied(served: &Served, data_capability: &str, id: &str, payload: Value) {
    let result = served.applied(data_capability, payload, Some(id)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The command `data_capability` writes, rejected by the tables: the applier
/// settles it — a store that failed would have failed the run — and the
/// engine holds no row for it.
async fn rejected(
    scratch: &Scratch,
    served: &Served,
    data_capability: &str,
    id: &str,
    payload: Value,
) -> CommandResult {
    let result = served.applied(data_capability, payload, Some(id)).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        count(
            scratch,
            &format!("dataguard_rows WHERE partition = 'BattlemapInstance/{id}'")
        )
        .await,
        0,
        "the engine holds no row of a rejected command"
    );
    result
}

/// How many rows `from` selects.
async fn count(scratch: &Scratch, from: &str) -> i64 {
    scratch
        .rows(&format!("SELECT count(*) AS n FROM {from}"), &[])
        .await[0]["n"]
        .as_i64()
        .expect("a count")
}

/// The constraint a write the tables refuse as a duplicate breaks: `sql`
/// written to the tables directly, past the engine.
async fn duplicate_of(scratch: &Scratch, sql: &str) -> String {
    let error = scratch
        .client()
        .batch_execute(sql)
        .await
        .expect_err("the tables refuse a duplicate");
    let db = error.as_db_error().expect("a database error");
    assert_eq!(db.code(), &SqlState::UNIQUE_VIOLATION, "{db}");
    db.constraint().expect("a named constraint").to_owned()
}

// AC4: a second instance of a scene is refused by 23505 and rejected.
#[tokio::test]
async fn one_instance_per_scene_is_refused_by_the_tables_across_two_rows() {
    let scratch = valombre("one_instance_per_scene").await;
    let served = bench().postgres(&scratch).await;
    applied(&served, "test.insert", INSTANCE, instance(INSTANCE, SCENE)).await;

    // The judge sees the second instance alone and lets it through: the
    // tables refuse it, and the command goes back to its author.
    rejected(
        &scratch,
        &served,
        "test.insert",
        SECOND_INSTANCE,
        instance(SECOND_INSTANCE, SCENE),
    )
    .await;
    assert_eq!(count(&scratch, "battlemap_instance").await, 1);
    // It is the unique index that refuses, as a duplicate (23505).
    assert_eq!(
        duplicate_of(
            &scratch,
            &format!(
                "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{SECOND_INSTANCE}', '{GUE}', '{SEANCE}', '{SCENE}')"
            )
        )
        .await,
        "battlemap_instance_scene_id_unique"
    );
    // Another scene takes its own instance.
    applied(
        &served,
        "test.insert",
        OTHER_INSTANCE,
        instance(OTHER_INSTANCE, OTHER_SCENE),
    )
    .await;
    assert_eq!(count(&scratch, "battlemap_instance").await, 2);
}

// AC5: a second combat running on an instance is refused by 23505 and
// rejected; once the first is ended, a new one is accepted.
#[tokio::test]
async fn one_active_combat_is_refused_by_the_tables_across_two_rows() {
    let scratch = valombre("one_active_combat").await;
    let served = bench().postgres(&scratch).await;
    applied(&served, "test.insert", INSTANCE, instance(INSTANCE, SCENE)).await;
    applied(&served, "test.insert", COMBAT, combat(COMBAT, INSTANCE)).await;

    rejected(
        &scratch,
        &served,
        "test.insert",
        SECOND_COMBAT,
        combat(SECOND_COMBAT, INSTANCE),
    )
    .await;
    assert_eq!(count(&scratch, "combat").await, 1);
    assert_eq!(
        duplicate_of(
            &scratch,
            &format!(
                "INSERT INTO combat (id, instance_id, round, started_at)
                 VALUES ('{SECOND_COMBAT}', '{INSTANCE}', 1, now())"
            )
        )
        .await,
        "combat_one_active"
    );

    // The first ended, the instance runs a new one.
    applied(
        &served,
        "test.update",
        COMBAT,
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
    )
    .await;
    applied(
        &served,
        "test.insert",
        SECOND_COMBAT,
        combat(SECOND_COMBAT, INSTANCE),
    )
    .await;
    assert_eq!(count(&scratch, "combat").await, 2);
    assert_eq!(count(&scratch, "combat WHERE ended_at IS NULL").await, 1);
}

// AC6: a number taken twice by a character on an instance is refused by 23505
// and rejected; the same number for another character, or on another
// instance, is accepted.
#[tokio::test]
async fn multi_drop_numbering_is_refused_by_the_tables_across_two_rows() {
    let scratch = valombre("multi_drop_numbering").await;
    let served = bench().postgres(&scratch).await;
    applied(&served, "test.insert", INSTANCE, instance(INSTANCE, SCENE)).await;
    applied(
        &served,
        "test.insert",
        OTHER_INSTANCE,
        instance(OTHER_INSTANCE, OTHER_SCENE),
    )
    .await;
    applied(
        &served,
        "test.insert",
        TOKEN_1,
        token(TOKEN_1, INSTANCE, BANDIT, 1),
    )
    .await;

    rejected(
        &scratch,
        &served,
        "test.insert",
        TOKEN_2,
        token(TOKEN_2, INSTANCE, BANDIT, 1),
    )
    .await;
    assert_eq!(count(&scratch, "token").await, 1);
    assert_eq!(
        duplicate_of(
            &scratch,
            &format!(
                "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                     hp_current, hp_max, conditions)
                 VALUES ('{TOKEN_2}', '{INSTANCE}', '{BANDIT}', 1, 'Jeton 1', 0, 0, true, 11, 11, '{{}}')"
            )
        )
        .await,
        "token_instance_character_number"
    );

    // The number 1 of another character, and of the same one elsewhere.
    applied(
        &served,
        "test.insert",
        TOKEN_3,
        token(TOKEN_3, INSTANCE, ARCHER, 1),
    )
    .await;
    applied(
        &served,
        "test.insert",
        TOKEN_4,
        token(TOKEN_4, OTHER_INSTANCE, BANDIT, 1),
    )
    .await;
    assert_eq!(count(&scratch, "token").await, 3);
    // A PC's token carries no number, so this index does not tell two of them
    // apart: its roster is migration 0015's (`tests/pj_roster.rs`). One PC on
    // two instances is accepted.
    scratch
        .seed(&format!(
            "INSERT INTO token (id, instance_id, character_id, name, q, r, visible, conditions)
             VALUES (gen_random_uuid(), '{INSTANCE}', '{ELOWEN}', 'Elowen', 0, 0, true, '{{}}'),
                    (gen_random_uuid(), '{OTHER_INSTANCE}', '{ELOWEN}', 'Elowen', 1, 0, true, '{{}}')"
        ))
        .await;
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

// AC9: rows only the engine holds that the indexes refuse — a scene's second
// instance, an instance's second running combat, a token's number taken twice
// — are logged and skipped by a start, which projects the others and opens.
#[tokio::test]
async fn a_start_logs_and_skips_the_partitions_the_unique_indexes_refuse() {
    let scratch = valombre("warm_duplicates").await;
    // A first start makes the engine's own tables; the instance of the forest
    // is applied, in the engine and in the tables, and the engine closes.
    let served = bench().postgres(&scratch).await;
    applied(
        &served,
        "test.insert",
        OTHER_INSTANCE,
        instance(OTHER_INSTANCE, OTHER_SCENE),
    )
    .await;
    drop(served);
    // Two instances of the ford; on the forest's instance, two combats
    // running and three tokens, two of them numbered alike: the engine's rows
    // alone, which the tables lost.
    for (id, scene) in [(INSTANCE, SCENE), (SECOND_INSTANCE, SCENE)] {
        engine_only(&scratch, id, &instance(id, scene)).await;
    }
    for id in [COMBAT, SECOND_COMBAT] {
        engine_only(&scratch, id, &combat(id, OTHER_INSTANCE)).await;
    }
    for (id, character) in [(TOKEN_1, BANDIT), (TOKEN_2, BANDIT), (TOKEN_3, ARCHER)] {
        engine_only(&scratch, id, &token(id, OTHER_INSTANCE, character, 1)).await;
    }

    // The start does not fail: each partition a refused write leaves is
    // logged, the others are projected.
    let served = bench().postgres(&scratch).await;

    // The partitions land in order, so the first of each pair does and the
    // second does not.
    let held = |table: &'static str| {
        let scratch = &scratch;
        async move {
            scratch
                .rows(&format!("SELECT id::text FROM {table} ORDER BY id"), &[])
                .await
                .iter()
                .map(|row| row["id"].as_str().expect("an id").to_owned())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(held("battlemap_instance").await, [INSTANCE, OTHER_INSTANCE]);
    assert_eq!(held("combat").await, [COMBAT]);
    assert_eq!(held("token").await, [TOKEN_1, TOKEN_3]);
    // The engine goes on over the rows the start projected.
    applied(&served, "test.update", TOKEN_1, json!({ "q": 5 })).await;
}
