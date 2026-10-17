//! The adapters of the `llm` port (`crates/ports/llm`), the crate
//! `ports.json` names as its `adapter`.
//!
//! **API mode** is [`ApiLlm`]: one [`Llm`] over two clients, chosen by the API
//! type `ProviderApiType@1` tells from the URL the DM entered, never by a
//! setting.
//!
//! - [`anthropic`]: the Anthropic Messages API, `anthropic-version`
//!   2023-06-01, the key in `x-api-key`.
//! - [`opencode_endpoint`]: the OpenAI-compatible chat completions of the
//!   DM's home opencode server, the key (if any) as a bearer token.
//!
//! The key is the one the configuration carries (the DM's settings), else
//! `LLM_API_KEY`. It is sent in a header and nowhere else: not in a report,
//! an error, the journal or a log.
//!
//! Only text leaves: a [`GenerateRequest`] holds nothing else. A URL whose API
//! type is not recognised is refused before any prompt is sent; to tell a
//! home server from a stranger the one thing asked of it is its list of
//! models, which is the Concept's own input.

use std::env;
use std::time::{Duration, Instant};

use ports_llm::{
    ApiType, CONNECTION_TEST_PROMPT, ConnectionReport, GenerateAnswer, GenerateRequest, Llm,
    LlmError, Models, Provider, ProviderConfig, RequestJournal, RequestKind, Secret,
    UNRECOGNISED_API_TYPE, detect_api_type,
};
use reqwest::{Client, ClientBuilder, Url, redirect};
use serde_json::{Value, json};

mod anthropic;
mod http;
mod opencode_endpoint;

use http::Failure;

/// The environment variable of the port's secret.
const KEY_VARIABLE: &str = "LLM_API_KEY";

/// What the connection test lets a model write: a word is enough.
const TEST_MAX_TOKENS: u32 = 16;
/// What a generation lets a model write.
const GENERATE_MAX_TOKENS: u32 = 4096;
/// A connection test, and each call of it, gives up after this.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Asking an endpoint for its list of models.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// A generation can be long: a brief, a whole session's journal.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(180);

/// What an API-mode call says to a configuration that is not an API one.
const NOT_AN_API: &str = "ce client ne parle qu'aux API : le mode binaire est un autre adaptateur";
/// What a call says when the cloud API has no key to be sent.
const NO_KEY: &str = "clé API absente : renseignez-la dans les réglages ou dans LLM_API_KEY";
/// What a call says to the cloud API's URL in clear: the key would travel in it.
const CLEARTEXT: &str = "l'API cloud n'accepte que https : la clé API ne circule pas en clair";

/// The `llm` port over HTTP: the Anthropic Messages API, or the opencode
/// server's endpoint.
///
/// The key of a request is the one the configuration carries. Only the cloud
/// API falls back to `LLM_API_KEY` (it is the cloud's key): the home server
/// is sent what the DM saved for it, or nothing. No request follows a
/// redirect, so neither a key nor a prompt goes anywhere the DM did not type.
#[derive(Clone, Debug)]
pub struct ApiLlm {
    http: Client,
    /// The cloud API's key when the configuration carries none (`LLM_API_KEY`).
    default_key: Option<Secret>,
    /// Whether `http://api.anthropic.com` is spoken to; only a test says yes.
    cleartext_cloud: bool,
    journal: RequestJournal,
}

impl ApiLlm {
    /// A client whose fallback key for the cloud API is `default_key`.
    ///
    /// # Panics
    ///
    /// When the TLS backend cannot be initialised.
    #[must_use]
    pub fn new(default_key: Option<Secret>) -> Self {
        Self::with_builder(Client::builder(), default_key)
    }

