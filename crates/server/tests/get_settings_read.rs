//! `ai.getSettings` read from the tables the AiSettings migration builds
//! (`DATABASE_URL`), through the connection `Reads` opens: the SQL of the
//! Capability meets the real columns. A scratch database of its own, dropped
//! when the test ends.

mod common;

use common::Reads;
use dataguard_server::engine::tokio_postgres::{self, NoTls};
use serde_json::json;

const MIGRATION: &str = include_str!("../../data/migrations/0001_ai_settings.sql");

const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
const HOME: &str = "a1000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

/// `url()` naming database `name`.
fn named(name: &str) -> String {
    let url = url();
    let (base, query) = url
        .split_once('?')
        .map_or((url.as_str(), ""), |(b, q)| (b, q));
    let (server, _) = base.rsplit_once('/').expect("a database in the URL");
    if query.is_empty() {
        format!("{server}/{name}")
    } else {
        format!("{server}/{name}?{query}")
    }
}

async fn connect(url: &str) -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

#[tokio::test]
async fn the_settings_are_read_from_the_tables_the_migration_builds() {
    let name = format!("get_settings_read_{}", std::process::id());
    let admin = connect(&url()).await;
    for sql in [
        format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        format!("CREATE DATABASE {name}"),
    ] {
        admin.batch_execute(&sql).await.expect("a scratch database");
    }
    let scratch = connect(&named(&name)).await;
    scratch.batch_execute(MIGRATION).await.expect("migrated");
    let reads = Reads::open(&named(&name)).await.expect("the reads open");

    let seeded = reads
        .run("ai.getSettings", &json!({}))
        .await
        .expect("an answer");
    assert_eq!(seeded["provider"], "cloud", "{seeded}");
    assert_eq!(seeded["lastTest"], json!(null), "{seeded}");
    assert_eq!(
        seeded["providers"]["opencode"]["url"],
        "http://192.168.1.40:4096"
    );
    assert_eq!(
        seeded["binaries"]["claude-code"]["label"],
        "Binaire Claude Code"
    );

    scratch
        .batch_execute(&format!(
            "UPDATE ai_provider SET api_key = 'sk-ant-api03-SECRETSECRETSECRET3f9c' WHERE id = '{CLOUD}';
             UPDATE ai_provider SET last_test_ok = true, last_test_at = '2026-10-17T10:00:01.250Z',
                 last_test_message = 'Connexion réussie', last_test_latency_ms = 840 WHERE id = '{HOME}';
             UPDATE ai_provider SET last_test_ok = false, last_test_at = '2026-10-17T10:00:01.750Z',
                 last_test_message = 'type d''API non reconnu' WHERE id = '{CLOUD}'"
        ))
        .await
        .expect("a key and two tests");
    let read = reads
        .run("ai.getSettings", &json!({}))
        .await
        .expect("an answer");
    assert_eq!(
        read["providers"]["cloud"]["key"],
        "sk-ant-••••••••••••••••3f9c"
    );
    assert!(!read.to_string().contains("SECRETSECRET"), "{read}");
    assert_eq!(
        read["lastTest"],
        json!({
            "ok": false,
            "at": "2026-10-17T10:00:01Z",
            "provider": "cloud",
            "latency": null,
            "message": "type d'API non reconnu",
        })
    );
    assert_eq!(read["testResults"]["opencode"]["ok"], true);

    drop(scratch);
    admin
        .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .await
        .expect("the scratch database dropped");
}
