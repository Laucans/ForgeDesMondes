//! The Phonon adapter against stub programs: the `speech-to-text` port's
//! contract suite, what the sidecar is handed, and the ways it fails. The real
//! Phonon is never run: the stubs are scripts this file writes, once, before
//! any program is started, and the adapter is pointed at them by path.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ports_speech_to_text::contract::{Scenario, run};
use ports_speech_to_text::{
    Dictation, SpeechToText, SpeechToTextError, Transcript, TranscriptStream, Vocabulary,
};
use ports_speech_to_text_phonon::PhononSidecar;
use serde_json::{Value, json};

const HEARD: &str = "Élowen entre dans la Tour d'Argent, le tonnerre gronde.";

/// What the recording stub says when it listens.
const PHRASES: [&str; 2] = ["Élowen entre dans la Tour d'Argent.", "Le tonnerre gronde."];

/// A sidecar that records how it was started and answers a fixed text. A call
/// leaves `call.<pid>.args` (one `arg:` line per argument), `.header` (the
/// first line of its input) and `.audio` (the rest) beside the script. When the
/// header asks it to listen it leaves `stream.<pid>.args`, `.header` and `.pid`
/// instead, says two phrases (and an empty line between them), then waits for
/// its input to close.
const RECORDING: &str = r#"#!/bin/sh
here=${0%/*}
IFS= read -r header
case "$header" in
*'"mode":"stream"'*)
  out="$here/stream.$$"
  for a in "$@"; do echo "arg:$a"; done > "$out.args"
  printf '%s\n' "$header" > "$out.header"
  echo $$ > "$out.pid"
  echo "Élowen entre dans la Tour d'Argent."
  echo ""
  echo "Le tonnerre gronde."
  while IFS= read -r _; do :; done
  ;;
*)
  out="$here/call.$$"
  for a in "$@"; do echo "arg:$a"; done > "$out.args"
  printf '%s\n' "$header" > "$out.header"
  cat > "$out.audio"
  echo "Élowen entre dans la Tour d'Argent, le tonnerre gronde."
  ;;
esac
"#;

/// A sidecar that fails the way a crashed engine does.
const FAILING: &str = "#!/bin/sh\ncat > /dev/null\necho 'boom : modèle absent' >&2\nexit 3\n";

/// A sidecar that succeeds and hears nothing.
const SILENT: &str = "#!/bin/sh\ncat > /dev/null\n";

/// A sidecar that answers bytes that are not text.
const GARBLED: &str = "#!/bin/sh\ncat > /dev/null\nprintf '\\377\\376\\n'\n";

/// A sidecar that says a phrase, then fails the way a crashed engine does,
/// with more to say on its standard error than a reason keeps.
const FAILING_STREAM: &str = r#"#!/bin/sh
echo "Élowen entre."
echo "boom : modèle absent" >&2
head -c 300 /dev/zero | tr '\0' y >&2
exit 3
"#;

/// A sidecar that says a phrase and ends, content.
const ENDING_STREAM: &str = "#!/bin/sh\necho 'Au revoir.'\n";

/// A sidecar that says bytes that are not text, then waits.
const GARBLED_STREAM: &str = "#!/bin/sh\nprintf '\\377\\376\\n'\nexec sleep 30\n";

/// A sidecar that says a lot on its standard error before it says a phrase.
const TALKATIVE_STREAM: &str = r#"#!/bin/sh
head -c 300000 /dev/zero | tr '\0' x >&2
echo "Enfin."
while IFS= read -r _; do :; done
"#;

/// A sidecar that talks on its standard error before it reads its input.
const CHATTY_STREAM: &str = r#"#!/bin/sh
head -c 300000 /dev/zero | tr '\0' x >&2
IFS= read -r header
echo "Lu."
while IFS= read -r _; do :; done
"#;

/// A sidecar that closes its output and stays.
const MUTE_STREAM: &str = "#!/bin/sh\nexec >&-\nexec sleep 30\n";

/// A sidecar that never answers.
const HANGING: &str = "#!/bin/sh\nexec sleep 30\n";

/// The directories the stubs live in, one per behaviour.
struct Stubs {
    recording: PathBuf,
    failing: PathBuf,
    silent: PathBuf,
    garbled: PathBuf,
    hanging: PathBuf,
    failing_stream: PathBuf,
    ending_stream: PathBuf,
    garbled_stream: PathBuf,
    talkative_stream: PathBuf,
    chatty_stream: PathBuf,
    mute_stream: PathBuf,
    /// A file that is there but not executable.
    plain: PathBuf,
    /// No file at all.
    missing: PathBuf,
}

/// Writes every stub before the first program is started: a script being
/// written while a program is started elsewhere in the process can make that
/// start fail with "text file busy". The directory is the process's own: under
/// nextest each test is a process, and one clearing a shared directory would
/// delete the stubs of another.
fn stubs() -> &'static Stubs {
    static STUBS: OnceLock<Stubs> = OnceLock::new();
    STUBS.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("phonon-stubs-{}", std::process::id()));
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
            garbled: stub("garbled", Some((GARBLED, 0o755))),
            hanging: stub("hanging", Some((HANGING, 0o755))),
            failing_stream: stub("failing-stream", Some((FAILING_STREAM, 0o755))),
            ending_stream: stub("ending-stream", Some((ENDING_STREAM, 0o755))),
            garbled_stream: stub("garbled-stream", Some((GARBLED_STREAM, 0o755))),
            talkative_stream: stub("talkative-stream", Some((TALKATIVE_STREAM, 0o755))),
            chatty_stream: stub("chatty-stream", Some((CHATTY_STREAM, 0o755))),
            mute_stream: stub("mute-stream", Some((MUTE_STREAM, 0o755))),
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
        heard: PHRASES.map(String::from).to_vec(),
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

#[tokio::test]
async fn a_sidecar_that_answers_something_other_than_text_is_a_failure() {
    let reason = failed(
        PhononSidecar::at(&stubs().garbled)
            .transcribe(&Dictation::new(audio(19)), &vocabulary())
            .await,
    );
    assert!(reason.contains("pas répondu en texte"), "{reason}");
}

/// What the recording stub was started with to listen.
struct Listen {
    arguments: String,
    header: String,
    pid: String,
}

/// The streams the recording stub was asked to open whose vocabulary names
/// `term`: each test marks its own.
fn listens_for(term: &str) -> Vec<Listen> {
    let dir = stubs().recording.parent().unwrap();
    let mut listens = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".header") else {
            continue;
        };
        if !stem.starts_with("stream.") {
            continue;
        }
        let header = fs::read_to_string(&path).unwrap();
        if !header.contains(term) {
            continue;
        }
        listens.push(Listen {
            arguments: fs::read_to_string(dir.join(format!("{stem}.args"))).unwrap(),
            header,
            pid: fs::read_to_string(dir.join(format!("{stem}.pid")))
                .unwrap()
                .trim()
                .to_owned(),
        });
    }
    listens
}

fn marked(term: &str) -> Vocabulary {
    Vocabulary {
        campaign_names: vec![term.into(), "Tour d'Argent".into()],
        aliases: vec!["la Louve".into()],
        sound_names: vec!["tonnerre lointain".into()],
    }
}

/// Whether a process is still there.
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Waits for `pid` to be gone, a process killed by a dropped task being
/// reaped a moment later.
async fn gone(pid: &str) -> bool {
    for _ in 0..100 {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn everything(stream: &mut TranscriptStream) -> Vec<Result<Transcript, SpeechToTextError>> {
    let mut all = Vec::new();
    while let Some(item) = stream.next().await {
        all.push(item);
    }
    all
}

#[tokio::test]
async fn a_stream_is_handed_the_language_the_vocabulary_and_the_mode_on_its_input_only() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let vocabulary = marked("Marquise-A");
    let mut stream = adapter.listen(&vocabulary).await.unwrap();
    assert_eq!(stream.next().await, Some(Ok(Transcript::new(PHRASES[0]))));
    stream.stop().await;

    let listens = listens_for("Marquise-A");
    assert_eq!(listens.len(), 1);
    assert_eq!(listens[0].arguments, "", "nothing is handed as an argument");
    let header: Value = serde_json::from_str(&listens[0].header).unwrap();
    assert_eq!(
        header,
        json!({"language": "fr", "mode": "stream", "vocabulary": {
            "campaign_names": ["Marquise-A", "Tour d'Argent"],
            "aliases": ["la Louve"],
            "sound_names": ["tonnerre lointain"],
        }})
    );
}

#[tokio::test]
async fn a_stream_yields_each_phrase_as_a_line_and_skips_the_blank_ones() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let mut stream = adapter.listen(&marked("Marquise-B")).await.unwrap();
    for phrase in PHRASES {
        assert_eq!(stream.next().await, Some(Ok(Transcript::new(phrase))));
    }
    stream.stop().await;
}

#[tokio::test]
async fn a_stream_is_journalled_with_the_vocabulary_and_no_audio_once_the_sidecar_is_started() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let mut stream = adapter.listen(&marked("Marquise-C")).await.unwrap();
    let journal = adapter.journal().list();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, 1);
    assert_eq!(journal[0].audio_bytes, 0);
    assert_eq!(journal[0].vocabulary, marked("Marquise-C"));
    stream.stop().await;
}

#[tokio::test]
async fn stopping_a_stream_leaves_no_process_running() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let mut stream = adapter.listen(&marked("Marquise-D")).await.unwrap();
    stream.next().await.unwrap().unwrap();
    let listens = listens_for("Marquise-D");
    assert_eq!(listens.len(), 1);
    assert!(alive(&listens[0].pid), "the sidecar listens");
    stream.stop().await;
    assert!(stream.is_finished());
    assert!(!alive(&listens[0].pid), "the sidecar is gone");
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn dropping_a_stream_leaves_no_process_running() {
    let adapter = PhononSidecar::at(&stubs().recording);
    let mut stream = adapter.listen(&marked("Marquise-E")).await.unwrap();
    stream.next().await.unwrap().unwrap();
    let pid = listens_for("Marquise-E").remove(0).pid;
    assert!(alive(&pid));
    drop(stream);
    assert!(gone(&pid).await, "the sidecar is gone");
}

#[tokio::test]
async fn a_sidecar_that_says_nothing_is_stopped_at_once() {
    let adapter = PhononSidecar::at(&stubs().hanging);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    let started = Instant::now();
    stream.stop().await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(stream.is_finished());
}

#[tokio::test]
async fn a_sidecar_that_ends_content_ends_the_stream_without_a_failure() {
    let adapter = PhononSidecar::at(&stubs().ending_stream);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    assert_eq!(
        everything(&mut stream).await,
        [Ok(Transcript::new("Au revoir."))]
    );
    assert!(stream.is_finished());
}

#[tokio::test]
async fn a_sidecar_that_fails_leaves_why_as_the_last_item_of_the_stream() {
    let adapter = PhononSidecar::at(&stubs().failing_stream);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    let mut all = everything(&mut stream).await;
    assert_eq!(all.len(), 2, "{all:?}");
    let SpeechToTextError::Failed(reason) = all.pop().unwrap().unwrap_err() else {
        panic!("expected a failure");
    };
    assert!(
        reason.starts_with("le sidecar de transcription a échoué (code 3) : boom : modèle absent"),
        "{reason}"
    );
    assert!(
        reason.chars().count()
            <= "le sidecar de transcription a échoué (code 3) : "
                .chars()
                .count()
                + 200,
        "{reason}"
    );
    assert_eq!(all, [Ok(Transcript::new("Élowen entre."))]);
}

#[tokio::test]
async fn a_sidecar_that_says_bytes_that_are_not_text_fails_the_stream() {
    let adapter = PhononSidecar::at(&stubs().garbled_stream);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    let all = everything(&mut stream).await;
    assert_eq!(all.len(), 1, "{all:?}");
    let reason = failed(all.into_iter().next().unwrap());
    assert!(reason.contains("pas répondu en texte"), "{reason}");
}

#[tokio::test]
async fn a_sidecar_that_says_a_lot_on_its_standard_error_is_not_blocked_by_it() {
    let adapter = PhononSidecar::at(&stubs().talkative_stream);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    let heard = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("the phrase came");
    assert_eq!(heard, Some(Ok(Transcript::new("Enfin."))));
    stream.stop().await;
}

#[tokio::test]
async fn no_sidecar_to_listen_to_is_a_failure_and_nothing_is_journalled() {
    for (adapter, said) in [
        (PhononSidecar::at(&stubs().missing), "introuvable"),
        (PhononSidecar::at(&stubs().plain), "introuvable"),
        (PhononSidecar::from_setting(None), "SPEECH_TO_TEXT_SIDECAR"),
    ] {
        let Err(error) = adapter.listen(&vocabulary()).await else {
            panic!("a stream was opened");
        };
        let SpeechToTextError::Failed(reason) = error else {
            panic!("expected a failure, got {error:?}");
        };
        assert!(reason.contains(said), "{reason}");
        assert!(adapter.journal().list().is_empty());
    }
}

#[tokio::test]
async fn a_sidecar_that_talks_before_it_reads_its_input_is_not_deadlocked_by_a_large_header() {
    let adapter = PhononSidecar::at(&stubs().chatty_stream);
    let vocabulary = Vocabulary {
        campaign_names: vec!["N".repeat(200_000)],
        ..Vocabulary::default()
    };
    let mut stream = adapter.listen(&vocabulary).await.unwrap();
    let heard = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("the phrase came");
    assert_eq!(heard, Some(Ok(Transcript::new("Lu."))));
    stream.stop().await;
}

#[tokio::test]
async fn a_sidecar_that_closes_its_output_and_stays_is_given_up_on() {
    let adapter = PhononSidecar::at(&stubs().mute_stream);
    let mut stream = adapter.listen(&vocabulary()).await.unwrap();
    let started = Instant::now();
    let all = tokio::time::timeout(Duration::from_secs(20), everything(&mut stream))
        .await
        .expect("the stream ended");
    assert!(all.is_empty(), "{all:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
}
