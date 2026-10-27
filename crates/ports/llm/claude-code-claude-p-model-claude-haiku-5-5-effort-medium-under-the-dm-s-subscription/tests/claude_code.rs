//! The Claude Code adapter against stub programs: the `llm` port's contract
//! suite, what the program is handed, and the ways it fails. No real `claude`
//! is ever run, and nothing touches the network: the stubs are scripts this
//! file writes, once, before any program is started, and they are looked for in
//! directories of their own, not on the machine's `PATH`.
#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ports_llm::contract::{Recognised, Scenario, run};
use ports_llm::{
    Binary, CONNECTION_TEST_PROMPT, GenerateRequest, Llm, LlmError, Models, Provider,
    ProviderConfig, RequestKind, Secret, TextKind,
};
use ports_llm_claude_code::ClaudeCodeLlm;

const MODEL: &str = "claude-haiku-5-5";
const KEY: &str = "sk-ant-api03-très-secret";
const SENTINEL: &str = "LLM_STUB_SENTINEL";

// --- the stub programs -----------------------------------------------------

/// A program that records how it was started and answers a line. A call leaves
/// `call.<pid>.args` (one `arg:` line per argument), `.env` (its environment)
/// and `.stdin` (the text it was handed) beside the script.
const RECORDING: &str = r#"#!/bin/sh
out="${0%/*}/call.$$"
for a in "$@"; do echo "arg:$a"; done > "$out.args"
env > "$out.env"
cat > "$out.stdin"
echo "Réponse de claude"
"#;

/// A program that fails the way a program without a login does.
const FAILING: &str = "#!/bin/sh\ncat > /dev/null\necho 'boom : modèle indisponible' >&2\nexit 3\n";

/// A program that says what is wrong on its standard output, as Claude Code does.
const LOUD: &str = "#!/bin/sh\ncat > /dev/null\necho 'Not logged in · Please run /login'\nexit 1\n";

/// A program that succeeds and says nothing.
const SILENT: &str = "#!/bin/sh\ncat > /dev/null\n";

/// A program that answers what it was told.
const ECHO: &str = "#!/bin/sh\ncat\n";

/// A program that takes its time.
const SLOW: &str = "#!/bin/sh\ncat > /dev/null\nsleep 0.2\necho ok\n";

/// The directories the stubs live in, one per behaviour.
struct Stubs {
    ok: PathBuf,
    failing: PathBuf,
    loud: PathBuf,
    silent: PathBuf,
    echo: PathBuf,
    slow: PathBuf,
    /// A program that is there but not executable.
    plain: PathBuf,
    /// No program at all.
    empty: PathBuf,
}

/// Writes every stub before the first program is started: a script being
/// written while a program is started elsewhere in the process can make that
/// start fail with "text file busy". Sets the variables the environment test
/// looks for in the program's, before any test thread starts a program.
fn stubs() -> &'static Stubs {
    static STUBS: OnceLock<Stubs> = OnceLock::new();
    STUBS.get_or_init(|| {
        // SAFETY: every test reaches this first and waits for it; no program is
        // started, and no other thread reads the environment, before it ends.
        unsafe {
            env::set_var("LLM_API_KEY", KEY);
            env::set_var(SENTINEL, "1");
        }
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("llm-claude-code-stubs");
        let _ = fs::remove_dir_all(&root);
        let dir = |name: &str, script: Option<(&str, u32)>| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            if let Some((script, mode)) = script {
                let path = dir.join("claude");
                fs::write(&path, script).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            }
            dir
        };
        Stubs {
            ok: dir("ok", Some((RECORDING, 0o755))),
            failing: dir("failing", Some((FAILING, 0o755))),
            loud: dir("loud", Some((LOUD, 0o755))),
            silent: dir("silent", Some((SILENT, 0o755))),
            echo: dir("echo", Some((ECHO, 0o755))),
            slow: dir("slow", Some((SLOW, 0o755))),
            plain: dir("plain", Some((RECORDING, 0o644))),
            empty: dir("empty", None),
        }
    })
}

/// What a stub was started with.
struct Call {
    arguments: Vec<String>,
    environment: String,
}

