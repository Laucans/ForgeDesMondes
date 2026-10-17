//! The binary-mode adapter and the selector, against stub programs: the `llm`
//! port's contract suite, what a program is handed, and the ways a program
//! fails. No real `opencode` or `claude` is ever run: the stubs are scripts
//! this file writes, once, before any program is started, and they are looked
//! for in directories of their own, not on the machine's `PATH`.
#![cfg(unix)]

use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axum::routing::{get, post};
use axum::{Json, Router};
use ports_llm::contract::{Recognised, Scenario, run};
use ports_llm::{
    Binary, CONNECTION_TEST_PROMPT, GenerateRequest, Llm, LlmError, Models, Provider,
    ProviderConfig, RequestKind, Secret, TextKind,
};
use ports_llm_adapters::{ApiLlm, BinaryLlm, Chosen, Selector, parse_binary};
use serde_json::json;

const GENERATION: &str = "claude-sonnet-5-5";
const SUMMARY: &str = "claude-haiku-4-5-20251001";
const CLOUD_KEY: &str = "sk-ant-api03-très-secret";
const CLOUD_HOST: &str = "api.anthropic.com";

// --- the stub programs -----------------------------------------------------

/// A program that records how it was started and answers a line naming its
/// model. A call leaves `call.<program>.<pid>.args` (one `arg:` line per
/// argument) and `.stdin` (the text it was handed) beside the script.
const RECORDING: &str = r#"#!/bin/sh
here=${0%/*}
name=${0##*/}
out="$here/call.$name.$$"
for a in "$@"; do echo "arg:$a"; done > "$out.args"
cat > "$out.stdin"
model=
for a in "$@"; do case "$a" in --model=*) model=${a#--model=};; esac; done
echo "Réponse de $name ($model)"
"#;

/// A program that fails the way a program without a login does.
const FAILING: &str = "#!/bin/sh\ncat > /dev/null\necho 'boom : modèle indisponible' >&2\nexit 3\n";

/// A program that says what is wrong on its standard output, as Claude Code does.
const LOUD: &str = "#!/bin/sh\ncat > /dev/null\necho 'Not logged in · Please run /login'\nexit 1\n";

/// A program that succeeds and says nothing.
const SILENT: &str = "#!/bin/sh\ncat > /dev/null\n";

/// A program that answers what it was told.
const ECHO: &str = "#!/bin/sh\ncat\n";

/// The directories the stubs live in, one per behaviour.
struct Stubs {
    ok: PathBuf,
    failing: PathBuf,
    loud: PathBuf,
    silent: PathBuf,
    echo: PathBuf,
    /// Programs that are there but not executable.
    plain: PathBuf,
    /// No program at all.
    empty: PathBuf,
}

/// Writes every stub before the first program is started: a script being
/// written while a program is started elsewhere in the process can make that
/// start fail with "text file busy".
fn stubs() -> &'static Stubs {
    static STUBS: OnceLock<Stubs> = OnceLock::new();
    STUBS.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("llm-binary-stubs");
        let _ = fs::remove_dir_all(&root);
        let dir = |name: &str, script: Option<(&str, u32)>| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            if let Some((script, mode)) = script {
                for program in ["opencode", "claude"] {
                    let path = dir.join(program);
                    fs::write(&path, script).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
                }
            }
            dir
        };
        Stubs {
            ok: dir("ok", Some((RECORDING, 0o755))),
            failing: dir("failing", Some((FAILING, 0o755))),
            loud: dir("loud", Some((LOUD, 0o755))),
            silent: dir("silent", Some((SILENT, 0o755))),
            echo: dir("echo", Some((ECHO, 0o755))),
            plain: dir("plain", Some((RECORDING, 0o644))),
            empty: dir("empty", None),
        }
    })
}

/// What a stub was started with.
struct Call {
    program: String,
    arguments: Vec<String>,
    stdin: String,
}

/// The calls of `dir`'s stubs that were handed exactly `text`.
fn calls_handed(dir: &Path, text: &str) -> Vec<Call> {
    let mut calls = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".stdin") else {
            continue;
        };
        if fs::read_to_string(&path).unwrap() != text {
            continue;
        }
        let args = fs::read_to_string(dir.join(format!("{stem}.args"))).unwrap();
        calls.push(Call {
            program: stem.split('.').nth(1).unwrap().to_owned(),
            arguments: args
                .lines()
                .map(|line| line.strip_prefix("arg:").unwrap().to_owned())
                .collect(),
            stdin: text.to_owned(),
        });
    }
    calls
}

