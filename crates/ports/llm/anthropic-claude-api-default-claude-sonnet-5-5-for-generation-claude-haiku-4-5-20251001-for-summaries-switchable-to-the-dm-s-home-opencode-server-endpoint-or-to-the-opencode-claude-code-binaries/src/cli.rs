//! Binary mode: the DM's own `opencode` or `claude` (Claude Code) program.
//!
//! The program is run on the DM's machine with the prompt on its standard
//! input and its answer read from its standard output. Nothing else is handed
//! to it: its arguments are fixed flags and the model's name, never the
//! prompt, so a whole session's journal is not bound by an argument's size and
//! the text is not readable in the process list. It is run from the system's
//! temporary directory, without `LLM_API_KEY`, and gives up after the same
//! time an API call does.
//!
//! Exactly two programs are supported ([`parse_binary`] refuses any other
//! name), and the connection test says whether the chosen one is installed
//! and answers: a program that prints nothing has not answered. The test
//! prompts the generation model; the summary model is
//! named in the message but, a program having no cheaper way to know whether a
//! model exists, not asked for.

use std::env::{self, consts::EXE_SUFFIX};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ports_llm::{
    Binary, CONNECTION_TEST_PROMPT, ConnectionReport, GenerateAnswer, GenerateRequest, Llm,
    LlmError, Provider, ProviderConfig, RequestJournal, RequestKind,
};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::{GENERATE_TIMEOUT, KEY_VARIABLE, TEST_TIMEOUT, elapsed_ms, failed};

/// How much of a program's standard error a reason keeps.
const DETAIL_LIMIT: usize = 200;

/// What a binary-mode call says to a configuration that is not a binary one.
const NOT_A_BINARY: &str =
    "ce client ne lance que des programmes : le mode API est un autre adaptateur";

/// The binary values a configuration may hold, as the DM's settings write them.
const SUPPORTED: &str = "« opencode » ou « claude-code »";

/// The binary a setting names: exactly `opencode` or `claude-code`.
///
/// # Errors
///
/// [`LlmError::Failed`] for any other value, which no program is run for.
pub fn parse_binary(name: &str) -> Result<Binary, LlmError> {
    match name {
        "opencode" => Ok(Binary::Opencode),
        "claude-code" => Ok(Binary::ClaudeCode),
        other => Err(LlmError::Failed(format!(
            "binaire non pris en charge : « {other} » (attendu : {SUPPORTED})"
        ))),
    }
}

/// The name a binary is run by.
const fn program(binary: Binary) -> &'static str {
    match binary {
        Binary::Opencode => "opencode",
        Binary::ClaudeCode => "claude",
    }
}

/// The arguments a binary is run with: its mode for a one-shot text answer
/// that reads the prompt on its standard input, and the model. The model is
/// one argument with its flag (`--model=x`), so a name that starts with a dash
/// is a value, not an option; a blank one leaves the program its own default.
/// Claude Code is given no tool to use (`--tools ""`): it is asked for text.
fn arguments(binary: Binary, model: &str) -> Vec<String> {
    let mut arguments = match binary {
        Binary::Opencode => vec!["run".to_owned()],
        Binary::ClaudeCode => vec!["--print".to_owned()],
    };
    if !model.trim().is_empty() {
        arguments.push(format!("--model={model}"));
    }
    if binary == Binary::ClaudeCode {
        // After the model: the option takes every value up to the next flag.
        arguments.extend(["--tools".to_owned(), String::new()]);
    }
    arguments
}

/// Why running a program failed.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    /// The program is not installed (or not on the search path).
    NotFound(&'static str),
    /// It is there but could not be started.
    Unlaunchable(io::ErrorKind),
    /// It did not answer in time.
    TimedOut(Duration),
    /// It ended with an error: its exit code (none when a signal ended it) and
    /// what it said on its standard error.
    Exited { code: Option<i32>, detail: String },
    /// It answered nothing.
    Empty,
}

