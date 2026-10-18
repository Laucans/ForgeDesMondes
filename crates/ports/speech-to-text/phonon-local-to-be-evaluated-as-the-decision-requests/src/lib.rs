//! The adapter of the `speech-to-text` port over a local Phonon sidecar.
//!
//! The sidecar is a program on the DM's machine; the audio goes to that
//! process and nowhere else — this crate holds no HTTP client and opens no
//! socket. [`PhononSidecar`] runs it for each dictation and speaks to it on
//! its standard streams:
//!
//! - **stdin**: one line of JSON, `{"language":"fr","vocabulary":{
//!   "campaign_names":[…],"aliases":[…],"sound_names":[…]}}`, then the raw
//!   audio until the stream closes. Nothing is handed to it as an argument, so
//!   the campaign's names are not readable in the process list.
//! - **stdout**: the French text it heard.
//!
//! The program to run comes from configuration ([`SIDECAR_VARIABLE`], or
//! [`PhononSidecar::at`]); none is hard-coded. A path with nothing runnable at
//! it, or no path at all, is a failure that says so, and nothing is journalled
//! for a request that never reached a started process. The sidecar is run from
//! the system's temporary directory and given up on after
//! [`TRANSCRIBE_TIMEOUT`].
//!
//! Judging how well Phonon hears French is not this crate's: it passes
//! [`ports_speech_to_text::contract`] on a stub program.

use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use ports_speech_to_text::{
    Dictation, LANGUAGE, RequestJournal, SpeechToText, SpeechToTextError, Transcript, Vocabulary,
};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// The environment variable that holds the path of the sidecar program.
pub const SIDECAR_VARIABLE: &str = "SPEECH_TO_TEXT_SIDECAR";

/// How long the sidecar has to answer one dictation.
pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// How much of the sidecar's standard error a reason keeps.
const DETAIL_LIMIT: usize = 200;

/// Why a transcription failed.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// No path was configured.
    NotConfigured,
    /// Nothing runnable is at the configured path.
    NotFound(PathBuf),
    /// It is there but could not be started.
    Unlaunchable(io::ErrorKind),
    /// It did not answer in time.
    TimedOut(Duration),
    /// It ended with an error: its exit code (none when a signal ended it)
    /// and what it said on its standard error.
    Exited { code: Option<i32>, detail: String },
    /// It answered nothing.
    Empty,
}

impl Failure {
    /// What the DM reads.
    fn reason(&self) -> String {
        match self {
            Self::NotConfigured => {
                format!("aucun sidecar de transcription configuré : renseignez {SIDECAR_VARIABLE}")
            }
            Self::NotFound(path) => format!(
                "programme introuvable : le sidecar de transcription n'est pas installé à « {} »",
                path.display()
            ),
            Self::Unlaunchable(kind) => {
                format!("impossible de lancer le sidecar de transcription : {kind}")
            }
            Self::TimedOut(limit) => {
                format!(
                    "le sidecar de transcription n'a pas répondu en {} s",
                    limit.as_secs_f64().ceil()
                )
            }
            Self::Exited { code, detail } => {
                let ended = match code {
                    Some(code) => format!("le sidecar de transcription a échoué (code {code})"),
                    None => "le sidecar de transcription a été interrompu".to_owned(),
                };
                if detail.is_empty() {
                    ended
                } else {
                    format!("{ended} : {detail}")
                }
            }
            Self::Empty => "aucun texte reconnu dans la dictée".to_owned(),
        }
    }
}

/// The `speech-to-text` port over the local Phonon sidecar.
#[derive(Clone, Debug)]
pub struct PhononSidecar {
    /// The program to run; `None` when none is configured.
    path: Option<PathBuf>,
    timeout: Duration,
    journal: RequestJournal,
}