// --- configurations --------------------------------------------------------

fn models() -> Models {
    Models {
        generation: GENERATION.into(),
        summary: SUMMARY.into(),
    }
}

fn binary_config(binary: Binary) -> ProviderConfig {
    ProviderConfig::Binary {
        binary,
        models: models(),
    }
}

fn api(url: impl Into<String>, key: Option<&str>) -> ProviderConfig {
    ProviderConfig::Api {
        url: url.into(),
        api_type: None,
        key: key.map(Secret::new),
        models: models(),
    }
}

fn client(dir: &Path) -> BinaryLlm {
    BinaryLlm::new().in_dirs([dir.to_path_buf()])
}

fn recognised_binaries() -> Scenario {
    Scenario {
        recognised: vec![
            Recognised {
                config: binary_config(Binary::Opencode),
                provider: Provider::OpencodeBinary,
            },
            Recognised {
                config: binary_config(Binary::ClaudeCode),
                provider: Provider::ClaudeCodeBinary,
            },
        ],
        unrecognised: vec![],
    }
}

// --- the contract suite ----------------------------------------------------

#[tokio::test]
async fn the_binary_adapter_passes_the_contract() {
    run(&client(&stubs().ok), &recognised_binaries()).await;
}

// --- what a program is handed ----------------------------------------------

#[tokio::test]
async fn a_program_is_handed_the_text_on_its_stdin_and_the_model_in_one_argument() {
    let llm = client(&stubs().ok);
    // The flags before the model's, and after it.
    for (binary, program, before, after) in [
        (Binary::Opencode, "opencode", vec!["run"], vec![]),
        (
            Binary::ClaudeCode,
            "claude",
            vec!["--print"],
            vec!["--tools", ""],
        ),
    ] {
        let config = binary_config(binary);
        for kind in TextKind::ALL {
            let text = format!("Texte confié à {program} ({kind:?}) : la taverne brûle.");
            llm.generate(&config, &GenerateRequest::new(kind, &text))
                .await
                .unwrap();
            let calls = calls_handed(&stubs().ok, &text);
            let [call] = calls.as_slice() else {
                panic!("{program}, {kind:?}: {} calls handed the text", calls.len());
            };
            assert_eq!(call.program, program);
            assert_eq!(call.stdin, text);
            let mut expected: Vec<String> = before.iter().map(ToString::to_string).collect();
            expected.push(format!("--model={}", config.models().for_kind(kind)));
            expected.extend(after.iter().map(ToString::to_string));
            assert_eq!(call.arguments, expected, "{program}, {kind:?}");
            // Nothing but the text: no argument carries it, none names a file.
            assert!(
                call.arguments
                    .iter()
                    .all(|argument| !argument.contains(&text))
            );
            assert!(
                call.arguments
                    .iter()
                    .all(|argument| before.contains(&argument.as_str())
                        || after.contains(&argument.as_str())
                        || argument.starts_with("--model="))
            );
        }
    }
}

#[tokio::test]
async fn the_connection_test_hands_the_test_prompt_to_the_generation_model() {
    let llm = client(&stubs().ok);
    let config = binary_config(Binary::ClaudeCode);
    let report = llm.test_connection(&config).await;
    assert!(report.ok, "{}", report.message);
    // Every test sends the same text, so every concurrent test's call carries
    // it too: the one thing to hold is that all of them name the same model.
    let calls = calls_handed(&stubs().ok, CONNECTION_TEST_PROMPT);
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .all(|call| call.arguments.contains(&format!("--model={GENERATION}"))),
        "a connection test did not ask the generation model"
    );
}

#[tokio::test]
async fn a_very_long_text_goes_in_and_comes_back_whole() {
    // Longer than an argument may be, and than a pipe holds in either direction.
    let text = "Elowen entre dans la taverne. ".repeat(20_000);
    let llm = client(&stubs().echo);
    let answer = llm
        .generate(
            &binary_config(Binary::Opencode),
            &GenerateRequest::new(TextKind::Journal, &text),
        )
        .await
        .unwrap();
    assert_eq!(answer.text, text.trim());
    assert_eq!(answer.model, SUMMARY);
}

