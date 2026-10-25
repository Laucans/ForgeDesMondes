//! The `speech-to-text` port behind one handle a server shares with every
//! unit: [`SharedSpeechToText`].
//!
//! [`SpeechToText`] answers with `impl Future`, so it is not dyn-compatible:
//! a unit cannot read a `dyn SpeechToText` from the server's `Shared`, which
//! holds one value per type. The handle boxes each future once, so one type
//! stands for any port: the fake, an adapter. It implements [`SpeechToText`]
//! itself, so the code a unit calls with a port takes it unchanged, and every
//! clone of it is the same port, with the same journal.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{
    Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, TranscriptStream,
    Vocabulary,
};

/// A future of the port, boxed.
type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// [`SpeechToText`], with its future boxed: what a handle holds as `dyn`.
trait Erased: Send + Sync {
    fn transcribe<'a>(
        &'a self,
        dictation: &'a Dictation,
        vocabulary: &'a Vocabulary,
    ) -> Boxed<'a, Result<Transcript, SpeechToTextError>>;

    fn listen<'a>(
        &'a self,
        vocabulary: &'a Vocabulary,
    ) -> Boxed<'a, Result<TranscriptStream, SpeechToTextError>>;

    fn journal(&self) -> &RequestJournal;
}

impl<P: SpeechToText> Erased for P {
    fn transcribe<'a>(
        &'a self,
        dictation: &'a Dictation,
        vocabulary: &'a Vocabulary,
    ) -> Boxed<'a, Result<Transcript, SpeechToTextError>> {
        Box::pin(SpeechToText::transcribe(self, dictation, vocabulary))
    }

    fn listen<'a>(
        &'a self,
        vocabulary: &'a Vocabulary,
    ) -> Boxed<'a, Result<TranscriptStream, SpeechToTextError>> {
        Box::pin(SpeechToText::listen(self, vocabulary))
    }

    fn journal(&self) -> &RequestJournal {
        SpeechToText::journal(self)
    }
}

/// The `speech-to-text` port a server hands its units: one port behind a
/// handle every clone of which is that port.
#[derive(Clone)]
pub struct SharedSpeechToText(Arc<dyn Erased>);

impl SharedSpeechToText {
    /// The handle on `port`.
    #[must_use]
    pub fn new<P: SpeechToText + 'static>(port: P) -> Self {
        Self(Arc::new(port))
    }
}

impl SpeechToText for SharedSpeechToText {
    async fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        self.0.transcribe(dictation, vocabulary).await
    }

    async fn listen(&self, vocabulary: &Vocabulary) -> Result<TranscriptStream, SpeechToTextError> {
        self.0.listen(vocabulary).await
    }

    fn journal(&self) -> &RequestJournal {
        self.0.journal()
    }
}
