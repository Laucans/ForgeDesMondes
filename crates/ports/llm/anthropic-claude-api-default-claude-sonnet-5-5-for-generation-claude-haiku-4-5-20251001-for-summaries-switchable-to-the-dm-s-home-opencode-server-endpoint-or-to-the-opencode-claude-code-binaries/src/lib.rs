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
use reqwest::Client;
use serde_json::{Value, json};

mod anthropic;
mod http;
mod opencode_endpoint;

use http::{Failure, api_base};

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

/// The `llm` port over HTTP: the Anthropic Messages API, or the opencode
/// server's endpoint.
#[derive(Clone, Debug)]
pub struct ApiLlm {
    http: Client,
    /// The key to use when a configuration carries none (`LLM_API_KEY`).
    default_key: Option<Secret>,
    journal: RequestJournal,
}

impl ApiLlm {
    /// A client whose fallback key is `default_key`.
    ///
    /// # Panics
    ///
    /// When the TLS backend cannot be initialised.
    #[must_use]
    pub fn new(default_key: Option<Secret>) -> Self {
        Self::with_client(Client::new(), default_key)
    }

    /// A client whose fallback key is `LLM_API_KEY`, if it is set and not empty.
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

    /// A client over an HTTP client already built, for one that needs a
    /// setting of its own (a proxy, a resolver).
    #[must_use]
    pub fn with_client(http: Client, default_key: Option<Secret>) -> Self {
        Self {
            http,
            default_key,
            journal: RequestJournal::new(),
        }
    }

    /// The key a call uses: the configuration's, else the fallback.
    fn key<'a>(&'a self, config_key: Option<&'a Secret>) -> Option<&'a Secret> {
        config_key.or(self.default_key.as_ref())
    }

    /// The API type behind `url`, asking the endpoint for its list of models
    /// when the URL alone cannot tell (`ProviderApiType@1`).
    async fn detect(&self, url: &str, key: Option<&Secret>) -> Detection {
        if let Some(api_type) = detect_api_type(url, None) {
            return Detection::Recognised(api_type, None);
        }
        // No list could make this URL a provider (not an http(s) URL with a
        // host): nothing is sent.
        if detect_api_type(url, Some(&json!({"object": "list", "data": []}))).is_none() {
            return Detection::Unrecognised(None);
        }
        let list =
            opencode_endpoint::list_models(&self.http, &api_base(url), key, PROBE_TIMEOUT).await;
        match list {
            Ok(list) => match detect_api_type(url, Some(&list)) {
                Some(api_type) => Detection::Recognised(api_type, Some(list)),
                None => Detection::Unrecognised(None),
            },
            Err(failure) => Detection::Unrecognised(Some(failure.reason())),
        }
    }

    /// Sends the connection-test prompt to the generation model, then checks
    /// the summary model exists. Returns how long the prompt took.
    async fn probe(
        &self,
        api_type: ApiType,
        url: &str,
        key: Option<&Secret>,
        models: &Models,
        model_list: Option<Value>,
    ) -> Result<u64, Failure> {
        let base = api_base(url);
        let provider = Provider::from(api_type);
        self.journal.record(
            RequestKind::ConnectionTest,
            provider,
            CONNECTION_TEST_PROMPT,
        );
        let started = Instant::now();
        let latency = match api_type {
            ApiType::Cloud => {
                let key = key.ok_or(Failure::Refused(401))?;
                anthropic::complete(
                    &self.http,
                    &base,
                    key,
                    &models.generation,
                    CONNECTION_TEST_PROMPT,
                    TEST_MAX_TOKENS,
                    TEST_TIMEOUT,
                )
                .await?;
                elapsed_ms(started)
            }
            ApiType::Opencode => {
                opencode_endpoint::complete(
                    &self.http,
                    &base,
                    key,
                    &models.generation,
                    CONNECTION_TEST_PROMPT,
                    TEST_MAX_TOKENS,
                    TEST_TIMEOUT,
                )
                .await?;
                elapsed_ms(started)
            }
        };
        match api_type {
            ApiType::Cloud => {
                let key = key.ok_or(Failure::Refused(401))?;
                anthropic::check_model(&self.http, &base, key, &models.summary, TEST_TIMEOUT)
                    .await?;
            }
            ApiType::Opencode => {
                let list = match model_list {
                    Some(list) => list,
                    None => {
                        opencode_endpoint::list_models(&self.http, &base, key, TEST_TIMEOUT).await?
                    }
                };
                if !opencode_endpoint::lists(&list, &models.summary) {
                    return Err(Failure::UnknownModel(models.summary.clone()));
                }
            }
        }
        Ok(latency)
    }
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
        let key = self.key(key.as_ref());
        // A test always detects anew: the type a saved provider carries is no
        // proof the URL is still what it was.
        let (api_type, model_list) = match self.detect(url, key).await {
            Detection::Recognised(api_type, list) => (api_type, list),
            Detection::Unrecognised(why) => {
                let message = match why {
                    Some(why) => format!("{why} — {UNRECOGNISED_API_TYPE}"),
                    None => UNRECOGNISED_API_TYPE.to_owned(),
                };
                return failed(None, message);
            }
        };
        let provider = Provider::from(api_type);
        if api_type == ApiType::Cloud && key.is_none() {
            return failed(Some(provider), NO_KEY.to_owned());
        }
        match self.probe(api_type, url, key, models, model_list).await {
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
        let key = self.key(key.as_ref());
        let api_type = match api_type {
            Some(api_type) => *api_type,
            None => match self.detect(url, key).await {
                Detection::Recognised(api_type, _) => api_type,
                Detection::Unrecognised(_) => return Err(LlmError::UnrecognisedApiType),
            },
        };
        let provider = Provider::from(api_type);
        let model = models.for_kind(request.kind);
        let base = api_base(url);
        let sent = match api_type {
            ApiType::Cloud => {
                let key = key.ok_or_else(|| LlmError::Failed(NO_KEY.to_owned()))?;
                self.journal
                    .record(RequestKind::from(request.kind), provider, &request.text);
                anthropic::complete(
                    &self.http,
                    &base,
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
                    &base,
                    key,
                    model,
                    &request.text,
                    GENERATE_MAX_TOKENS,
                    GENERATE_TIMEOUT,
                )
                .await
            }
        };
        match sent {
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
    fn a_configuration_key_wins_over_the_fallback() {
        let llm = ApiLlm::new(Some(Secret::new("env")));
        let own = Secret::new("settings");
        assert_eq!(llm.key(Some(&own)), Some(&own));
        assert_eq!(llm.key(None), Some(&Secret::new("env")));
        assert_eq!(ApiLlm::new(None).key(None), None);
    }
}