#[tokio::test]
async fn the_answer_is_what_the_program_printed_from_the_model_of_the_kind() {
    let llm = client(&stubs().ok);
    let config = binary_config(Binary::ClaudeCode);
    let brief = llm
        .generate(&config, &GenerateRequest::new(TextKind::Brief, "un brief"))
        .await
        .unwrap();
    assert_eq!(brief.text, format!("Réponse de claude ({GENERATION})"));
    assert_eq!(brief.model, GENERATION);
    let journal = llm
        .generate(
            &config,
            &GenerateRequest::new(TextKind::Journal, "un journal"),
        )
        .await
        .unwrap();
    assert_eq!(journal.text, format!("Réponse de claude ({SUMMARY})"));
    assert_eq!(journal.model, SUMMARY);
}

// --- a program that is not there, or does not answer ------------------------

#[tokio::test]
async fn a_program_that_is_not_installed_is_a_failure_that_says_so() {
    for (binary, program) in [
        (Binary::Opencode, "opencode"),
        (Binary::ClaudeCode, "claude"),
    ] {
        let llm = client(&stubs().empty);
        let config = binary_config(binary);
        let report = llm.test_connection(&config).await;
        assert!(!report.ok);
        assert_eq!(report.provider, Some(Provider::from(binary)));
        assert!(report.message.contains("introuvable"), "{}", report.message);
        assert!(report.message.contains(program), "{}", report.message);
        let Err(LlmError::Failed(reason)) = llm
            .generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
            .await
        else {
            panic!("{program}: generated without a program");
        };
        assert!(reason.contains("introuvable"), "{reason}");
        // Nothing was sent, so nothing is journalled.
        assert!(llm.journal().list().is_empty());
    }
}

#[tokio::test]
async fn a_file_that_is_not_executable_is_not_a_program() {
    let llm = client(&stubs().plain);
    let report = llm.test_connection(&binary_config(Binary::Opencode)).await;
    assert!(!report.ok);
    assert!(report.message.contains("introuvable"), "{}", report.message);
}

#[tokio::test]
async fn a_program_that_exits_with_an_error_says_what_it_said() {
    let llm = client(&stubs().failing);
    let config = binary_config(Binary::ClaudeCode);
    let report = llm.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::ClaudeCodeBinary));
    assert!(report.message.contains("code 3"), "{}", report.message);
    assert!(
        report.message.contains("modèle indisponible"),
        "{}",
        report.message
    );
    let Err(LlmError::Failed(reason)) = llm
        .generate(&config, &GenerateRequest::new(TextKind::Notes, "des notes"))
        .await
    else {
        panic!("a failing program generated");
    };
    assert!(reason.contains("code 3"), "{reason}");
    // The text did reach the program: both requests are journalled.
    let journal = llm.journal().list();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0].kind, RequestKind::ConnectionTest);
    assert_eq!(journal[1].text, "des notes");
}

