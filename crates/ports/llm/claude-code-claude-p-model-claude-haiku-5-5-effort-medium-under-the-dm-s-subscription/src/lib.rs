//! The adapter of the `llm` port (`crates/ports/llm`) that `ports.json` names:
//! the DM's own `claude` program (Claude Code), under the DM's subscription.
//!
//! Every call — a generation of any kind, a summary, the connection test — runs
//! the same command, whose argument list [`claude_args`] builds in one place:
//! `claude -p --model claude-haiku-5-5 --effort medium`. The models a
//! [`ProviderConfig`] names are ignored, and so is the rest of it: there is one
//! provider, one model and one effort. The answer and the connection report
//! name the model that really answered.
//!
//! The prompt goes on the program's standard input, never on its command line,
//! so a whole session's journal is not bound by an argument's size and the text
//! is not readable in the process list. The program is run from the system's
//! temporary directory and without `LLM_API_KEY`: it runs under the DM's logged-in
//! session, and no key is stored or passed. It gives up after the same time the
//! old API calls did.
//!
//! The connection test says whether the program is installed and answers: a
//! program that prints nothing has not answered.

use std::env::{self, consts::EXE_SUFFIX};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ports_llm::{
    CONNECTION_TEST_PROMPT, ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError,
    Provider, ProviderConfig, RequestJournal, RequestKind,
};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// The program the DM has installed.
const PROGRAM: &str = "claude";
/// The one model of every call.
const MODEL: &str = "claude-haiku-5-5";
/// The one effort of every call.
const EFFORT: &str = "medium";
/// The environment variable of the port's old secret, which the program never sees.
const KEY_VARIABLE: &str = "LLM_API_KEY";
/// The provider every request is journalled under.
const PROVIDER: Provider = Provider::ClaudeCodeBinary;

/// How much of a program's standard error a reason keeps.
const DETAIL_LIMIT: usize = 200;
/// A connection test gives up after this.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);
/// A generation can be long: a brief, a whole session's journal.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(180);

/// The argument list of every call: print mode (the prompt is read on the
/// standard input), the model and the effort. The one place it is written.
const fn claude_args() -> [&'static str; 5] {
    ["-p", "--model", MODEL, "--effort", EFFORT]
}

/// Why running the program failed.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// The program is not installed (or not on the search path).
    NotFound,
    /// It is there but could not be started.
    Unlaunchable(io::ErrorKind),
    /// It did not answer in time.
    TimedOut(Duration),
    /// It ended with an error: its exit code (none when a signal ended it) and
    /// what it said.
    Exited { code: Option<i32>, detail: String },
    /// It answered nothing.
    Empty,
}

impl Failure {
    /// What the DM reads.
    fn reason(&self) -> String {
        match self {
            Self::NotFound => format!(
                "programme introuvable : « {PROGRAM} » n'est pas installé ou absent du PATH"
            ),
            Self::Unlaunchable(kind) => format!("impossible de lancer le programme : {kind}"),
            Self::TimedOut(limit) => {
                format!("le programme n'a pas répondu en {} s", limit.as_secs())
            }
            Self::Exited { code, detail } => {
                let ended = match code {
                    Some(code) => format!("le programme a échoué (code {code})"),
                    None => "le programme a été interrompu".to_owned(),
                };
                if detail.is_empty() {
                    ended
                } else {
                    format!("{ended} : {detail}")
                }
            }
            Self::Empty => "réponse du programme vide".to_owned(),
        }
    }
}

/// The `llm` port over the DM's `claude` program.
///
/// The program is looked for on the process's `PATH`, or in the directories of
/// [`ClaudeCodeLlm::in_dirs`]. Only a request that reaches a started program is
/// journalled: a program that is not installed is sent nothing.
#[derive(Clone, Debug, Default)]
pub struct ClaudeCodeLlm {
    /// Where to look for the program; `None` is the process's `PATH`.
    dirs: Option<Vec<PathBuf>>,
    journal: RequestJournal,
}

impl ClaudeCodeLlm {
    /// A client that looks for the program on the process's `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The same client, looking for the program in `dirs` and nowhere else: for
    /// a test that stands a stub in for the program.
    #[must_use]
    pub fn in_dirs(mut self, dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        self.dirs = Some(dirs.into_iter().collect());
        self
    }