impl Failure {
    /// What the DM reads.
    fn reason(&self) -> String {
        match self {
            Self::NotFound(name) => {
                format!("programme introuvable : « {name} » n'est pas installé ou absent du PATH")
            }
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

/// The `llm` port over the DM's local programs.
///
/// The programs are looked for on the process's `PATH`, or in the directories
/// of [`BinaryLlm::in_dirs`]. Only a request that reaches a started program is
/// journalled: a program that is not installed is sent nothing.
#[derive(Clone, Debug, Default)]
pub struct BinaryLlm {
    /// Where to look for the programs; `None` is the process's `PATH`.
    dirs: Option<Vec<PathBuf>>,
    journal: RequestJournal,
}

impl BinaryLlm {
    /// A client that looks for the programs on the process's `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The same client, looking for the programs in `dirs` and nowhere else:
    /// for a test that stands a stub in for the program.
    #[must_use]
    pub fn in_dirs(mut self, dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        self.dirs = Some(dirs.into_iter().collect());
        self
    }

    /// The same client writing to `journal`, which a selector shares between
    /// its clients.
    #[must_use]
    pub fn with_journal(mut self, journal: RequestJournal) -> Self {
        self.journal = journal;
        self
    }

    /// Where `name` is installed, if it is.
    fn find(&self, name: &str) -> Option<PathBuf> {
        let file = format!("{name}{EXE_SUFFIX}");
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

    /// Runs `binary` on `text` with `model`, journalling the request once the
    /// program is started, and returns what it printed.
    async fn run(
        &self,
        binary: Binary,
        model: &str,
        kind: RequestKind,
        text: &str,
        limit: Duration,
    ) -> Result<String, Failure> {
        let name = program(binary);
        let path = self.find(name).ok_or(Failure::NotFound(name))?;
        // The program was found: whatever stops it starting now (a missing
        // interpreter included) is not "not installed".
        let mut child = command(&path, binary, model)
            .spawn()
            .map_err(|error| Failure::Unlaunchable(error.kind()))?;
        self.journal.record(kind, Provider::from(binary), text);
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
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            // Claude Code says what is wrong (not logged in...) on its standard
            // output; the others on their standard error.
            let said = String::from_utf8_lossy(&output.stderr);
            let said = if said.trim().is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                said
            };
            Err(Failure::Exited {
                code: output.status.code(),
                detail: said.trim().chars().take(DETAIL_LIMIT).collect(),
            })
        }
    }
}

/// The program, ready to be started: its arguments, a pipe for each of its
/// three streams, the port's key withheld, and nothing of the DM's working
/// directory to read.
fn command(path: &Path, binary: Binary, model: &str) -> Command {
    let mut command = Command::new(path);
    command
        .args(arguments(binary, model))
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

impl Llm for BinaryLlm {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        let ProviderConfig::Binary { binary, models } = config else {
            return failed(None, NOT_A_BINARY.to_owned());
        };
        let provider = Provider::from(*binary);
        let started = Instant::now();
        let tested = self
            .run(
                *binary,
                &models.generation,
                RequestKind::ConnectionTest,
                CONNECTION_TEST_PROMPT,
                TEST_TIMEOUT,
            )
            .await
            .and_then(|answer| {
                if answer.is_empty() {
                    Err(Failure::Empty)
                } else {
                    Ok(answer)
                }
            });
        match tested {
            Ok(_) => ConnectionReport {
                ok: true,
                provider: Some(provider),
                latency_ms: elapsed_ms(started),
                message: format!(
                    "Connexion réussie ({}) — génération : {}, résumé : {}",
                    provider.label(),
                    models.generation,
                    models.summary
                ),
            },
            Err(failure) => failed(
                Some(provider),
                format!("Échec ({}) : {}", provider.label(), failure.reason()),
            ),
        }
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        let ProviderConfig::Binary { binary, models } = config else {
            return Err(LlmError::Failed(NOT_A_BINARY.to_owned()));
        };
        let model = models.for_kind(request.kind);
        let text = self
            .run(
                *binary,
                model,
                RequestKind::from(request.kind),
                &request.text,
                GENERATE_TIMEOUT,
            )
            .await
            .map_err(|failure| LlmError::Failed(failure.reason()))?;
        if text.is_empty() {
            return Err(LlmError::Failed(Failure::Empty.reason()));
        }
        Ok(GenerateAnswer {
            text,
            model: model.to_owned(),
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
    fn exactly_two_binaries_are_accepted_by_their_exact_name() {
        assert_eq!(parse_binary("opencode"), Ok(Binary::Opencode));
        assert_eq!(parse_binary("claude-code"), Ok(Binary::ClaudeCode));
        for other in [
            "claude",
            "Opencode",
            "CLAUDE-CODE",
            " opencode",
            "opencode ",
            "gemini",
            "",
        ] {
            let Err(LlmError::Failed(reason)) = parse_binary(other) else {
                panic!("{other:?} was accepted");
            };
            assert!(reason.contains("opencode") && reason.contains("claude-code"));
            assert!(reason.contains(&format!("« {other} »")), "{reason}");
        }
    }

    #[test]
    fn the_prompt_is_never_an_argument_and_the_model_is_one_argument() {
        assert_eq!(
            arguments(Binary::Opencode, "anthropic/claude-sonnet-5-5"),
            ["run", "--model=anthropic/claude-sonnet-5-5"]
        );
        assert_eq!(
            arguments(Binary::ClaudeCode, "claude-haiku-4-5-20251001"),
            [
                "--print",
                "--model=claude-haiku-4-5-20251001",
                "--tools",
                ""
            ]
        );
        // A name that looks like an option stays a value; a blank one is none.
        assert_eq!(
            arguments(Binary::ClaudeCode, "--tools"),
            ["--print", "--model=--tools", "--tools", ""]
        );
        assert_eq!(arguments(Binary::Opencode, "  "), ["run"]);
    }

    #[test]
    fn the_command_withholds_the_key_and_runs_from_the_temporary_directory() {
        let command = command(Path::new("/bin/true"), Binary::ClaudeCode, "m");
        let std = command.as_std();
        assert!(
            std.get_envs()
                .any(|(name, value)| name == KEY_VARIABLE && value.is_none()),
            "LLM_API_KEY is not removed from the program's environment"
        );
        assert_eq!(std.get_current_dir(), Some(env::temp_dir().as_path()));
    }

    #[test]
    fn a_relative_directory_of_the_search_path_is_never_searched() {
        let llm =
            BinaryLlm::new().in_dirs([PathBuf::from(""), PathBuf::from("."), PathBuf::from("bin")]);
        assert_eq!(llm.find("sh"), None);
        let sh = Path::new("/bin/sh");
        if sh.exists() {
            let llm = BinaryLlm::new().in_dirs([PathBuf::from("bin"), PathBuf::from("/bin")]);
            assert_eq!(llm.find("sh").as_deref(), Some(sh));
        }
    }

    #[test]
    fn a_failure_reads_for_the_dm() {
        assert_eq!(
            Failure::NotFound("claude").reason(),
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
    }
}
