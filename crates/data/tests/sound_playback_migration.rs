//! The SoundPlayback aggregate on its table
//! (`crates/data/migrations/0009_sound_playback.sql`, laid by the schema
//! task) against the harness's PostgreSQL (`DATABASE_URL`): several
//! playbacks of a session play at once, « Tout arrêter » stops those still
//! playing and them alone, a playback is the DM view's and reaches nothing
//! of the player view, and each relation deletes and updates as the
//! aggregate declares. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::Path;

use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 1, live.
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
/// Valombre's session 2, live in another tab.
const AUTRE_SEANCE: &str = "5e000000-0000-4000-8000-000000000002";
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000002";

/// The statement « Tout arrêter » stands for: every playback of `$1` still
/// playing stops now.
const STOP_ALL: &str = "UPDATE sound_playback SET stopped_at = now() \
                        WHERE session_id = $1::text::uuid AND stopped_at IS NULL";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated by the Data layer, with Valombre, its two
/// sessions and two sounds of its soundbox — and nothing playing yet.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("sound_playback_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        let migrations = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
        data::migrate(&mut client, Path::new(migrations))
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', false), \
                 ('{AUTRE_SEANCE}', '{VALOMBRE}', 2, 'La Crypte', 'live', false); \
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES \
                 ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg'), \
                 ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg')"
            ))
            .await
            .expect("a campaign, its sessions and its sounds");
        Scratch { client, name }
    }

    /// `sound` played in `session` from `origin`.
    async fn play(&self, session: &str, sound: &str, origin: &str) {
        self.client
            .execute(
                "INSERT INTO sound_playback (session_id, sound_id, origin, started_at) \
                 VALUES ($1::text::uuid, $2::text::uuid, $3, now())",
                &[&session, &sound, &origin],
            )
            .await
            .expect("a playback added");
    }

    /// `session`'s playbacks: the sound's name, and whether it still plays,
    /// sorted.
    async fn playbacks(&self, session: &str) -> Vec<(String, bool)> {
        self.client
            .query(
                "SELECT s.name, p.stopped_at IS NULL FROM sound_playback p \
                 JOIN sound s ON s.id = p.sound_id \
                 WHERE p.session_id = $1::text::uuid ORDER BY 1, 2",
                &[&session],
            )
            .await
            .expect("the playbacks read")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// The instants `session`'s stopped playbacks stopped at, in UTC, as
    /// text, in order.
    async fn stops(&self, session: &str) -> Vec<String> {
        self.client
            .query(
                "SELECT (stopped_at AT TIME ZONE 'UTC')::text FROM sound_playback \
                 WHERE session_id = $1::text::uuid AND stopped_at IS NOT NULL \
                 ORDER BY stopped_at",
                &[&session],
            )
            .await
            .expect("the stops read")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn count(&self, sql: &str) -> i64 {
        self.client
            .query_one(sql, &[])
            .await
            .expect(sql)
            .get::<_, i64>(0)
    }

    /// The constraint `sql` is refused under.
    async fn refused_by(&self, sql: &str) -> (SqlState, Option<String>) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        (db.code().clone(), db.constraint().map(str::to_string))
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        connect(&config)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

fn playing(name: &str) -> (String, bool) {
    (name.to_string(), true)
}

fn stopped(name: &str) -> (String, bool) {
    (name.to_string(), false)
}

#[tokio::test]
async fn simultaneous_playback_keeps_every_playback_of_a_session_playing() {
    let s = Scratch::new("simultaneous").await;
    // R111: the crowd from the soundbox, then the door from the pastille, and
    // the door again over it.
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(SEANCE, PORTE, "pastille").await;
    s.play(SEANCE, PORTE, "pastille").await;
    assert_eq!(
        s.playbacks(SEANCE).await,
        [
            playing("Bruit de foule"),
            playing("Porte qui grince"),
            playing("Porte qui grince")
        ]
    );
    // No uniqueness keeps two playbacks apart: the key is the only one.
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_index \
             WHERE indrelid = 'sound_playback'::regclass AND indisunique AND NOT indisprimary"
        )
        .await,
        0
    );
    s.drop().await;
}

#[tokio::test]
async fn stop_all_stops_the_session_s_playbacks_still_playing_and_them_alone() {
    let s = Scratch::new("stop_all").await;
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(SEANCE, PORTE, "pastille").await;
    s.play(AUTRE_SEANCE, FOULE, "soundbox").await;
    // The door stopped on its own, earlier.
    s.client
        .batch_execute(&format!(
            "UPDATE sound_playback SET stopped_at = '2026-01-01 20:00:00+00' \
             WHERE session_id = '{SEANCE}' AND sound_id = '{PORTE}'"
        ))
        .await
        .expect("the door stopped");
    s.play(SEANCE, PORTE, "pastille").await;
    let stopping = s
        .client
        .execute(STOP_ALL, &[&SEANCE])
        .await
        .expect("« Tout arrêter »");
    assert_eq!(stopping, 2, "the two still playing");
    assert_eq!(
        s.playbacks(SEANCE).await,
        [
            stopped("Bruit de foule"),
            stopped("Porte qui grince"),
            stopped("Porte qui grince")
        ]
    );
    // The door's first stop is kept, the others share theirs.
    let stops = s.stops(SEANCE).await;
    assert!(stops[0].starts_with("2026-01-01 20:00:00"), "{stops:?}");
    assert_eq!(stops[1], stops[2]);
    // The other session plays on.
    assert_eq!(s.playbacks(AUTRE_SEANCE).await, [playing("Bruit de foule")]);
    // Nothing is left to stop: a second « Tout arrêter » moves nothing.
    assert_eq!(s.client.execute(STOP_ALL, &[&SEANCE]).await.unwrap(), 0);
    assert_eq!(s.stops(SEANCE).await, stops);
    s.drop().await;
}