    /// Where the program is installed, if it is.
    fn find(&self) -> Option<PathBuf> {
        let file = format!("{PROGRAM}{EXE_SUFFIX}");
        let dirs = match &self.dirs {
            Some(dirs) => dirs.clone(),
            None => env::var_os("PATH")
                .map(|path| env::split_paths(&path).collect())
                .unwrap_or_default(),
        };
        // A relative entry of PATH (an empty one is the current directory) is
        // never searched: a program planted beside the server is not the DM's.
        dirs.iter()
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(&file))
            .find(|candidate| is_executable(candidate))
    }

    /// Runs the program on `text`, journalling the request once the program is
    /// started, and returns what it printed.
    async fn run(&self, kind: RequestKind, text: &str, limit: Duration) -> Result<String, Failure> {
        let path = self.find().ok_or(Failure::NotFound)?;
        // The program was found: whatever stops it starting now (a missing
        // interpreter included) is not "not installed".
        let mut child = command(&path)
            .spawn()
            .map_err(|error| Failure::Unlaunchable(error.kind()))?;
        self.journal.record(kind, PROVIDER, text);
        let stdin = child.stdin.take();
        // The text goes in while the output is read, so a program that answers
        // before it has read everything cannot block on a full pipe. A program
        // that stops reading is its own business: its exit says how it went.
        let feed = async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(text.as_bytes()).await;
            }
        };
        let finished = tokio::time::timeout(limit, async {
            tokio::join!(feed, child.wait_with_output()).1
        })
        .await;
        // On a timeout the child is dropped with the future: it is killed.
        let output = finished
            .map_err(|_| Failure::TimedOut(limit))?
            .map_err(|error| Failure::Unlaunchable(error.kind()))?;
        if !output.status.success() {
            // Claude Code says what is wrong (not logged in...) on its standard
            // output, other failures on its standard error.
            let said = String::from_utf8_lossy(&output.stderr);
            let said = if said.trim().is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                said
            };
            return Err(Failure::Exited {
                code: output.status.code(),
                detail: said.trim().chars().take(DETAIL_LIMIT).collect(),
            });
        }
        let answer = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if answer.is_empty() {
            return Err(Failure::Empty);
        }
        Ok(answer)
    }
}

/// The program, ready to be started: its arguments, a pipe for each of its
/// three streams, the port's key withheld, and nothing of the DM's working
/// directory to read.
fn command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command
        .args(claude_args())
        .env_remove(KEY_VARIABLE)
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

impl Llm for ClaudeCodeLlm {
    async fn test_connection(&self, _config: &ProviderConfig) -> ConnectionReport {
        let started = Instant::now();
        let tested = self
            .run(
                RequestKind::ConnectionTest,
                CONNECTION_TEST_PROMPT,
                TEST_TIMEOUT,
            )
            .await;
        match tested {
            Ok(_) => ConnectionReport {
                ok: true,
                provider: Some(PROVIDER),
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                message: format!(
                    "Connexion réussie ({}) — génération : {MODEL}, résumé : {MODEL}, effort : {EFFORT}",
                    PROVIDER.label()
                ),
            },
            Err(failure) => ConnectionReport {
                ok: false,
                provider: Some(PROVIDER),
                latency_ms: 0,
                message: format!("Échec ({}) : {}", PROVIDER.label(), failure.reason()),
            },
        }
    }

    async fn generate(
        &self,
        _config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        let text = self
            .run(
                RequestKind::from(request.kind),
                &request.text,
                GENERATE_TIMEOUT,
            )
            .await
            .map_err(|failure| LlmError::Failed(failure.reason()))?;
        Ok(GenerateAnswer {
            text,
            model: MODEL.to_owned(),
        })
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_never_an_argument_and_the_arguments_are_fixed() {
        assert_eq!(
            claude_args(),
            ["-p", "--model", "claude-haiku-5-5", "--effort", "medium"]
        );
    }

    #[test]
    fn the_command_withholds_the_key_and_runs_from_the_temporary_directory() {
        let command = command(Path::new("/bin/true"));
        let std = command.as_std();
        assert!(
            std.get_envs()
                .any(|(name, value)| name == KEY_VARIABLE && value.is_none()),
            "LLM_API_KEY is not removed from the program's environment"
        );
        assert_eq!(std.get_current_dir(), Some(env::temp_dir().as_path()));
        assert_eq!(std.get_args().collect::<Vec<_>>(), claude_args());
    }

    #[test]
    fn a_relative_directory_of_the_search_path_is_never_searched() {
        let llm = ClaudeCodeLlm::new().in_dirs([
            PathBuf::from(""),
            PathBuf::from("."),
            PathBuf::from("bin"),
        ]);
        assert_eq!(llm.find(), None);
    }

    #[test]
    fn a_failure_reads_for_the_dm() {
        assert_eq!(
            Failure::NotFound.reason(),
            "programme introuvable : « claude » n'est pas installé ou absent du PATH"
        );
        assert_eq!(
            Failure::Exited {
                code: Some(2),
                detail: "boom".into()
            }
            .reason(),
            "le programme a échoué (code 2) : boom"
        );
        assert_eq!(
            Failure::Exited {
                code: Some(2),
                detail: String::new()
            }
            .reason(),
            "le programme a échoué (code 2)"
        );
        assert_eq!(
            Failure::Exited {
                code: None,
                detail: String::new()
            }
            .reason(),
            "le programme a été interrompu"
        );
        assert_eq!(
            Failure::TimedOut(Duration::from_secs(30)).reason(),
            "le programme n'a pas répondu en 30 s"
        );
        assert_eq!(
            Failure::Unlaunchable(io::ErrorKind::PermissionDenied).reason(),
            "impossible de lancer le programme : permission denied"
        );
        assert_eq!(Failure::Empty.reason(), "réponse du programme vide");
    }
}
