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
//! To listen ([`SpeechToText::listen`]) the sidecar is started once for the
//! whole stream, and the microphone is its own to open: the audio never reaches
//! this crate.
//!
//! - **stdin**: the same line of JSON, with `"mode":"stream"` added, and then
//!   nothing: the stream is stopped when the process is killed, so a sidecar
//!   that reads its input sees it close just before.
//! - **stdout**: one phrase per line, as it is heard; blank lines are skipped.
//!   The stream ends when the sidecar ends. A sidecar that ends with an error
//!   leaves that error as the last item of the stream, with what it said on its
//!   standard error. [`TRANSCRIBE_TIMEOUT`] does not apply: a session is long.
//!
//! The sidecar must read its input to the end. The program to run comes from
//! configuration ([`SIDECAR_VARIABLE`], or [`PhononSidecar::at`]); none is
//! hard-coded. It is a path — a relative one is taken from the working
//! directory of the process — never a name searched on `PATH`. A path with nothing runnable at
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

use ports_speech_to_text::stream::{Phrases, StopSignal};
use ports_speech_to_text::{
    Dictation, LANGUAGE, RequestJournal, SpeechToText, SpeechToTextError, Transcript,
    TranscriptStream, Vocabulary,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

/// The environment variable that holds the path of the sidecar program.
pub const SIDECAR_VARIABLE: &str = "SPEECH_TO_TEXT_SIDECAR";

/// How long the sidecar has to answer one dictation.
pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// How much of the sidecar's standard error a reason keeps.
const DETAIL_LIMIT: usize = 200;

/// How much of the sidecar's standard error a stream keeps to read at its end;
/// the rest is read and dropped, so a talkative sidecar never blocks on it.
const STDERR_KEPT: usize = 4096;

/// How long a stream waits for what a sidecar that ended still had to say on
/// its standard error.
const STDERR_PATIENCE: Duration = Duration::from_secs(1);

/// How long a stream gives a sidecar that closed its output to end.
const EXIT_PATIENCE: Duration = Duration::from_secs(2);

/// Why a transcription failed.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// No path was configured.
    NotConfigured,
    /// Nothing runnable is at the configured path.
    NotFound(PathBuf),
    /// It is there but could not be started.
    Unlaunchable(io::ErrorKind),
    /// It was started, and talking to it failed.
    Io(io::ErrorKind),
    /// It answered something that is not text.
    NotText,
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
            Self::Io(kind) => {
                format!("échange impossible avec le sidecar de transcription : {kind}")
            }
            Self::NotText => "le sidecar de transcription n'a pas répondu en texte".to_owned(),
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

    /// Starts the sidecar: a process with its three streams piped, not yet
    /// spoken to.
    fn start(&self) -> Result<Child, Failure> {
        let configured = self.path.as_deref().ok_or(Failure::NotConfigured)?;
        // The sidecar is run from another directory than the process's own: a
        // relative path is made absolute first, so the check and the start
        // look at the same file.
        let path =
            std::path::absolute(configured).map_err(|error| Failure::Unlaunchable(error.kind()))?;
        if !is_executable(&path) {
            return Err(Failure::NotFound(configured.to_owned()));
        }
        command(&path)
            .spawn()
            .map_err(|error| Failure::Unlaunchable(error.kind()))
    }

    /// Runs the sidecar on `dictation`, journalling the request once the
    /// process is started, and returns what it printed.
    async fn run(&self, dictation: &Dictation, vocabulary: &Vocabulary) -> Result<String, Failure> {
        let mut child = self.start()?;
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
            .map_err(|error| Failure::Io(error.kind()))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            return Err(Failure::Exited {
                code: output.status.code(),
                detail: said.trim().chars().take(DETAIL_LIMIT).collect(),
            });
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| Failure::NotText)?
            .trim()
            .to_owned();
        if text.is_empty() {
            return Err(Failure::Empty);
        }
        Ok(text)
    }
}

/// The line that opens the sidecar's standard input: the language and the
/// vocabulary, as passed. JSON escapes every newline, so it is one line.
fn header(vocabulary: &Vocabulary) -> String {
    line(&payload(vocabulary))
}

/// The line that opens the sidecar's standard input to listen: the same, and
/// the mode that tells the sidecar to open the microphone itself.
fn stream_header(vocabulary: &Vocabulary) -> String {
    let mut payload = payload(vocabulary);
    payload["mode"] = json!("stream");
    line(&payload)
}

fn payload(vocabulary: &Vocabulary) -> Value {
    json!({
        "language": LANGUAGE,
        "vocabulary": {
            "campaign_names": vocabulary.campaign_names,
            "aliases": vocabulary.aliases,
            "sound_names": vocabulary.sound_names,
        },
    })
}