/// The calls of `dir`'s stub that were handed exactly `text`.
fn calls_handed(dir: &Path, text: &str) -> Vec<Call> {
    let mut calls = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".stdin") else {
            continue;
        };
        if fs::read(&path).unwrap() != text.as_bytes() {
            continue;
        }
        let args = fs::read_to_string(dir.join(format!("{stem}.args"))).unwrap();
        calls.push(Call {
            arguments: args
                .lines()
                .map(|line| line.strip_prefix("arg:").unwrap().to_owned())
                .collect(),
            environment: fs::read_to_string(dir.join(format!("{stem}.env"))).unwrap(),
        });
    }
    calls
}

/// The one call that was handed `text`.
fn the_call_handed(dir: &Path, text: &str) -> Call {
    let mut calls = calls_handed(dir, text);
    assert_eq!(calls.len(), 1, "{} calls handed {text:?}", calls.len());
    calls.remove(0)
}

// --- configurations --------------------------------------------------------

fn config(generation: &str, summary: &str) -> ProviderConfig {
    ProviderConfig::Binary {
        binary: Binary::ClaudeCode,
        models: Models {
            generation: generation.into(),
            summary: summary.into(),
        },
    }
}

fn client(dir: &Path) -> ClaudeCodeLlm {
    ClaudeCodeLlm::new().in_dirs([dir.to_path_buf()])
}

const EXPECTED_ARGUMENTS: [&str; 5] = ["-p", "--model", MODEL, "--effort", "medium"];

// --- the contract suite ----------------------------------------------------

// AC1: the adapter passes the port's contract::run suite on a stub program
#[tokio::test]
async fn the_adapter_passes_the_contract() {
    let scenario = Scenario {
        recognised: vec![Recognised {
            config: config(MODEL, MODEL),
            provider: Provider::ClaudeCodeBinary,
        }],
        unrecognised: vec![],
    };
    run(&client(&stubs().ok), &scenario).await;
}

// --- what the program is handed --------------------------------------------

// AC2: every TextKind and the connection test run exactly the five arguments, the prompt on stdin
#[tokio::test]
async fn every_call_runs_the_same_arguments_and_the_prompt_arrives_on_stdin() {
    let llm = client(&stubs().ok);
    let config = config(MODEL, MODEL);
    for kind in TextKind::ALL {
        let text = format!("Texte confié au programme ({kind:?}) : la taverne brûle.");
        let answer = llm
            .generate(&config, &GenerateRequest::new(kind, &text))
            .await
            .unwrap();
        assert_eq!(answer.text, "Réponse de claude");
        assert_eq!(answer.model, MODEL);
        let call = the_call_handed(&stubs().ok, &text);
        assert_eq!(call.arguments, EXPECTED_ARGUMENTS, "{kind:?}");
    }
    let report = llm.test_connection(&config).await;
    assert!(report.ok, "{}", report.message);
    let calls = calls_handed(&stubs().ok, CONNECTION_TEST_PROMPT);
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .all(|call| call.arguments == EXPECTED_ARGUMENTS),
        "a connection test ran other arguments"
    );
}

// AC3: the models a ProviderConfig names are ignored, the program gets haiku at medium
#[tokio::test]
async fn the_models_of_the_configuration_are_ignored() {
    let llm = client(&stubs().ok);
    let other = config("claude-sonnet-5-5", "claude-opus-5-5");
    for kind in TextKind::ALL {
        let text = format!("Autres modèles demandés ({kind:?}).");
        let answer = llm
            .generate(&other, &GenerateRequest::new(kind, &text))
            .await
            .unwrap();
        assert_eq!(answer.model, MODEL, "{kind:?}");
        let call = the_call_handed(&stubs().ok, &text);
        assert_eq!(call.arguments, EXPECTED_ARGUMENTS, "{kind:?}");
    }
    let report = llm.test_connection(&other).await;
    assert!(report.ok, "{}", report.message);
    assert!(report.message.contains(MODEL), "{}", report.message);
    assert!(!report.message.contains("claude-sonnet-5-5"));
}

