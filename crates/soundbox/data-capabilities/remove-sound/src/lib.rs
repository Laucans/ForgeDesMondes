//! DataCapability `soundbox.removeSound` (`data-capability.json`): the DM
//! removes a sound they no longer want from the library. The removal is one
//! command on `Sound/<its id>`, the sound's partition, of effect `delete`:
//! the engine's soft delete. Its row is kept as a tombstone, the sound as it
//! was, so a removal made by mistake can be undone (no command restores one
//! yet); the library's readers no longer see it, and its playbacks follow
//! the cascade `SoundPlayback.soundId` declares.
//!
//! A removal names nothing but its target: [`target`] reads the sound it
//! names, and whatever the wire's payload holds is not written. One naming
//! no sound the product holds — none, one that is no uuid, one removed
//! already — is refused before anything is queued ([`Refusal`], 404).
//!
//! Its `Prepare` ([`plugin`]) reads the sound as the queue leaves it. Its
//! projection ([`tables`]) takes the removed sound's row out of `sound` in
//! the transaction that applies the tombstone, and lends the engine a sound
//! only the tables hold — the fixtures' — so that it, too, can be removed.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

pub mod tables;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.removeSound";

/// Its target aggregate, as `data-capability.json` names it.
const AGGREGATE: &str = "Sound";

/// The partitions of its target aggregate: `Sound/<sound id>`.
const PARTITION: &str = "Sound/";

/// Why a removal is refused before it is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It names no sound, or one the product does not hold.
    NoSound,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSound => "ce son n'existe pas",
        })
    }
}

/// The sound a removal names — the command's target — trimmed and lowercase
/// as the `sound` table writes its ids.
///
/// # Errors
///
/// [`Refusal::NoSound`] when no sound is named, or one that is no uuid: no
/// sound of the product is named so.
pub fn target(target: Option<&str>) -> Result<String, Refusal> {
    target
        .and_then(|id| uuid(id.trim()))
        .ok_or(Refusal::NoSound)
}

/// `text` as a uuid, lowercase as the tables write it: 8-4-4-4-12
/// hexadecimal digits; `None` when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    let shaped = text.len() == 36
        && text.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    shaped.then(|| text.to_ascii_lowercase())
}

/// What the DataCapability plugs into the engine: its `Prepare`, and the
/// projection of a removed sound out of `sound`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, RemoveSound)
        .project_pg(AGGREGATE, tables::Removals)
}

/// A removal made the tombstone of the sound it names: one naming no sound
/// the queue leaves — none, or one removed already — is refused, 404.
struct RemoveSound;

#[async_trait]
impl Prepare for RemoveSound {
    async fn prepare(
        &self,
        context: &Context<'_>,
        _payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let sound = target(target_id).map_err(refused)?;
        if context
            .row(&format!("{PARTITION}{sound}"))
            .await?
            .is_empty()
        {
            return Err(refused(Refusal::NoSound));
        }
        Ok(plugins::Prepared {
            target_id: Some(sound),
            ..plugins::Prepared::default()
        })
    }
}

/// A removal refused: the sound it names is not found.
fn refused(refusal: Refusal) -> plugins::Refusal {
    plugins::Refusal::not_found(refusal.to_string())
}
