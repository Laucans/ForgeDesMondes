//! The rows the engine holds, as the wire's callers read them.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::Result;
use harness_dataguard::command::Entry;
use harness_dataguard::store::Store;
use serde_json::Value;

/// The rows the engine holds, as a command about to be submitted sees them.
#[derive(Clone)]
pub struct Rows {
    store: Arc<dyn Store>,
}

impl Rows {
    /// The rows of `store`.
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// `partition`'s row as the commands queued on it will leave it: the row
    /// applied, then each pending command's payload, in queue order.
    ///
    /// # Errors
    ///
    /// The store's.
    pub async fn projected(&self, partition: &str) -> Result<BTreeMap<String, Value>> {
        let mut row = self.store.row(partition).await?;
        let mut ahead: Vec<Entry> = self
            .store
            .pending()
            .await?
            .into_iter()
            .filter(|entry| entry.partition == partition)
            .collect();
        ahead.sort_by_key(|entry| entry.position);
        for entry in ahead {
            row.extend(entry.payload);
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use harness_dataguard::command::{CommandResult, State};
    use harness_dataguard::memory::MemoryStore;
    use harness_dataguard::store::Write;
    use serde_json::json;

    use super::*;

    const MORLOX: &str = "Entity/e1000000-0000-4000-8000-000000000003";

    fn write(values: Value, tombstone: bool) -> Write {
        let Value::Object(values) = values else {
            panic!("a row is an object");
        };
        Write {
            partition: MORLOX.to_string(),
            values: values.into_iter().collect(),
            tombstone,
        }
    }

    fn result() -> CommandResult {
        CommandResult {
            command_id: "c_1".to_string(),
            status: State::Applied,
            data_version: None,
            violations: Vec::new(),
            review_id: None,
            attachment: None,
        }
    }

    /// A command pending on `partition`, at `position`, writing `payload`.
    fn pending(command: &str, partition: &str, position: u64, payload: Value) -> Entry {
        serde_json::from_value(json!({
            "command": command,
            "data_capability": "campaign.saveEntity@1",
            "by": "dm",
            "partition": partition,
            "position": position,
            "based_on": { "version": 0 },
            "payload": payload,
            "effect": "upsert",
            "touches": ["Entity.name"],
            "mode": "confirm_on_stale",
            "projection": { "confirmed": {}, "pending_ahead": [] },
            "state": "queued",
        }))
        .expect("an entry")
    }

    #[tokio::test]
    async fn a_row_projected_is_the_applied_one_and_its_own_partition_s_commands_in_order() {
        let inner = Arc::new(MemoryStore::default());
        let store: Arc<dyn Store> = inner.clone();
        store
            .apply(
                &write(json!({ "name": "Morlox", "notes": "applied" }), false),
                &result(),
            )
            .await
            .expect("applied");
        for entry in [
            pending("c_3", MORLOX, 3, json!({ "notes": "last" })),
            pending("c_2", MORLOX, 2, json!({ "notes": "first", "hpMax": 21 })),
            pending("c_9", "Entity/another", 1, json!({ "name": "Bandit" })),
        ] {
            store.save(&entry).await.expect("saved");
        }
        let row = Rows::new(store).projected(MORLOX).await.expect("projected");
        assert_eq!(row["name"], "Morlox");
        assert_eq!(row["notes"], "last");
        assert_eq!(row["hpMax"], 21);
    }
}