    /// A client whose fallback key is `LLM_API_KEY`, if it is set and not blank.
    ///
    /// # Panics
    ///
    /// When the TLS backend cannot be initialised.
    #[must_use]
    // Nothing in this workspace builds the adapter yet: the selector of the
    // binary-mode task and the server will, with the process's environment.
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        Self::new(fallback_key(env::var(KEY_VARIABLE).ok()))
    }

    /// A client over an HTTP client `builder` already set (a proxy, a
    /// resolver); its redirect policy is replaced by none.
    ///
    /// # Panics
    ///
    /// When the TLS backend cannot be initialised.
    #[must_use]
    pub fn with_builder(builder: ClientBuilder, default_key: Option<Secret>) -> Self {
        let http = builder
            .redirect(redirect::Policy::none())
            .build()
            .expect("the HTTP client's TLS backend initialises");
        Self {
            http,
            default_key,
            cleartext_cloud: false,
            journal: RequestJournal::new(),
        }
    }

    /// The same client, speaking to the cloud API over plain HTTP too: for a
    /// stand-in on a local port, which cannot have a certificate.
    #[must_use]
    pub fn allowing_cleartext_cloud(mut self) -> Self {
        self.cleartext_cloud = true;
        self
    }

    /// The key the cloud API is sent: the configuration's, else the fallback.
    fn cloud_key<'a>(&'a self, config_key: Option<&'a Secret>) -> Option<&'a Secret> {
        usable(config_key).or(self.default_key.as_ref())
    }

    /// Why the cloud API is not spoken to at `url`, if it is not.
    fn cloud_refusal(&self, url: &str) -> Option<&'static str> {
        let clear = Url::parse(url.trim()).is_ok_and(|url| url.scheme() == "http");
        (clear && !self.cleartext_cloud).then_some(CLEARTEXT)
    }

    /// The API type behind `url`, asking the endpoint for its list of models
    /// when the URL alone cannot tell (`ProviderApiType@1`). The endpoint is
    /// asked with `key` only if the configuration has one: whatever it is,
    /// the DM saved it for this URL.
    async fn detect(&self, url: &str, key: Option<&Secret>) -> Detection {
        if let Some(api_type) = detect_api_type(url, None) {
            return Detection::Recognised(api_type, None);
        }
        // The Concept's own reading of "a URL with a host that a list of
        // models could make a provider": anything else has nothing to ask, and
        // nothing is sent. (A URL parser alone reads `https:///v1` as the host
        // `v1`.)
        let minimal_list = json!({"object": "list", "data": []});
        if detect_api_type(url, Some(&minimal_list)).is_none() {
            return Detection::Unrecognised(None);
        }
        match opencode_endpoint::list_models(&self.http, url, key, PROBE_TIMEOUT).await {
            Ok(list) => match detect_api_type(url, Some(&list)) {
                Some(api_type) => Detection::Recognised(api_type, Some(list)),
                None => Detection::Unrecognised(None),
            },
            Err(failure) => Detection::Unrecognised(Some(failure.reason())),
        }
    }

    /// Prompts the cloud's generation model, then checks its summary model
    /// exists. Returns how long the prompt took.
    async fn test_cloud(&self, url: &str, key: &Secret, models: &Models) -> Result<u64, Failure> {
        self.journal.record(
            RequestKind::ConnectionTest,
            Provider::Cloud,
            CONNECTION_TEST_PROMPT,
        );
        let started = Instant::now();
        anthropic::complete(
            &self.http,
            url,
            key,
            &models.generation,
            CONNECTION_TEST_PROMPT,
            TEST_MAX_TOKENS,
            TEST_TIMEOUT,
        )
        .await?;
        let latency = elapsed_ms(started);
        anthropic::check_model(&self.http, url, key, &models.summary, TEST_TIMEOUT).await?;
        Ok(latency)
    }

    /// Prompts the home server's generation model, then checks its list of
    /// models holds the summary model. Returns how long the prompt took.
    async fn test_opencode(
        &self,
        url: &str,
        key: Option<&Secret>,
        models: &Models,
        listed: Option<Value>,
    ) -> Result<u64, Failure> {
        self.journal.record(
            RequestKind::ConnectionTest,
            Provider::OpencodeServer,
            CONNECTION_TEST_PROMPT,
        );
        let started = Instant::now();
        opencode_endpoint::complete(
            &self.http,
            url,
            key,
            &models.generation,
            CONNECTION_TEST_PROMPT,
            TEST_MAX_TOKENS,
            TEST_TIMEOUT,
        )
        .await?;
        let latency = elapsed_ms(started);
        let list = match listed {
            Some(list) => list,
            None => opencode_endpoint::list_models(&self.http, url, key, TEST_TIMEOUT).await?,
        };
        if opencode_endpoint::lists(&list, &models.summary) {
            Ok(latency)
        } else {
            Err(Failure::UnknownModel(models.summary.clone()))
        }
    }
}

/// A key that is something to send: a blank one is none.
fn usable(key: Option<&Secret>) -> Option<&Secret> {
    key.filter(|key| !key.expose().trim().is_empty())
}

/// What asking a URL what it is came to.
enum Detection {
    /// Its type, and the list of models the endpoint answered if it was asked.
    Recognised(ApiType, Option<Value>),
    /// Not recognised, and why the endpoint could not tell if it was asked.
    Unrecognised(Option<String>),
}