// AC4: LLM_API_KEY, set in the parent's environment, is not in the child's
#[tokio::test]
async fn the_key_never_reaches_the_program() {
    let llm = client(&stubs().ok);
    assert_eq!(env::var("LLM_API_KEY").as_deref(), Ok(KEY));
    // A key the DM's configuration carries is not sent either.
    let with_key = ProviderConfig::Api {
        url: "https://api.anthropic.com".into(),
        api_type: None,
        key: Some(Secret::new("sk-ant-de-la-config")),
        models: Models {
            generation: MODEL.into(),
            summary: MODEL.into(),
        },
    };
    let text = "Un texte pour lire l'environnement du programme.";
    llm.generate(&with_key, &GenerateRequest::new(TextKind::Prompt, text))
        .await
        .unwrap();
    let call = the_call_handed(&stubs().ok, text);
    // The environment was recorded, and inherited...
    assert!(
        call.environment.contains(&format!("{SENTINEL}=1")),
        "{}",
        call.environment
    );
    // ... except for the key.
    assert!(
        !call.environment.contains("LLM_API_KEY"),
        "LLM_API_KEY reached the program"
    );
    assert!(!call.environment.contains(KEY));
    assert!(!call.environment.contains("sk-ant-de-la-config"));
    assert!(
        !call
            .arguments
            .iter()
            .any(|argument| argument.contains("sk-"))
    );
}

#[tokio::test]
async fn a_very_long_text_goes_in_and_comes_back_whole() {
    // Longer than an argument may be, and than a pipe holds in either direction.
    let text = "Elowen entre dans la taverne. ".repeat(20_000);
    let answer = client(&stubs().echo)
        .generate(
            &config(MODEL, MODEL),
            &GenerateRequest::new(TextKind::Journal, &text),
        )
        .await
        .unwrap();
    assert_eq!(answer.text, text.trim());
}

#[tokio::test]
async fn the_journal_records_the_text_under_claude_code() {
    let llm = client(&stubs().ok);
    llm.generate(
        &config(MODEL, MODEL),
        &GenerateRequest::new(TextKind::Notes, "Notes journalisées."),
    )
    .await
    .unwrap();
    let journal = llm.journal().list();
    let [record] = journal.as_slice() else {
        panic!("{journal:?}");
    };
    assert_eq!(record.kind, RequestKind::Notes);
    assert_eq!(record.provider, Provider::ClaudeCodeBinary);
    assert_eq!(record.text, "Notes journalisées.");
}

#[tokio::test]
async fn the_connection_test_reports_how_long_the_program_took() {
    let report = client(&stubs().slow)
        .test_connection(&config(MODEL, MODEL))
        .await;
    assert!(report.ok, "{}", report.message);
    assert!(report.latency_ms >= 200, "{}", report.latency_ms);
    assert_eq!(report.provider, Some(Provider::ClaudeCodeBinary));
}

// --- a program that is not there, or does not answer ------------------------

const NOT_INSTALLED: &str =
    "programme introuvable : « claude » n'est pas installé ou absent du PATH";

// AC5: a missing program is the French « introuvable » failure, and nothing is journalled
#[tokio::test]
async fn a_program_that_is_not_installed_is_a_failure_that_says_so() {
    for dir in [&stubs().empty, &stubs().plain] {
        let llm = client(dir);
        let config = config(MODEL, MODEL);
        let report = llm.test_connection(&config).await;
        assert!(!report.ok);
        assert_eq!(report.provider, Some(Provider::ClaudeCodeBinary));
        assert_eq!(report.latency_ms, 0);
        assert_eq!(
            report.message,
            format!("Échec (binaire Claude Code) : {NOT_INSTALLED}")
        );
        let result = llm
            .generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
            .await;
        assert_eq!(result, Err(LlmError::Failed(NOT_INSTALLED.to_owned())));
        // Nothing was sent, so nothing is journalled.
        assert!(llm.journal().list().is_empty());
    }
}

// AC5: a program that exits non-zero is the French « a échoué » failure, with what it said
#[tokio::test]
async fn a_program_that_exits_with_an_error_says_what_it_said() {
    let llm = client(&stubs().failing);
    let config = config(MODEL, MODEL);
    let report = llm.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::ClaudeCodeBinary));
    assert_eq!(
        report.message,
        "Échec (binaire Claude Code) : le programme a échoué (code 3) : boom : modèle indisponible"
    );
    let result = llm
        .generate(&config, &GenerateRequest::new(TextKind::Brief, "bonjour"))
        .await;
    assert_eq!(
        result,
        Err(LlmError::Failed(
            "le programme a échoué (code 3) : boom : modèle indisponible".to_owned()
        ))
    );
    // The program was started, so the requests are journalled.
    assert_eq!(llm.journal().list().len(), 2);
}