impl PhononSidecar {
    /// An adapter running the program at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self::configured(Some(path.into()))
    }

    /// An adapter running the program [`SIDECAR_VARIABLE`] names. With no
    /// value (or an empty one) every transcription fails, saying so.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::from_setting(env::var_os(SIDECAR_VARIABLE))
    }

    /// An adapter running the program `setting` names — what
    /// [`SIDECAR_VARIABLE`] holds, or `None` when it holds nothing.
    #[must_use]
    pub fn from_setting(setting: Option<OsString>) -> Self {
        Self::configured(setting.filter(|value| !value.is_empty()).map(PathBuf::from))
    }

    fn configured(path: Option<PathBuf>) -> Self {
        Self {
            path,
            timeout: TRANSCRIBE_TIMEOUT,
            journal: RequestJournal::new(),
        }
    }

    /// The same adapter, giving up on the sidecar after `timeout`.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs the sidecar on `dictation`, journalling the request once the
    /// process is started, and returns what it printed.
    async fn run(&self, dictation: &Dictation, vocabulary: &Vocabulary) -> Result<String, Failure> {
        let path = self.path.as_deref().ok_or(Failure::NotConfigured)?;
        if !is_executable(path) {
            return Err(Failure::NotFound(path.to_owned()));
        }
        let mut child = command(path)
            .spawn()
            .map_err(|error| Failure::Unlaunchable(error.kind()))?;
        self.journal.record(dictation, vocabulary);
        let stdin = child.stdin.take();
        let header = header(vocabulary);
        // The audio goes in while the text is read, so a sidecar that answers
        // before it has read everything cannot block on a full pipe. One that
        // stops reading is its own business: its exit says how it went.
        let feed = async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(header.as_bytes()).await;
                let _ = stdin.write_all(&dictation.audio).await;
            }
        };
        let finished = tokio::time::timeout(self.timeout, async {
            tokio::join!(feed, child.wait_with_output()).1
        })
        .await;
        // On a timeout the child is dropped with the future: it is killed.
        let output = finished
            .map_err(|_| Failure::TimedOut(self.timeout))?
            .map_err(|error| Failure::Unlaunchable(error.kind()))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            return Err(Failure::Exited {
                code: output.status.code(),
                detail: said.trim().chars().take(DETAIL_LIMIT).collect(),
            });
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() {
            return Err(Failure::Empty);
        }
        Ok(text)
    }
}

/// The line that opens the sidecar's standard input: the language and the
/// vocabulary, as passed. JSON escapes every newline, so it is one line.
fn header(vocabulary: &Vocabulary) -> String {
    let mut line = json!({
        "language": LANGUAGE,
        "vocabulary": {
            "campaign_names": vocabulary.campaign_names,
            "aliases": vocabulary.aliases,
            "sound_names": vocabulary.sound_names,
        },
    })
    .to_string();
    line.push('\n');
    line
}

/// The sidecar, ready to be started: a pipe for each of its three streams, and
/// nothing of the DM's working directory to read.
fn command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command
        .current_dir(env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// Whether `path` is a file that can be run.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

impl SpeechToText for PhononSidecar {
    async fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        if dictation.audio.is_empty() {
            return Err(SpeechToTextError::EmptyAudio);
        }
        self.run(dictation, vocabulary)
            .await
            .map(Transcript::new)
            .map_err(|failure| SpeechToTextError::Failed(failure.reason()))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_comes_from_the_setting_and_an_empty_one_is_none() {
        let set = PhononSidecar::from_setting(Some("/opt/phonon/sidecar".into()));
        assert_eq!(set.path.as_deref(), Some(Path::new("/opt/phonon/sidecar")));
        assert_eq!(
            PhononSidecar::from_setting(Some(OsString::new())).path,
            None
        );
        assert_eq!(PhononSidecar::from_setting(None).path, None);
        assert_eq!(set.timeout, TRANSCRIBE_TIMEOUT);
    }

    #[test]
    fn the_header_is_one_line_carrying_the_language_and_the_vocabulary_as_passed() {
        let vocabulary = Vocabulary {
            campaign_names: vec!["Élowen".into(), "ligne\nbrisée".into()],
            aliases: vec!["la Louve".into()],
            sound_names: vec![],
        };
        let line = header(&vocabulary);
        assert!(
            line.ends_with('\n') && line.matches('\n').count() == 1,
            "{line:?}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            parsed,
            json!({"language": "fr", "vocabulary": {
                "campaign_names": ["Élowen", "ligne\nbrisée"],
                "aliases": ["la Louve"],
                "sound_names": [],
            }})
        );
    }

    #[test]
    fn a_failure_reads_for_the_dm() {
        assert!(Failure::NotConfigured.reason().contains(SIDECAR_VARIABLE));
        let missing = Failure::NotFound(PathBuf::from("/nulle/part")).reason();
        assert!(
            missing.contains("introuvable") && missing.contains("/nulle/part"),
            "{missing}"
        );
        assert_eq!(
            Failure::Exited {
                code: Some(2),
                detail: "boom".into()
            }
            .reason(),
            "le sidecar de transcription a échoué (code 2) : boom"
        );
        assert_eq!(
            Failure::Exited {
                code: None,
                detail: String::new()
            }
            .reason(),
            "le sidecar de transcription a été interrompu"
        );
        assert_eq!(
            Failure::TimedOut(Duration::from_secs(30)).reason(),
            "le sidecar de transcription n'a pas répondu en 30 s"
        );
        assert_eq!(
            Failure::Empty.reason(),
            "aucun texte reconnu dans la dictée"
        );
        assert!(
            Failure::Unlaunchable(io::ErrorKind::PermissionDenied)
                .reason()
                .contains("permission")
        );
    }
}
