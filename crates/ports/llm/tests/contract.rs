//! The contract suite, run against the fake — as an adapter crate runs it
//! against itself.

use ports_llm::contract::{Recognised, Scenario, run};
use ports_llm::fake::FakeLlm;
use ports_llm::{
    Binary, ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, Models, Provider,
    ProviderConfig, RequestJournal, Secret, SharedLlm,
};
use serde_json::json;

const OPENCODE_URL: &str = "http://192.168.1.40:4096";

fn models() -> Models {
    Models {
        generation: "claude-sonnet-5-5".into(),
        summary: "claude-haiku-4-5-20251001".into(),
    }
}

fn api(url: &str, key: Option<&str>) -> ProviderConfig {
    ProviderConfig::Api {
        url: url.into(),
        api_type: None,
        key: key.map(Secret::new),
        models: models(),
    }
}

fn binary(binary: Binary) -> ProviderConfig {
    ProviderConfig::Binary {
        binary,
        models: models(),
    }
}

fn scenario() -> Scenario {
    Scenario {
        recognised: vec![
            Recognised {
                config: api(
                    "https://api.anthropic.com/v1",
                    Some("sk-ant-api03-très-secret"),
                ),
                provider: Provider::Cloud,
            },
            Recognised {
                config: api(OPENCODE_URL, Some("clé-du-serveur-maison")),
                provider: Provider::OpencodeServer,
            },
            Recognised {
                config: binary(Binary::Opencode),
                provider: Provider::OpencodeBinary,
            },
            Recognised {
                config: binary(Binary::ClaudeCode),
                provider: Provider::ClaudeCodeBinary,
            },
        ],
        unrecognised: vec![
            api("https://inconnu.example.org/pas-une-api", Some("sk-x")),
            api("pas une url", None),
        ],
    }
}

fn fake() -> FakeLlm {
    FakeLlm::new().with_endpoint(
        OPENCODE_URL,
        json!({"object": "list", "data": [{"id": "qwen2.5-32b-instruct", "object": "model"}]}),
    )
}

#[tokio::test]
async fn the_fake_passes_the_contract() {
    run(&fake(), &scenario()).await;
}

#[tokio::test]
async fn the_contract_still_holds_on_a_fake_that_already_has_requests() {
    let fake = fake();
    run(&fake, &scenario()).await;
    run(&fake, &scenario()).await;
}

/// The handle a server shares with its units is the port behind it: it
/// passes the contract, and every clone of it reads the same journal.
#[tokio::test]
async fn the_shared_handle_passes_the_contract_and_shares_its_journal() {
    let shared = SharedLlm::new(fake());
    run(&shared, &scenario()).await;
    let clone = shared.clone();
    assert!(!shared.journal().list().is_empty());
    assert_eq!(clone.journal().list(), shared.journal().list());
}

/// Behind the handle, a port that breaks the contract still breaks it.
#[tokio::test]
#[should_panic(expected = "the model that answered")]
async fn the_shared_handle_hands_the_port_s_own_answer_over() {
    run(&SharedLlm::new(Faulty::new(Fault::WrongModel)), &scenario()).await;
}

/// The suite is only worth running if it fails on a port that breaks it: a
/// fake that was told the wrong type of the opencode URL must be caught.
#[tokio::test]
#[should_panic(expected = "provider reached")]
async fn the_contract_fails_a_port_that_reaches_the_wrong_provider() {
    let mut scenario = scenario();
    scenario.recognised[1].provider = Provider::Cloud;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "an unrecognised API type connected")]
async fn the_contract_fails_a_port_that_recognises_what_it_should_not() {
    let mut scenario = scenario();
    scenario.unrecognised.push(api(OPENCODE_URL, None));
    run(&fake(), &scenario).await;
}

/// A port that behaves as the fake does, except for one fault.
enum Fault {
    /// Answers with a model other than the one of the request's kind.
    WrongModel,
    /// Journals a text other than the one it sent.
    AlteredText,
    /// Says the key in the connection test's message.
    LeaksKey,
    /// Takes the type a saved provider carries for a detection.
    TrustsSavedType,
}

struct Faulty {
    inner: FakeLlm,
    fault: Fault,
    journal: RequestJournal,
}

impl Faulty {
    fn new(fault: Fault) -> Self {
        Self {
            inner: fake(),
            fault,
            journal: RequestJournal::new(),
        }
    }

    /// Copies the request the fake just journalled into this port's own
    /// journal, its text rewritten when `rewrite`.
    fn mirror_last(&self, rewrite: bool) {
        let last = self
            .inner
            .journal()
            .list()
            .pop()
            .expect("a request was journalled");
        let text = if rewrite {
            format!("{} (réécrit)", last.text)
        } else {
            last.text
        };
        self.journal.record(last.kind, last.provider, &text);
    }
}

impl Llm for Faulty {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        if let (
            Fault::TrustsSavedType,
            ProviderConfig::Api {
                api_type: Some(_), ..
            },
        ) = (&self.fault, config)
        {
            return ConnectionReport {
                ok: true,
                provider: Some(Provider::Cloud),
                latency_ms: 1,
                message: "Connexion réussie".into(),
            };
        }
        let mut report = self.inner.test_connection(config).await;
        if report.ok {
            self.mirror_last(false);
        }
        if let (Fault::LeaksKey, ProviderConfig::Api { key: Some(key), .. }) = (&self.fault, config)
        {
            report.message = format!("{} ({})", report.message, key.expose());
        }
        report
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        let mut answer = self.inner.generate(config, request).await?;
        self.mirror_last(matches!(self.fault, Fault::AlteredText));
        if matches!(self.fault, Fault::WrongModel) {
            answer.model = "autre-modele".into();
        }
        Ok(answer)
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[tokio::test]
#[should_panic(expected = "the model that answered")]
async fn the_contract_fails_a_port_that_answers_with_the_wrong_model() {
    run(&Faulty::new(Fault::WrongModel), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the text journalled")]
async fn the_contract_fails_a_port_that_journals_other_text_than_it_sent() {
    run(&Faulty::new(Fault::AlteredText), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the key leaked")]
async fn the_contract_fails_a_port_that_leaks_the_key() {
    run(&Faulty::new(Fault::LeaksKey), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "a saved API type stood for the detection")]
async fn the_contract_fails_a_port_that_trusts_a_saved_type_for_the_test() {
    run(&Faulty::new(Fault::TrustsSavedType), &scenario()).await;
}
