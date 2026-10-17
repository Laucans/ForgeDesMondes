//! The contract suite of the `llm` port: what every adapter does the same.
//!
//! An adapter crate runs it against itself from a test of its own, on
//! whatever stands in for its provider (a local server, a stub program):
//!
//! ```ignore
//! #[tokio::test]
//! async fn the_adapter_passes_the_contract() {
//!     let scenario = Scenario { recognised: vec![/* ... */], unrecognised: vec![/* ... */] };
//!     ports_llm::contract::run(&MyAdapter::new(), &scenario).await;
//! }
//! ```
//!
//! It holds the adapter to: a successful connection test names the provider
//! reached and both models; an unrecognised API type fails with the Concept's
//! message and sends nothing; a generation is answered by the model of its
//! kind; the journal lists, in order, each request with its kind and its text
//! only; and the key appears nowhere the port speaks. A failed check panics
//! with the provider it was about.

use crate::{
    ApiType, CONNECTION_TEST_PROMPT, GenerateRequest, Llm, LlmError, Provider, ProviderConfig,
    RequestKind, TextKind, UNRECOGNISED_API_TYPE,
};

/// A configuration the adapter must reach, and the provider it must say it
/// reached.
#[derive(Clone, Debug)]
pub struct Recognised {
    /// The configuration handed to the port.
    pub config: ProviderConfig,
    /// The provider the report must name.
    pub provider: Provider,
}

/// What the adapter's provider stand-in makes of the configurations.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// Configurations that connect: a cloud URL, an opencode URL, a binary.
    pub recognised: Vec<Recognised>,
    /// API configurations whose URL is not recognised.
    pub unrecognised: Vec<ProviderConfig>,
}

/// Runs the whole suite on `llm`.
///
/// # Panics
///
/// When the adapter breaks the contract.
pub async fn run<L: Llm>(llm: &L, scenario: &Scenario) {
    assert!(
        !scenario.recognised.is_empty(),
        "the scenario names no provider to reach"
    );
    connection_tests_name_the_provider_and_the_models(llm, scenario).await;
    an_unrecognised_api_type_fails_and_sends_nothing(llm, scenario).await;
    generation_is_answered_by_the_model_of_its_kind(llm, scenario).await;
    the_journal_lists_each_request_in_order_with_its_text_only(llm, scenario).await;
    the_key_is_spoken_nowhere(llm, scenario).await;
}

async fn connection_tests_name_the_provider_and_the_models<L: Llm>(llm: &L, scenario: &Scenario) {
    for Recognised { config, provider } in &scenario.recognised {
        let report = llm.test_connection(config).await;
        assert!(
            report.ok,
            "{provider:?}: the test failed: {}",
            report.message
        );
        assert_eq!(
            report.provider,
            Some(*provider),
            "{provider:?}: provider reached"
        );
        let models = config.models();
        for model in [&models.generation, &models.summary] {
            assert!(
                report.message.contains(model.as_str()),
                "{provider:?}: the message {:?} does not name the model {model:?}",
                report.message
            );
        }
    }
}

async fn an_unrecognised_api_type_fails_and_sends_nothing<L: Llm>(llm: &L, scenario: &Scenario) {
    for config in &scenario.unrecognised {
        let before = llm.journal().list().len();
        let report = llm.test_connection(config).await;
        assert!(!report.ok, "{config:?}: an unrecognised API type connected");
        assert_eq!(
            report.provider, None,
            "{config:?}: provider of a failed test"
        );
        assert!(
            report.message.contains(UNRECOGNISED_API_TYPE),
            "{config:?}: the message {:?} does not say {UNRECOGNISED_API_TYPE:?}",
            report.message
        );
        // A type a saved provider carries is no proof for a test: it detects anew.
        if let ProviderConfig::Api {
            url, key, models, ..
        } = config
        {
            let hinted = ProviderConfig::Api {
                url: url.clone(),
                api_type: Some(ApiType::Cloud),
                key: key.clone(),
                models: models.clone(),
            };
            assert!(
                !llm.test_connection(&hinted).await.ok,
                "{config:?}: a saved API type stood for the detection"
            );
        }
        let request = GenerateRequest::new(TextKind::Prompt, "bonjour");
        assert_eq!(
            llm.generate(config, &request).await,
            Err(LlmError::UnrecognisedApiType),
            "{config:?}: generation through an unrecognised API type"
        );
        assert_eq!(
            llm.journal().list().len(),
            before,
            "{config:?}: a request was journalled though none could be sent"
        );
    }
}

