//! The `character-sheets` port behind one handle a server shares with every
//! unit: [`SharedCharacterSheets`].
//!
//! [`CharacterSheets`] answers with `impl Future`, so it is not
//! dyn-compatible: a unit cannot read a `dyn CharacterSheets` from the
//! server's `Shared`, which holds one value per type. The handle boxes each
//! future once, so one type stands for any port: the fake, an adapter. It
//! implements [`CharacterSheets`] itself, so the code a unit calls with a port
//! takes it unchanged, and every clone of it is the same port.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError};

/// A future of the port, boxed.
type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// [`CharacterSheets`], with its future boxed: what a handle holds as `dyn`.
trait Erased: Send + Sync {
    fn fetch(&self, id: CharacterId) -> Boxed<'_, Result<CharacterSheet, CharacterSheetsError>>;
}

impl<P: CharacterSheets> Erased for P {
    fn fetch(&self, id: CharacterId) -> Boxed<'_, Result<CharacterSheet, CharacterSheetsError>> {
        Box::pin(CharacterSheets::fetch(self, id))
    }
}

/// The `character-sheets` port a server hands its units: one port behind a
/// handle every clone of which is that port.
#[derive(Clone)]
pub struct SharedCharacterSheets(Arc<dyn Erased>);

impl SharedCharacterSheets {
    /// The handle on `port`.
    #[must_use]
    pub fn new<P: CharacterSheets + 'static>(port: P) -> Self {
        Self(Arc::new(port))
    }
}

impl CharacterSheets for SharedCharacterSheets {
    async fn fetch(&self, id: CharacterId) -> Result<CharacterSheet, CharacterSheetsError> {
        self.0.fetch(id).await
    }
}
