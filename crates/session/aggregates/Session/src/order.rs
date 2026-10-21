//! A session's scene order (`crates/data/migrations/0006_session.sql`,
//! `scene.position`), rewritten whole by [`SceneOrder`] inside the
//! transaction that applies it.
//!
//! The order is a row of its own, `Session/<order id>` ([`order_id`]),
//! whose `scenePosition` is the session's scenes' ids in the order they
//! play. Each write of it is a reorder: two reorders of one session queue
//! on it, and no other write of the session — its own row's, a scene's —
//! carries an order that a later write would replay.
//!
//! The ranks it leaves go from 1 to n whatever was applied since the order
//! was asked (`scene-order-contiguous`): every scene of the session takes
//! its rank in one statement, the scenes the order lists first, in its
//! order, then any added since, in their ranks; a scene listed and removed
//! since is no longer ranked. The order itself — every scene once, none of
//! another session — is the DataCapability's to judge when it is submitted.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

use crate::SCENE_POSITION;

/// The partitions of the aggregate: `Session/<id>`.
const PARTITION: &str = "Session/";

/// Where a uuid writes its version, and the bit an order id flips there.
const VERSION_AT: usize = 14;
const ORDER_BIT: u32 = 8;

/// The session `$1` names, its row locked until the commit: a scene added
/// to it waits for its ranks, or they for it.
const LOCK_SESSION: &str = "SELECT 1 FROM session WHERE id = $1::text::uuid FOR UPDATE";

/// Every scene of the session `$1` at its rank: those the order `$2` lists
/// in its order, then the others in their ranks.
const REORDER: &str = "UPDATE scene s SET position = ranked.rank::integer
    FROM (SELECT sc.id, row_number() OVER (
                ORDER BY listed.rank NULLS LAST, sc.position, sc.id) AS rank
            FROM scene sc
            LEFT JOIN unnest($2::text[]) WITH ORDINALITY AS listed(id, rank)
                ON listed.id = sc.id::text
            WHERE sc.session_id = $1::text::uuid) ranked
    WHERE s.id = ranked.id";

/// The id of the row the scene order of `session` is written on — and, given
/// that id, the session's: the uuid with its version digit's high bit
/// flipped, a version no row of the product is written with (theirs are 4
/// and 7). `None` when `id` is no uuid.
#[must_use]
pub fn order_id(id: &str) -> Option<String> {
    if !is_uuid(id) {
        return None;
    }
    id.char_indices()
        .map(|(at, c)| match at {
            VERSION_AT => c
                .to_digit(16)
                .and_then(|digit| char::from_digit(digit ^ ORDER_BIT, 16)),
            _ => Some(c.to_ascii_lowercase()),
        })
        .collect()
}

/// A uuid written 8-4-4-4-12 in hex digits, in either case.
pub(crate) fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The Session aggregate's projection of its scene orders into `scene`.
pub struct SceneOrder;

#[async_trait]
impl ProjectPg for SceneOrder {
    /// The ranks an order leaves. A row that holds no order — a session's,
    /// a scene's — or a tombstone writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some((session, order)) = ordered(write) else {
            return Ok(());
        };
        tx.execute(LOCK_SESSION, &[&session]).await?;
        tx.execute(REORDER, &[&session, &order]).await?;
        Ok(())
    }
}

/// The session an order's write is of, and the scenes' ids it lists,
/// lowercase as the tables write them: `None` for a tombstone, a row that
/// holds no order, or one whose partition is no order id.
fn ordered(write: &Write) -> Option<(String, Vec<String>)> {
    if write.tombstone {
        return None;
    }
    let order = write.values.get(SCENE_POSITION)?.as_array()?;
    let session = order_id(write.partition.strip_prefix(PARTITION)?)?;
    let order = order
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_ascii_lowercase)
        .collect();
    Some((session, order))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_dataguard::store::Write;
    use serde_json::{Value, json};

    use super::{is_uuid, order_id, ordered};

    const SEANCE: &str = "51000000-0000-4000-8000-000000000004";

    fn write(values: Value, tombstone: bool) -> Write {
        on(
            "Session/51000000-0000-c000-8000-000000000004",
            values,
            tombstone,
        )
    }

    fn on(partition: &str, values: Value, tombstone: bool) -> Write {
        let Value::Object(values) = values else {
            panic!("a row is an object");
        };
        Write {
            partition: partition.to_owned(),
            values: values.into_iter().collect::<BTreeMap<_, _>>(),
            tombstone,
        }
    }

    #[test]
    fn an_order_id_flips_the_version_digit_and_flips_back_to_its_session() {
        assert_eq!(
            order_id(SEANCE).as_deref(),
            Some("51000000-0000-c000-8000-000000000004")
        );
        assert_eq!(
            order_id("51000000-0000-c000-8000-000000000004").as_deref(),
            Some(SEANCE)
        );
        // A v7 scene's id, written in capitals: lowercase, its version 7 → f.
        assert_eq!(
            order_id("0192AB00-0000-7ABC-8DEF-0123456789AB").as_deref(),
            Some("0192ab00-0000-fabc-8def-0123456789ab")
        );
        for not_one in [
            "",
            "seance-4",
            "51000000-0000-4000-8000-00000000000",
            "51000000x0000-4000-8000-000000000004",
            "51000000-0000-g000-8000-000000000004",
        ] {
            assert_eq!(order_id(not_one), None, "{not_one}");
            assert!(!is_uuid(not_one), "{not_one}");
        }
    }

    #[test]
    fn only_a_row_holding_an_order_is_one_and_never_its_tombstone() {
        let order = json!({ "scenePosition": ["5C000000-0000-4000-8000-000000000002", "5c000000-0000-4000-8000-000000000001"] });
        assert_eq!(
            ordered(&write(order.clone(), false)),
            Some((
                SEANCE.to_owned(),
                vec![
                    "5c000000-0000-4000-8000-000000000002".to_owned(),
                    "5c000000-0000-4000-8000-000000000001".to_owned()
                ]
            ))
        );
        assert_eq!(ordered(&write(order.clone(), true)), None);
        // A partition that is no order id is no session's order.
        for partition in [
            "Session/seance-4",
            "Entity/51000000-0000-c000-8000-000000000004",
        ] {
            assert_eq!(
                ordered(&on(partition, order.clone(), false)),
                None,
                "{partition}"
            );
        }
        // A scene's rank is no order, nor is a row without one.
        assert_eq!(ordered(&write(json!({ "scenePosition": 2 }), false)), None);
        assert_eq!(
            ordered(&write(json!({ "sessionStatus": "live" }), false)),
            None
        );
    }
}
