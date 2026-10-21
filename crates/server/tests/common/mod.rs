//! What the read tests share: the registry's reads as the server opens them.

use std::collections::BTreeMap;

use dataguard_server::engine::Result;
use serde_json::Value;

/// Every Capability of the registry over the database a URL names, as the
/// server reads it (`server::reads`), asked with the variables of a read's
/// body.
pub struct Reads(dataguard_server::engine::reads::Reads);

impl Reads {
    /// The reads over the database `url` names.
    pub async fn open(url: &str) -> std::result::Result<Self, server::Error> {
        Ok(Self(server::reads(url).await?))
    }

    /// What the Capability `id` answers to `variables`, an object.
    pub async fn run(&self, id: &str, variables: &Value) -> Result<Value> {
        let variables: BTreeMap<String, Value> = variables
            .as_object()
            .map(|given| given.clone().into_iter().collect())
            .unwrap_or_default();
        self.0.run(id, &variables).await
    }
}