#[tokio::test]
async fn dm_side_only_refuses_a_playback_from_the_player_view() {
    let s = Scratch::new("dm_side_only").await;
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(SEANCE, PORTE, "pastille").await;
    for sql in [
        format!(
            "INSERT INTO sound_playback (session_id, sound_id, origin, started_at) \
             VALUES ('{SEANCE}', '{PORTE}', 'player', now())"
        ),
        "UPDATE sound_playback SET origin = 'joueur'".to_string(),
    ] {
        assert_eq!(
            s.refused_by(&sql).await,
            (
                SqlState::CHECK_VIOLATION,
                Some("sound_playback_origin".to_string())
            ),
            "{sql}"
        );
    }
    assert_eq!(
        s.refused_by(&format!(
            "INSERT INTO sound_playback (session_id, sound_id, started_at) \
             VALUES ('{SEANCE}', '{PORTE}', now())"
        ))
        .await
        .0,
        SqlState::NOT_NULL_VIOLATION
    );
    s.drop().await;
}

#[tokio::test]
async fn dm_side_only_leaves_the_player_view_nothing_to_reach_a_playback_by() {
    let s = Scratch::new("player_view").await;
    // No row of any table points to a playback, nor to a sound from what the
    // player view shows (diffusion).
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_constraint \
             WHERE contype = 'f' AND confrelid = 'sound_playback'::regclass"
        )
        .await,
        0
    );
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_constraint WHERE contype = 'f' \
             AND conrelid = 'diffusion'::regclass \
             AND confrelid IN ('sound_playback'::regclass, 'sound'::regclass)"
        )
        .await,
        0
    );
    // No view reads the playbacks.
    assert_eq!(
        s.count(
            "SELECT count(DISTINCT v.oid) FROM pg_depend d \
             JOIN pg_rewrite r ON r.oid = d.objid \
             JOIN pg_class v ON v.oid = r.ev_class \
             WHERE d.refobjid = 'sound_playback'::regclass AND v.oid <> d.refobjid"
        )
        .await,
        0
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_session_deletes_its_playbacks_and_theirs_alone() {
    let s = Scratch::new("session_cascade").await;
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(SEANCE, PORTE, "pastille").await;
    s.play(AUTRE_SEANCE, FOULE, "soundbox").await;
    s.client
        .batch_execute(&format!("DELETE FROM session WHERE id = '{SEANCE}'"))
        .await
        .expect("the session deleted");
    assert!(s.playbacks(SEANCE).await.is_empty());
    assert_eq!(s.playbacks(AUTRE_SEANCE).await, [playing("Bruit de foule")]);
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_sound_deletes_its_playbacks_and_theirs_alone() {
    let s = Scratch::new("sound_cascade").await;
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(SEANCE, PORTE, "pastille").await;
    s.play(AUTRE_SEANCE, PORTE, "soundbox").await;
    s.client
        .batch_execute(&format!("DELETE FROM sound WHERE id = '{PORTE}'"))
        .await
        .expect("the sound deleted");
    assert_eq!(s.playbacks(SEANCE).await, [playing("Bruit de foule")]);
    assert!(s.playbacks(AUTRE_SEANCE).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_session_s_and_a_sound_s_new_ids_are_propagated_to_their_playbacks() {
    let s = Scratch::new("propagate").await;
    s.play(SEANCE, FOULE, "soundbox").await;
    s.play(AUTRE_SEANCE, PORTE, "pastille").await;
    let new_session = "5e000000-0000-4000-8000-0000000000ff";
    let new_sound = "50000000-0000-4000-8000-0000000000ff";
    s.client
        .batch_execute(&format!(
            "UPDATE session SET id = '{new_session}' WHERE id = '{SEANCE}'; \
             UPDATE sound SET id = '{new_sound}' WHERE id = '{FOULE}'"
        ))
        .await
        .expect("the new ids");
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM sound_playback \
             WHERE session_id = '{new_session}' AND sound_id = '{new_sound}'"
        ))
        .await,
        1
    );
    assert_eq!(
        s.playbacks(AUTRE_SEANCE).await,
        [playing("Porte qui grince")]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_playback_is_of_a_session_and_a_sound_that_exist() {
    let s = Scratch::new("references").await;
    let ghost = "00000000-0000-4000-8000-000000000000";
    for (session, sound) in [(ghost, FOULE), (SEANCE, ghost)] {
        assert_eq!(
            s.refused_by(&format!(
                "INSERT INTO sound_playback (session_id, sound_id, origin, started_at) \
                 VALUES ('{session}', '{sound}', 'soundbox', now())"
            ))
            .await
            .0,
            SqlState::FOREIGN_KEY_VIOLATION
        );
    }
    s.drop().await;
}