// AC5: what Claude Code says on its standard output is heard
#[tokio::test]
async fn a_program_that_explains_on_its_stdout_is_heard() {
    let result = client(&stubs().loud)
        .generate(
            &config(MODEL, MODEL),
            &GenerateRequest::new(TextKind::Prompt, "bonjour"),
        )
        .await;
    assert_eq!(
        result,
        Err(LlmError::Failed(
            "le programme a échoué (code 1) : Not logged in · Please run /login".to_owned()
        ))
    );
}

#[tokio::test]
async fn a_program_that_prints_nothing_has_not_answered() {
    let llm = client(&stubs().silent);
    let config = config(MODEL, MODEL);
    let report = llm.test_connection(&config).await;
    assert!(!report.ok);
    assert!(
        report.message.contains("réponse du programme vide"),
        "{}",
        report.message
    );
    assert_eq!(
        llm.generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
            .await,
        Err(LlmError::Failed("réponse du programme vide".to_owned()))
    );
}

// --- the repository ----------------------------------------------------------

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

// AC6: the claude argument list is built in one place of the source
#[test]
fn the_argument_list_is_written_once() {
    let source = include_str!("../src/lib.rs");
    let source = source.split("#[cfg(test)]").next().unwrap();
    for flag in [
        "\"--model\"",
        "\"--effort\"",
        "\"-p\"",
        "\"medium\"",
        "\"claude-haiku-5-5\"",
    ] {
        assert_eq!(
            source.matches(flag).count(),
            1,
            "{flag} is written more than once"
        );
    }
    assert_eq!(source.matches("fn claude_args").count(), 1);
    assert_eq!(source.matches(".args(").count(), 1);
}

// AC7: ports.json's llm entry names Claude Code, this crate, the subscription and no secret
#[test]
fn ports_json_names_claude_code() {
    let ports: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root().join("ports.json")).unwrap()).unwrap();
    let llm = ports["ports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|port| port["id"] == "llm")
        .unwrap();
    for field in [&llm["provider"], &llm["tech"]["name"]] {
        assert!(field.as_str().unwrap().contains("claude"), "{field}");
        assert!(
            field.as_str().unwrap().contains("claude-haiku-5-5"),
            "{field}"
        );
    }
    assert!(llm["provider"].as_str().unwrap().contains("Claude Code"));
    assert_eq!(
        llm["adapter"],
        "crates/ports/llm/claude-code-claude-p-model-claude-haiku-5-5-effort-medium-under-the-dm-s-subscription"
    );
    assert!(
        root()
            .join(llm["adapter"].as_str().unwrap())
            .join("Cargo.toml")
            .is_file()
    );
    assert!(llm["cost"].as_str().unwrap().contains("subscription"));
    assert_eq!(llm["secrets"], serde_json::json!([]));
    assert_eq!(
        llm["usedBy"],
        serde_json::json!([
            "ai.testConnection",
            "ai.saveSettings",
            "ai.listProviderRequests",
            "ai.extractFromNotes",
            "ai.analyzeBrief",
            "ai.createFromBrief",
            "ai.proposeCharacter",
            "session.closeSession"
        ])
    );
}

// AC8: the old adapter crate is gone from the workspace, the new one is a member
#[test]
fn the_old_adapter_is_gone_and_the_new_one_is_a_member() {
    let old_name = ["ports-llm", "adapters"].join("-");
    let old_dir = ["anthropic", "claude", "api", "default"].join("-");
    let manifest = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let lock = fs::read_to_string(root().join("Cargo.lock")).unwrap();
    assert!(!manifest.contains(&old_dir));
    assert!(!lock.contains(&old_name));
    assert!(
        manifest.contains("\"crates/ports/llm/claude-code-claude-p-model-claude-haiku-5-5-effort-medium-under-the-dm-s-subscription\""),
        "the root Cargo.toml does not list the new crate"
    );
    let siblings: Vec<_> = fs::read_dir(root().join("crates/ports/llm"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        siblings.iter().all(|name| !name.starts_with(&old_dir)),
        "{siblings:?}"
    );
}

// AC10: a docs/toBeCheck note lists the checks that need a real run
#[test]
fn a_note_lists_what_only_a_real_run_shows() {
    let note =
        fs::read_to_string(root().join("docs/toBeCheck/llm-claude-code-adapter.md")).unwrap();
    assert!(note.contains("claude -p --model claude-haiku-5-5 --effort medium"));
    assert!(note.contains("subscription"));
    assert!(note.contains("--effort"));
}