/// The key `LLM_API_KEY` holds: none when it is unset or blank.
fn fallback_key(value: Option<String>) -> Option<Secret> {
    value.filter(|key| !key.trim().is_empty()).map(Secret::new)
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn failed(provider: Option<Provider>, message: String) -> ConnectionReport {
    ConnectionReport {
        ok: false,
        provider,
        latency_ms: 0,
        message,
    }
}

impl Llm for ApiLlm {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        let ProviderConfig::Api {
            url, key, models, ..
        } = config
        else {
            return failed(None, NOT_AN_API.to_owned());
        };
        let config_key = usable(key.as_ref());
        // A test always detects anew: the type a saved provider carries is no
        // proof the URL is still what it was.
        let (api_type, listed) = match self.detect(url, config_key).await {
            Detection::Recognised(api_type, listed) => (api_type, listed),
            Detection::Unrecognised(why) => {
                let message = match why {
                    Some(why) => format!("{why} — {UNRECOGNISED_API_TYPE}"),
                    None => UNRECOGNISED_API_TYPE.to_owned(),
                };
                return failed(None, message);
            }
        };
        let provider = Provider::from(api_type);
        let tested = match api_type {
            ApiType::Cloud => {
                if let Some(refusal) = self.cloud_refusal(url) {
                    return failed(Some(provider), refusal.to_owned());
                }
                let Some(key) = self.cloud_key(config_key) else {
                    return failed(Some(provider), NO_KEY.to_owned());
                };
                self.test_cloud(url, key, models).await
            }
            ApiType::Opencode => self.test_opencode(url, config_key, models, listed).await,
        };
        match tested {
            Ok(latency_ms) => ConnectionReport {
                ok: true,
                provider: Some(provider),
                latency_ms,
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
        let ProviderConfig::Api {
            url,
            api_type,
            key,
            models,
        } = config
        else {
            return Err(LlmError::Failed(NOT_AN_API.to_owned()));
        };
        let config_key = usable(key.as_ref());
        // A saved type spares the endpoint a question, but only the home
        // server's: the cloud is told by its host, so a URL that changed since
        // the type was saved cannot be sent the cloud's key.
        let api_type = match api_type {
            Some(ApiType::Opencode) if detect_api_type(url, None).is_none() => ApiType::Opencode,
            _ => match self.detect(url, config_key).await {
                Detection::Recognised(api_type, _) => api_type,
                Detection::Unrecognised(_) => return Err(LlmError::UnrecognisedApiType),
            },
        };
        let provider = Provider::from(api_type);
        let model = models.for_kind(request.kind);
        let sent = match api_type {
            ApiType::Cloud => {
                if let Some(refusal) = self.cloud_refusal(url) {
                    return Err(LlmError::Failed(refusal.to_owned()));
                }
                let key = self
                    .cloud_key(config_key)
                    .ok_or_else(|| LlmError::Failed(NO_KEY.to_owned()))?;
                self.journal
                    .record(RequestKind::from(request.kind), provider, &request.text);
                anthropic::complete(
                    &self.http,
                    url,
                    key,
                    model,
                    &request.text,
                    GENERATE_MAX_TOKENS,
                    GENERATE_TIMEOUT,
                )
                .await
            }
            ApiType::Opencode => {
                self.journal
                    .record(RequestKind::from(request.kind), provider, &request.text);
                opencode_endpoint::complete(
                    &self.http,
                    url,
                    config_key,
                    model,
                    &request.text,
                    GENERATE_MAX_TOKENS,
                    GENERATE_TIMEOUT,
                )
                .await
            }
        };
        match sent {
            Ok(text) if text.trim().is_empty() => {
                Err(LlmError::Failed(Failure::Malformed.reason()))
            }
            Ok(text) => Ok(GenerateAnswer {
                text,
                model: model.to_owned(),
            }),
            Err(failure) => Err(LlmError::Failed(failure.reason())),
        }
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_or_blank_llm_api_key_is_no_key() {
        assert_eq!(fallback_key(None), None);
        assert_eq!(fallback_key(Some(String::new())), None);
        assert_eq!(fallback_key(Some("  ".into())), None);
        assert_eq!(
            fallback_key(Some("sk-ant-1".into())),
            Some(Secret::new("sk-ant-1"))
        );
    }

    #[test]
    fn a_configuration_key_wins_over_the_fallback_and_a_blank_one_is_none() {
        let llm = ApiLlm::new(Some(Secret::new("env")));
        let own = Secret::new("settings");
        assert_eq!(llm.cloud_key(Some(&own)), Some(&own));
        assert_eq!(llm.cloud_key(None), Some(&Secret::new("env")));
        assert_eq!(
            llm.cloud_key(Some(&Secret::new(" "))),
            Some(&Secret::new("env"))
        );
        assert_eq!(ApiLlm::new(None).cloud_key(None), None);
    }

    #[test]
    fn the_cloud_is_spoken_to_over_https_only_unless_a_test_allows_it() {
        let llm = ApiLlm::new(None);
        assert_eq!(
            llm.cloud_refusal("http://api.anthropic.com/v1"),
            Some(CLEARTEXT)
        );
        assert_eq!(
            llm.cloud_refusal(" HTTP://api.anthropic.com"),
            Some(CLEARTEXT)
        );
        assert_eq!(llm.cloud_refusal("https://api.anthropic.com/v1"), None);
        let local = llm.allowing_cleartext_cloud();
        assert_eq!(
            local.cloud_refusal("http://api.anthropic.com:8080/v1"),
            None
        );
    }
}
