//! DataCapability `soundbox.removeSound` (`data-capability.json`): the DM
//! removes a sound they no longer want from the library. The removal is one
//! command on `Sound/<its id>`, the sound's partition, of effect `delete`:
//! the engine's soft delete. Its row is kept as a tombstone, the sound as it
//! was, so the removal can be undone; the library's readers no longer see
//! it, and its playbacks follow the cascade `SoundPlayback.soundId`
//! declares.
//!
//! A removal names nothing but its target: [`target`] reads the sound it
//! names; the tombstone is the sound as the engine held it, whatever the
//! wire's payload holds. One naming no sound is refused before
//! anything is queued ([`Refusal`]). Whether the sound exists is the
//! server's to ask, against the rows it holds: one removed already is none.

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.removeSound";

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

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The sound a removal names — the command's target — trimmed and lowercase
/// as the `sound` table writes its ids.
///
/// # Errors
///
/// [`Refusal::NoSound`] when no sound is named.
pub fn target(target: Option<&str>) -> Result<String, Refusal> {
    target
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .ok_or(Refusal::NoSound)
}