fn line(payload: &Value) -> String {
    let mut line = payload.to_string();
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

    async fn listen(&self, vocabulary: &Vocabulary) -> Result<TranscriptStream, SpeechToTextError> {
        let reasoned = |failure: Failure| SpeechToTextError::Failed(failure.reason());
        let mut child = self.start().map_err(reasoned)?;
        // `command` pipes all three; the stream owns them from here.
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(reasoned(Failure::Io(io::ErrorKind::NotConnected)));
        };
        self.journal.record_listen(vocabulary);
        let pipes = Pipes {
            stdin,
            stdout,
            stderr,
        };
        let header = stream_header(vocabulary);
        Ok(TranscriptStream::spawn(move |phrases, stop| {
            pump(child, pipes, header, phrases, stop)
        }))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

/// How listening to the sidecar came to its end.
enum Ending {
    /// The caller stopped the stream, or went away.
    Stopped,
    /// The sidecar closed its output.
    Closed,
    /// Reading its output failed.
    Failed(Failure),
}

/// The three streams of a started sidecar.
struct Pipes {
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

/// The task of a stream: gives the sidecar its header, hands the caller each
/// phrase it prints, and at the end leaves no process behind.
async fn pump(
    mut child: Child,
    pipes: Pipes,
    header: String,
    phrases: Phrases,
    mut stop: StopSignal,
) {
    let Pipes {
        mut stdin,
        stdout,
        stderr,
    } = pipes;
    let mut output = BufReader::new(stdout).lines();
    let mut stderr = Some(stderr);
    let mut said = Vec::new();
    let mut chunk = [0u8; 512];

    // The header goes in while the output is read, as for a dictation: a
    // sidecar that talks before it has read its input cannot block on a full
    // pipe. Then the input stays open: one that stops reading is its own
    // business, its exit says how it went.
    let mut sending = std::pin::pin!(async move {
        let _ = stdin.write_all(header.as_bytes()).await;
        stdin
    });
    let mut stdin = None;
    let ending = loop {
        tokio::select! {
            () = stop.requested() => break Ending::Stopped,
            sent = &mut sending, if stdin.is_none() => stdin = Some(sent),
            read = output.next_line() => match read {
                Ok(Some(text)) => {
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    let heard = phrases.say(Transcript::new(text));
                    if stop.unless_stopped(heard).await != Some(true) {
                        break Ending::Stopped;
                    }
                }
                Ok(None) => break Ending::Closed,
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    break Ending::Failed(Failure::NotText);
                }
                Err(error) => break Ending::Failed(Failure::Io(error.kind())),
            },
            read = read_error(&mut stderr, &mut chunk) => keep(&mut said, &chunk[..read]),
        }
    };
    // Whatever the ending, the process is over before the task is.
    drop(stdin);
    let failure = match ending {
        Ending::Stopped => {
            let _ = child.kill().await;
            return;
        }
        Ending::Failed(failure) => {
            let _ = child.kill().await;
            failure
        }
        Ending::Closed => {
            // It said everything it had to say; it is given a moment to end.
            let waited = stop
                .unless_stopped(tokio::time::timeout(EXIT_PATIENCE, child.wait()))
                .await;
            let status = match waited {
                Some(Ok(status)) => status,
                // Stopped, or still there though it has said all: the stream
                // is over either way.
                Some(Err(_)) | None => {
                    let _ = child.kill().await;
                    return;
                }
            };
            if let Some(stderr) = stderr.as_mut() {
                let _ = tokio::time::timeout(STDERR_PATIENCE, async {
                    while let Ok(read @ 1..) = stderr.read(&mut chunk).await {
                        keep(&mut said, &chunk[..read]);
                    }
                })
                .await;
            }
            match status {
                Ok(status) if status.success() => return,
                Ok(status) => Failure::Exited {
                    code: status.code(),
                    detail: String::from_utf8_lossy(&said)
                        .trim()
                        .chars()
                        .take(DETAIL_LIMIT)
                        .collect(),
                },
                Err(error) => Failure::Io(error.kind()),
            }
        }
    };
    let failed = phrases.fail(SpeechToTextError::Failed(failure.reason()));
    let _ = stop.unless_stopped(failed).await;
}

/// Reads what the sidecar says on its standard error: how many bytes of
/// `chunk` are filled. Once it is closed, `stderr` is emptied and this waits
/// for ever, which leaves the rest of a `select!` to run.
async fn read_error(stderr: &mut Option<ChildStderr>, chunk: &mut [u8]) -> usize {
    if let Some(open) = stderr.as_mut() {
        if let Ok(read @ 1..) = open.read(chunk).await {
            return read;
        }
        *stderr = None;
    }
    std::future::pending().await
}

/// Keeps `read` in `said`, up to [`STDERR_KEPT`] bytes.
fn keep(said: &mut Vec<u8>, read: &[u8]) {
    let room = STDERR_KEPT.saturating_sub(said.len());
    said.extend_from_slice(&read[..read.len().min(room)]);
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
    fn the_environment_variable_is_the_setting() {
        let expected = env::var_os(SIDECAR_VARIABLE)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        assert_eq!(PhononSidecar::from_environment().path, expected);
    }

    #[test]
    fn the_configured_path_is_made_absolute_before_the_sidecar_is_run_elsewhere() {
        let relative = std::path::absolute("bin/phonon").unwrap();
        assert!(relative.is_absolute() && relative.ends_with("bin/phonon"));
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
    fn the_stream_header_is_the_header_asking_the_sidecar_to_listen() {
        let vocabulary = Vocabulary {
            campaign_names: vec!["Élowen".into()],
            aliases: vec![],
            sound_names: vec!["orage".into()],
        };
        let line = stream_header(&vocabulary);
        assert!(
            line.ends_with('\n') && line.matches('\n').count() == 1,
            "{line:?}"
        );
        let mut expected: serde_json::Value = serde_json::from_str(&header(&vocabulary)).unwrap();
        assert!(expected.get("mode").is_none(), "a dictation has no mode");
        expected["mode"] = json!("stream");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            expected
        );
    }

    #[test]
    fn what_a_stream_keeps_of_the_standard_error_is_bounded() {
        let mut said = Vec::new();
        keep(&mut said, b"boom");
        keep(&mut said, b"!");
        assert_eq!(said, b"boom!");
        keep(&mut said, &vec![b'x'; STDERR_KEPT]);
        assert_eq!(said.len(), STDERR_KEPT);
        keep(&mut said, b"more");
        assert_eq!(said.len(), STDERR_KEPT);
        assert!(said.starts_with(b"boom!x"));
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
        assert!(
            Failure::Io(io::ErrorKind::BrokenPipe)
                .reason()
                .contains("échange impossible")
        );
        assert!(Failure::NotText.reason().contains("texte"));
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