#[tokio::test]
async fn a_program_that_explains_on_its_stdout_is_heard() {
    let llm = client(&stubs().loud);
    let report = llm
        .test_connection(&binary_config(Binary::ClaudeCode))
        .await;
    assert!(!report.ok);
    assert!(report.message.contains("code 1"), "{}", report.message);
    assert!(
        report.message.contains("Not logged in"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn a_program_that_prints_nothing_has_not_answered() {
    let llm = client(&stubs().silent);
    let config = binary_config(Binary::Opencode);
    let report = llm.test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.message.contains("vide"), "{}", report.message);
    let Err(LlmError::Failed(reason)) = llm
        .generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
        .await
    else {
        panic!("a silent program generated");
    };
    assert!(reason.contains("vide"), "{reason}");
}

#[tokio::test]
async fn an_api_configuration_is_not_for_this_client() {
    let llm = client(&stubs().ok);
    let config = api("https://api.anthropic.com/v1", Some(CLOUD_KEY));
    assert!(!llm.test_connection(&config).await.ok);
    assert!(matches!(
        llm.generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
            .await,
        Err(LlmError::Failed(_))
    ));
    assert!(llm.journal().list().is_empty());
}

// --- the binary values a setting may hold ----------------------------------

#[test]
fn only_opencode_and_claude_code_are_binaries() {
    assert_eq!(parse_binary("opencode"), Ok(Binary::Opencode));
    assert_eq!(parse_binary("claude-code"), Ok(Binary::ClaudeCode));
    assert!(matches!(
        parse_binary("gemini-cli"),
        Err(LlmError::Failed(_))
    ));
    assert!(parse_binary("claude").is_err());
}

// --- the selector ----------------------------------------------------------

/// A server that plays both APIs, which both live under `/v1`: the Anthropic
/// Messages API, and the home opencode server's OpenAI-compatible one.
async fn api_server() -> SocketAddr {
    let models = json!({"object": "list", "data": [{"id": GENERATION}, {"id": SUMMARY}]});
    let app = Router::new()
        .route("/v1/models", get(move || async move { Json(models) }))
        .route(
            "/v1/chat/completions",
            post(|| async { Json(json!({"choices": [{"message": {"content": "ok"}}]})) }),
        )
        .route(
            "/v1/messages",
            post(|| async { Json(json!({"content": [{"type": "text", "text": "ok"}]})) }),
        )
        .route(
            "/v1/models/{id}",
            get(|| async { Json(json!({"type": "model"})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// A selector over the stub programs and a server standing in for both APIs.
async fn selector() -> (Selector, ProviderConfig, ProviderConfig) {
    let server = api_server().await;
    let http = reqwest::Client::builder().resolve(CLOUD_HOST, server);
    let llm = ApiLlm::with_builder(http, None).allowing_cleartext_cloud();
    let cloud = api(
        format!("http://{CLOUD_HOST}:{}/v1", server.port()),
        Some(CLOUD_KEY),
    );
    let home = api(format!("http://{server}"), None);
    (Selector::new(llm, client(&stubs().ok)), cloud, home)
}

#[tokio::test]
async fn the_selector_returns_the_client_of_each_of_the_four_configurations() {
    let (selector, cloud, home) = selector().await;
    let opencode = binary_config(Binary::Opencode);
    let claude = binary_config(Binary::ClaudeCode);
    assert!(matches!(selector.client(&cloud), Chosen::Api(_)));
    assert!(matches!(selector.client(&home), Chosen::Api(_)));
    assert!(matches!(selector.client(&opencode), Chosen::Binary(_)));
    assert!(matches!(selector.client(&claude), Chosen::Binary(_)));

    // And each one reaches its own provider.
    for (config, provider) in [
        (&cloud, Provider::Cloud),
        (&home, Provider::OpencodeServer),
        (&opencode, Provider::OpencodeBinary),
        (&claude, Provider::ClaudeCodeBinary),
    ] {
        let report = selector.test_connection(config).await;
        assert!(report.ok, "{provider:?}: {}", report.message);
        assert_eq!(report.provider, Some(provider));
        let answer = selector
            .generate(config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
            .await
            .unwrap_or_else(|error| panic!("{provider:?}: {error}"));
        assert_eq!(answer.model, GENERATION);
    }
}

#[tokio::test]
async fn the_selector_keeps_one_journal_for_every_client() {
    let (selector, cloud, home) = selector().await;
    for config in [
        cloud,
        binary_config(Binary::ClaudeCode),
        home,
        binary_config(Binary::Opencode),
    ] {
        selector
            .generate(&config, &GenerateRequest::new(TextKind::Brief, "un brief"))
            .await
            .unwrap();
    }
    // A client reached through the selector writes the selector's journal.
    for config in [binary_config(Binary::Opencode), api("ftp://x", None)] {
        assert_eq!(selector.client(&config).journal().list().len(), 4);
    }
    let providers: Vec<_> = selector
        .journal()
        .list()
        .iter()
        .map(|record| (record.sequence, record.provider))
        .collect();
    assert_eq!(
        providers,
        [
            (1, Provider::Cloud),
            (2, Provider::ClaudeCodeBinary),
            (3, Provider::OpencodeServer),
            (4, Provider::OpencodeBinary),
        ]
    );
}

#[tokio::test]
async fn the_selector_passes_the_contract_for_all_four() {
    let (selector, cloud, home) = selector().await;
    let mut scenario = recognised_binaries();
    scenario.recognised.push(Recognised {
        config: cloud,
        provider: Provider::Cloud,
    });
    scenario.recognised.push(Recognised {
        config: home,
        provider: Provider::OpencodeServer,
    });
    scenario.unrecognised.push(api("ftp://serveur", None));
    run(&selector, &scenario).await;
}