async fn generation_is_answered_by_the_model_of_its_kind<L: Llm>(llm: &L, scenario: &Scenario) {
    for Recognised { config, provider } in &scenario.recognised {
        for kind in TextKind::ALL {
            let request = GenerateRequest::new(kind, format!("texte de test ({kind:?})"));
            let answer = llm
                .generate(config, &request)
                .await
                .unwrap_or_else(|error| panic!("{provider:?}, {kind:?}: {error}"));
            assert!(
                !answer.text.trim().is_empty(),
                "{provider:?}, {kind:?}: an empty answer"
            );
            assert_eq!(
                answer.model,
                config.models().for_kind(kind),
                "{provider:?}, {kind:?}: the model that answered"
            );
        }
    }
}

async fn the_journal_lists_each_request_in_order_with_its_text_only<L: Llm>(
    llm: &L,
    scenario: &Scenario,
) {
    for Recognised { config, provider } in &scenario.recognised {
        journal_of_one_provider(llm, config, *provider).await;
    }
}

async fn journal_of_one_provider<L: Llm>(llm: &L, config: &ProviderConfig, provider: Provider) {
    let before = llm.journal().list().len();

    llm.test_connection(config).await;
    let sent = [
        (
            TextKind::Notes,
            "Les notes de la séance : la taverne brûle.",
        ),
        (TextKind::Journal, "00:01 Elowen entre dans la taverne."),
        (
            TextKind::Brief,
            "Une embuscade dans la forêt, trois gobelins.",
        ),
    ];
    for (kind, text) in sent {
        llm.generate(config, &GenerateRequest::new(kind, text))
            .await
            .unwrap_or_else(|error| panic!("{provider:?}, {kind:?}: {error}"));
    }

    let journal = llm.journal().list();
    let new = &journal[before..];
    assert_eq!(
        new.len(),
        1 + sent.len(),
        "{provider:?}: requests journalled"
    );
    assert!(
        journal
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence),
        "{provider:?}: the sequence numbers do not increase"
    );
    assert_eq!(new[0].kind, RequestKind::ConnectionTest);
    assert_eq!(new[0].text, CONNECTION_TEST_PROMPT);
    for (record, (kind, text)) in new[1..].iter().zip(sent) {
        assert_eq!(record.kind, RequestKind::from(kind), "{provider:?}: kind");
        assert_eq!(record.text, text, "{provider:?}: the text journalled");
    }
    for record in new {
        assert_eq!(
            record.provider, provider,
            "{provider:?}: provider journalled"
        );
    }
}

async fn the_key_is_spoken_nowhere<L: Llm>(llm: &L, scenario: &Scenario) {
    let configs = scenario
        .recognised
        .iter()
        .map(|recognised| &recognised.config)
        .chain(&scenario.unrecognised);
    for config in configs {
        let ProviderConfig::Api { key: Some(key), .. } = config else {
            continue;
        };
        // Whatever the outcome — a report, an answer, an error — and whatever
        // the kind of request.
        let mut spoken = format!("{:?}", llm.test_connection(config).await);
        for kind in TextKind::ALL {
            let result = llm
                .generate(config, &GenerateRequest::new(kind, "bonjour"))
                .await;
            spoken.push_str(&format!(" {result:?}"));
            if let Err(error) = result {
                spoken.push_str(&format!(" {error}"));
            }
        }
        spoken.push_str(&format!(" {:?}", llm.journal().list()));
        assert!(
            !spoken.contains(key.expose()),
            "{config:?}: the key leaked into a report, an answer, an error or the journal"
        );
    }
}
