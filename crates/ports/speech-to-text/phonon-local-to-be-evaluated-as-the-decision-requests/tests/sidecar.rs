//! The Phonon adapter against stub programs: the `speech-to-text` port's
//! contract suite, what the sidecar is handed, and the ways it fails. The real
//! Phonon is never run: the stubs are scripts this file writes, once, before
//! any program is started, and the adapter is pointed at them by path.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use ports_speech_to_text::contract::{Scenario, run};
use ports_speech_to_text::{Dictation, SpeechToText, SpeechToTextError, Vocabulary};
use ports_speech_to_text_phonon::PhononSidecar;
use serde_json::{Value, json};

const HEARD: &str = "Élowen entre dans la Tour d'Argent, le tonnerre gronde.";

/// A sidecar that records how it was started and answers a fixed text. A call
/// leaves `call.<pid>.args` (one `arg:` line per argument), `.header` (the
/// first line of its input) and `.audio` (the rest) beside the script.
const RECORDING: &str = r#"#!/bin/sh
here=${0%/*}
out="$here/call.$$"
for a in "$@"; do echo "arg:$a"; done > "$out.args"
IFS= read -r header
printf '%s\n' "$header" > "$out.header"
cat > "$out.audio"
echo "Élowen entre dans la Tour d'Argent, le tonnerre gronde."
"#;

/// A sidecar that fails the way a crashed engine does.
const FAILING: &str = "#!/bin/sh\ncat > /dev/null\necho 'boom : modèle absent' >&2\nexit 3\n";

/// A sidecar that succeeds and hears nothing.
const SILENT: &str = "#!/bin/sh\ncat > /dev/null\n";

/// A sidecar that never answers.
const HANGING: &str = "#!/bin/sh\nexec sleep 30\n";

/// The directories the stubs live in, one per behaviour.
struct Stubs {
    recording: PathBuf,
    failing: PathBuf,
    silent: PathBuf,
    hanging: PathBuf,
    /// A file that is there but not executable.
    plain: PathBuf,
    /// No file at all.
    missing: PathBuf,
}

/// Writes every stub before the first program is started: a script being
/// written while a program is started elsewhere in the process can make that
/// start fail with "text file busy".
fn stubs() -> &'static Stubs {
    static STUBS: OnceLock<Stubs> = OnceLock::new();
    STUBS.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("phonon-stubs");
        let _ = fs::remove_dir_all(&root);
        let stub = |name: &str, script: Option<(&str, u32)>| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("phonon");
            if let Some((script, mode)) = script {
                fs::write(&path, script).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            }
            path
        };
        Stubs {
            recording: stub("recording", Some((RECORDING, 0o755))),
            failing: stub("failing", Some((FAILING, 0o755))),
            silent: stub("silent", Some((SILENT, 0o755))),
            hanging: stub("hanging", Some((HANGING, 0o755))),
            plain: stub("plain", Some((RECORDING, 0o644))),
            missing: stub("missing", None),
        }
    })
}

/// What the recording stub was started with.
struct Call {
    arguments: String,
    header: String,
    audio: Vec<u8>,
}

/// The calls the recording stub received with exactly `audio`.
fn calls_with(audio: &[u8]) -> Vec<Call> {
    let dir = stubs().recording.parent().unwrap();
    let mut calls = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".audio") else {
            continue;
        };
        if fs::read(&path).unwrap() != audio {
            continue;
        }
        calls.push(Call {
            arguments: fs::read_to_string(dir.join(format!("{stem}.args"))).unwrap(),
            header: fs::read_to_string(dir.join(format!("{stem}.header"))).unwrap(),
            audio: fs::read(&path).unwrap(),
        });
    }
    calls
}

fn vocabulary() -> Vocabulary {
    Vocabulary {
        campaign_names: vec!["Élowen".into(), "Tour d'Argent".into()],
        aliases: vec!["Élo".into(), "la Louve".into()],
        sound_names: vec!["tonnerre lointain".into()],
    }
}

/// Audio that is not text: NUL, newlines, bytes that are not UTF-8.
fn audio(tag: u8) -> Vec<u8> {
    vec![
        tag, 0, b'\n', 0xff, b'\n', 0xfe, 0, tag, b'\r', b'\n', 1, 2, 3,
    ]
}

fn failed(result: Result<ports_speech_to_text::Transcript, SpeechToTextError>) -> String {
    match result {
        Err(SpeechToTextError::Failed(reason)) => reason,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn the_adapter_passes_the_contract_on_a_stub_sidecar() {
    let scenario = Scenario {
        dictation: Dictation::new(vec![7u8; 3200]),
        vocabulary: vocabulary(),
        expected: HEARD.into(),
    };
    run(&PhononSidecar::at(&stubs().recording), &scenario).await;
}

#[tokio::test]
async fn the_sidecar_is_handed_the_language_the_vocabulary_and_the_audio_on_its_input_only() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let dictation = Dictation::new(audio(11));
    let transcript = adapter.transcribe(&dictation, &vocabulary()).await.unwrap();
    assert_eq!(transcript.text, HEARD);

    let calls = calls_with(&audio(11));
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.audio, dictation.audio, "the audio, byte for byte");
    assert_eq!(call.arguments, "", "nothing is handed as an argument");
    let header: Value = serde_json::from_str(&call.header).unwrap();
    assert_eq!(
        header,
        json!({"language": "fr", "vocabulary": {
            "campaign_names": ["Élowen", "Tour d'Argent"],
            "aliases": ["Élo", "la Louve"],
            "sound_names": ["tonnerre lointain"],
        }})
    );
}

#[tokio::test]
async fn the_request_is_journalled_once_the_sidecar_is_started() {
    let adapter = PhononSidecar::at(&stubs().recording);
    adapter
        .transcribe(&Dictation::new(audio(12)), &vocabulary())
        .await
        .unwrap();
    let journal = adapter.journal().list();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].audio_bytes, audio(12).len());
    assert_eq!(journal[0].vocabulary, vocabulary());
}

#[tokio::test]
async fn no_sidecar_at_the_configured_path_is_a_failure_saying_it_was_not_found() {
    let adapter = PhononSidecar::at(&stubs().missing);
    let reason = failed(
        adapter
            .transcribe(&Dictation::new(audio(13)), &vocabulary())
            .await,
    );
    assert!(reason.contains("introuvable"), "{reason}");
    assert!(
        reason.contains(&stubs().missing.display().to_string()),
        "{reason}"
    );
    assert!(adapter.journal().list().is_empty());
}

#[tokio::test]
async fn a_file_that_is_not_executable_is_not_a_sidecar() {
    let adapter = PhononSidecar::at(&stubs().plain);
    let reason = failed(
        adapter
            .transcribe(&Dictation::new(audio(14)), &vocabulary())
            .await,
    );
    assert!(reason.contains("introuvable"), "{reason}");
    assert!(adapter.journal().list().is_empty());
    assert!(calls_with(&audio(14)).is_empty());
}

#[tokio::test]
async fn no_configured_path_is_a_failure_naming_the_setting() {
    let adapter = PhononSidecar::from_setting(None);
    let reason = failed(
        adapter
            .transcribe(&Dictation::new(audio(15)), &vocabulary())
            .await,
    );
    assert!(reason.contains("SPEECH_TO_TEXT_SIDECAR"), "{reason}");
    assert!(adapter.journal().list().is_empty());
}

#[tokio::test]
async fn an_empty_dictation_starts_no_sidecar() {
    let adapter = PhononSidecar::at(&stubs().recording);
    assert_eq!(
        adapter
            .transcribe(&Dictation::new(Vec::new()), &vocabulary())
            .await,
        Err(SpeechToTextError::EmptyAudio)
    );
    assert!(adapter.journal().list().is_empty());
}

#[tokio::test]
async fn a_sidecar_that_fails_says_why() {
    let reason = failed(
        PhononSidecar::at(&stubs().failing)
            .transcribe(&Dictation::new(audio(16)), &vocabulary())
            .await,
    );
    assert!(
        reason.contains("code 3") && reason.contains("modèle absent"),
        "{reason}"
    );
}

#[tokio::test]
async fn a_sidecar_that_hears_nothing_is_a_failure_not_an_empty_text() {
    let reason = failed(
        PhononSidecar::at(&stubs().silent)
            .transcribe(&Dictation::new(audio(17)), &vocabulary())
            .await,
    );
    assert!(reason.contains("aucun texte"), "{reason}");
}

#[tokio::test]
async fn a_sidecar_that_never_answers_is_given_up_on() {
    let adapter = PhononSidecar::at(&stubs().hanging).with_timeout(Duration::from_millis(300));
    let reason = failed(
        adapter
            .transcribe(&Dictation::new(audio(18)), &vocabulary())
            .await,
    );
    assert!(reason.contains("n'a pas répondu"), "{reason}");
}
