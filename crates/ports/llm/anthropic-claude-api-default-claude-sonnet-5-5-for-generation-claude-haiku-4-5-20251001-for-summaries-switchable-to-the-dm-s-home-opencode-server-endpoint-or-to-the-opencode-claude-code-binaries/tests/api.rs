//! The API-mode adapter against local stand-ins for its providers: the
//! `llm` port's contract suite for each client, and the behaviours the suite
//! does not name — headers, the key, the models, the ways an endpoint fails.
//! No network: every request goes to a server this file starts on 127.0.0.1.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use ports_llm::contract::{Recognised, Scenario, run};
use ports_llm::{
    Binary, GenerateRequest, Llm, LlmError, Models, Provider, ProviderConfig, RequestKind, Secret,
    TextKind,
};
use ports_llm_adapters::ApiLlm;
use serde_json::{Value, json};

const GENERATION: &str = "claude-sonnet-5-5";
const SUMMARY: &str = "claude-haiku-4-5-20251001";
const CLOUD_KEY: &str = "sk-ant-api03-très-secret";
const HOME_KEY: &str = "clé-du-serveur-maison";
const CLOUD_HOST: &str = "api.anthropic.com";

// --- the stand-ins ---------------------------------------------------------

/// What a stand-in plays.
#[derive(Clone, Copy)]
enum Flavour {
    /// The Anthropic Messages API: it checks `x-api-key` and the version, and
    /// knows [`GENERATION`] and [`SUMMARY`].
    Anthropic { key: &'static str },
    /// The home opencode server: the OpenAI shapes, knowing the same models;
    /// `bearer` is the key it demands, if it demands one.
    Opencode { bearer: Option<&'static str> },
    /// A server that is none of them: JSON for everything.
    Opaque,
    /// A server that answers every request with this status and an error object.
    Failing(u16),
    /// A proxy that answers every request with this status and a page.
    Page(u16),
    /// A server that answers every request with a redirect to this URL.
    Redirecting(&'static str),
    /// The home server of a model that spends its tokens thinking: it lists
    /// its models, and its completions have an empty content.
    Reasoning,
}

/// One request a stand-in received.
struct Seen {
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Value,
}

struct Stub {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

#[derive(Clone)]
struct StubState {
    flavour: Flavour,
    delay: Duration,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Stub {
    async fn start(flavour: Flavour) -> Self {
        Self::start_slow(flavour, Duration::ZERO).await
    }

    /// A stand-in that waits `delay` before every answer.
    async fn start_slow(flavour: Flavour, delay: Duration) -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let state = StubState {
            flavour,
            delay,
            seen: Arc::clone(&seen),
        };
        let app = Router::new().fallback(respond).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { addr, seen }
    }

    /// `http://127.0.0.1:<port>`: a host that is not the cloud's.
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// `http://api.anthropic.com:<port>/v1`, which [`cloud_client`] resolves
    /// to this stand-in: the cloud host, over plain HTTP, on a local port.
    fn cloud_url(&self) -> String {
        format!("http://{CLOUD_HOST}:{}/v1", self.addr.port())
    }

    fn requests(&self) -> Vec<(String, String)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|seen| (seen.method.to_string(), seen.path.clone()))
            .collect()
    }

    /// The requests received, by method and path.
    fn count(&self, method: &str, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|(m, p)| m == method && p == path)
            .count()
    }

    fn with_seen<T>(&self, method: &str, path: &str, read: impl Fn(&Seen) -> T) -> Vec<T> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| seen.method.as_str() == method && seen.path == path)
            .map(read)
            .collect()
    }
}

fn reply(status: u16, body: &Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn page(status: u16, text: &str) -> Response {
    (StatusCode::from_u16(status).unwrap(), text.to_owned()).into_response()
}

fn header_of(headers: &HeaderMap, name: &str) -> String {
    // Raw bytes: a key may hold more than visible ASCII.
    headers
        .get(name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default()
}

fn known(model: &str) -> bool {
    model == GENERATION || model == SUMMARY
}

async fn respond(
    State(state): State<StubState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    tokio::time::sleep(state.delay).await;
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let path = uri.path().to_owned();
    state.seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        headers: headers.clone(),
        body: body.clone(),
    });
    let model = body["model"].as_str().unwrap_or_default();
    match (state.flavour, method.as_str(), path.as_str()) {
        (Flavour::Failing(status), _, _) => {
            reply(status, &json!({"error": {"message": "en panne"}}))
        }
        (Flavour::Page(status), _, _) => page(status, "404 Not Found"),
        (Flavour::Redirecting(to), _, _) => {
            (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, to)]).into_response()
        }
        (Flavour::Reasoning, "POST", _) => reply(
            200,
            &json!({"object": "chat.completion", "model": model, "choices": [
                {"index": 0, "message": {"role": "assistant", "content": ""}}]}),
        ),
        (Flavour::Reasoning, _, _) => reply(
            200,
            &json!({"object": "list", "data": [
                {"id": GENERATION, "object": "model"}, {"id": SUMMARY, "object": "model"}]}),
        ),
        (Flavour::Opaque, _, _) => reply(200, &json!({"hello": "world"})),
        (Flavour::Anthropic { key }, _, _) if header_of(&headers, "x-api-key") != key => {
            // A provider that says too much: it quotes the key it was given.
            let sent = header_of(&headers, "x-api-key");
            reply(
                401,
                &json!({"type": "error", "error": {
                    "type": "authentication_error", "message": format!("invalid x-api-key: {sent}")}}),
            )
        }
        (Flavour::Anthropic { .. }, _, _)
            if header_of(&headers, "anthropic-version") != "2023-06-01" =>
        {
            reply(400, &json!({"error": {"message": "anthropic-version"}}))
        }
        (Flavour::Anthropic { .. }, "POST", "/v1/messages") if known(model) => reply(
            200,
            &json!({"type": "message", "role": "assistant", "model": model,
                    "content": [{"type": "text", "text": format!("ok de {model}")}]}),
        ),
        (Flavour::Anthropic { .. }, "GET", path) if path.starts_with("/v1/models/") => {
            let id = path.trim_start_matches("/v1/models/");
            if known(id) {
                reply(200, &json!({"type": "model", "id": id}))
            } else {
                reply(
                    404,
                    &json!({"type": "error", "error": {
                    "type": "not_found_error", "message": format!("model: {id}")}}),
                )
            }
        }
        (Flavour::Anthropic { .. }, "POST", "/v1/messages") => reply(
            404,
            &json!({"type": "error", "error": {
                "type": "not_found_error", "message": format!("model: {model}")}}),
        ),
        (Flavour::Opencode { bearer: Some(key) }, _, _)
            if header_of(&headers, "authorization") != format!("Bearer {key}") =>
        {
            reply(401, &json!({"error": {"message": "unauthorized"}}))
        }
        (Flavour::Opencode { .. }, "GET", "/v1/models") => reply(
            200,
            &json!({"object": "list", "data": [
                {"id": GENERATION, "object": "model"}, {"id": SUMMARY, "object": "model"}]}),
        ),
        (Flavour::Opencode { .. }, "POST", "/v1/chat/completions") if known(model) => reply(
            200,
            &json!({"object": "chat.completion", "model": model, "choices": [
                {"index": 0, "message": {"role": "assistant", "content": format!("ok de {model}")}}]}),
        ),
        (Flavour::Opencode { .. }, "POST", "/v1/chat/completions") => reply(
            404,
            &json!({"error": {"message": format!("The model `{model}` does not exist")}}),
        ),
        _ => reply(404, &json!({"error": {"message": "no route"}})),
    }
}

/// A client that reaches `api.anthropic.com` at the stand-in.
fn cloud_client(stub: &Stub, default_key: Option<&str>) -> ApiLlm {
    let http = reqwest::Client::builder().resolve(CLOUD_HOST, stub.addr);
    ApiLlm::with_builder(http, default_key.map(Secret::new)).allowing_cleartext_cloud()
}

fn llm() -> ApiLlm {
    ApiLlm::new(None)
}

fn models() -> Models {
    Models {
        generation: GENERATION.into(),
        summary: SUMMARY.into(),
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

fn api_with(url: &str, key: Option<&str>, generation: &str, summary: &str) -> ProviderConfig {
    ProviderConfig::Api {
        url: url.into(),
        api_type: None,
        key: key.map(Secret::new),
        models: Models {
            generation: generation.into(),
            summary: summary.into(),
        },
    }
}

fn prompt(text: &str) -> GenerateRequest {
    GenerateRequest::new(TextKind::Prompt, text)
}

/// A port on which nothing listens.
async fn closed_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

// --- the contract suite ----------------------------------------------------

#[tokio::test]
async fn the_anthropic_client_passes_the_contract() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let opaque = Stub::start(Flavour::Opaque).await;
    let scenario = Scenario {
        recognised: vec![Recognised {
            config: api(cloud.cloud_url(), Some(CLOUD_KEY)),
            provider: Provider::Cloud,
        }],
        unrecognised: vec![
            api(opaque.url(), Some(CLOUD_KEY)),
            api("pas une url", Some(CLOUD_KEY)),
            api("ftp://api.anthropic.com", None),
        ],
    };
    run(&cloud_client(&cloud, None), &scenario).await;
}

#[tokio::test]
async fn the_opencode_client_passes_the_contract() {
    let home = Stub::start(Flavour::Opencode {
        bearer: Some(HOME_KEY),
    })
    .await;
    let opaque = Stub::start(Flavour::Opaque).await;
    let scenario = Scenario {
        recognised: vec![Recognised {
            config: api(home.url(), Some(HOME_KEY)),
            provider: Provider::OpencodeServer,
        }],
        unrecognised: vec![api(opaque.url(), Some(HOME_KEY)), api("pas une url", None)],
    };
    run(&llm(), &scenario).await;
}

#[tokio::test]
async fn one_client_passes_the_contract_for_both_apis() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    let scenario = Scenario {
        recognised: vec![
            Recognised {
                config: api(cloud.cloud_url(), Some(CLOUD_KEY)),
                provider: Provider::Cloud,
            },
            Recognised {
                config: api(home.url(), None),
                provider: Provider::OpencodeServer,
            },
        ],
        unrecognised: vec![api("ftp://serveur", None)],
    };
    run(&cloud_client(&cloud, None), &scenario).await;
}

// --- the Anthropic Messages API --------------------------------------------

#[tokio::test]
async fn the_cloud_speaks_messages_with_the_key_and_the_pinned_version() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    let report = client
        .test_connection(&api(cloud.cloud_url(), Some(CLOUD_KEY)))
        .await;
    assert!(report.ok, "{}", report.message);

    let versions = cloud.with_seen("POST", "/v1/messages", |seen| {
        header_of(&seen.headers, "anthropic-version")
    });
    assert_eq!(versions, ["2023-06-01"]);
    let keys = cloud.with_seen("POST", "/v1/messages", |seen| {
        header_of(&seen.headers, "x-api-key")
    });
    assert_eq!(keys, [CLOUD_KEY]);
    // The summary model is looked up, not prompted.
    assert_eq!(cloud.count("GET", &format!("/v1/models/{SUMMARY}")), 1);
    assert_eq!(cloud.count("POST", "/v1/messages"), 1);
}

#[tokio::test]
async fn the_test_prompts_the_generation_model_with_text_only() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    client
        .test_connection(&api(cloud.cloud_url(), Some(CLOUD_KEY)))
        .await;
    let bodies = cloud.with_seen("POST", "/v1/messages", |seen| seen.body.clone());
    assert_eq!(
        bodies,
        [json!({
            "model": GENERATION,
            "max_tokens": 16,
            "messages": [{"role": "user", "content": ports_llm::CONNECTION_TEST_PROMPT}],
        })]
    );
}

#[tokio::test]
async fn a_generation_sends_its_text_to_the_model_of_its_kind() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    let config = api(cloud.cloud_url(), Some(CLOUD_KEY));
    for (kind, model) in [(TextKind::Notes, GENERATION), (TextKind::Journal, SUMMARY)] {
        let answer = client
            .generate(&config, &GenerateRequest::new(kind, "00:01 la taverne"))
            .await
            .unwrap();
        assert_eq!(answer.model, model);
        assert_eq!(answer.text, format!("ok de {model}"));
    }
    let sent = cloud.with_seen("POST", "/v1/messages", |seen| {
        (
            seen.body["model"].as_str().unwrap().to_owned(),
            seen.body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .to_owned(),
            seen.body
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(sent[0].0, GENERATION);
    assert_eq!(sent[1].0, SUMMARY);
    assert!(sent.iter().all(|(_, text, _)| text == "00:01 la taverne"));
    // The body holds a model, a limit and messages: no other field to carry a payload.
    assert!(
        sent.iter()
            .all(|(_, _, keys)| keys == &["max_tokens", "messages", "model"])
    );
}

#[tokio::test]
async fn the_key_is_the_settings_then_llm_api_key() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let url = cloud.cloud_url();

    // The settings carry none: the environment's key serves.
    let fallback = cloud_client(&cloud, Some(CLOUD_KEY));
    assert!(fallback.test_connection(&api(&url, None)).await.ok);

    // The settings carry one: it wins over the environment's.
    let wrong_fallback = cloud_client(&cloud, Some("sk-ant-de-l-environnement"));
    assert!(
        wrong_fallback
            .test_connection(&api(&url, Some(CLOUD_KEY)))
            .await
            .ok
    );
    assert!(
        !wrong_fallback.test_connection(&api(&url, None)).await.ok,
        "the environment's key was the wrong one"
    );
}

#[tokio::test]
async fn the_cloud_without_any_key_sends_nothing() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    let config = api(cloud.cloud_url(), None);

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::Cloud));
    assert!(
        report.message.contains("clé API absente"),
        "{}",
        report.message
    );
    let error = client
        .generate(&config, &prompt("bonjour"))
        .await
        .unwrap_err();
    assert!(matches!(&error, LlmError::Failed(why) if why.contains("clé API absente")));

    assert!(cloud.requests().is_empty(), "{:?}", cloud.requests());
    assert!(client.journal().list().is_empty());
}

#[tokio::test]
async fn a_wrong_key_is_refused_and_the_message_does_not_quote_it() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    let wrong = "sk-ant-api03-mauvaise-clé";
    let config = api(cloud.cloud_url(), Some(wrong));

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.message.contains("refusé"), "{}", report.message);
    let error = client
        .generate(&config, &prompt("bonjour"))
        .await
        .unwrap_err();

    // The stand-in quoted the key back; nothing the port says does.
    let spoken = format!("{report:?} {error} {error:?} {:?}", client.journal().list());
    assert!(!spoken.contains(wrong), "{spoken}");
    assert!(!spoken.contains("mauvaise"), "{spoken}");
}

#[tokio::test]
async fn an_unknown_model_is_named() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, None);
    let url = cloud.cloud_url();

    let summary = client
        .test_connection(&api_with(
            &url,
            Some(CLOUD_KEY),
            GENERATION,
            "claude-inconnu-1",
        ))
        .await;
    assert!(!summary.ok);
    assert_eq!(summary.provider, Some(Provider::Cloud));
    assert!(
        summary
            .message
            .contains("modèle inconnu : claude-inconnu-1"),
        "{}",
        summary.message
    );

    let generation = client
        .test_connection(&api_with(
            &url,
            Some(CLOUD_KEY),
            "claude-inconnu-2",
            SUMMARY,
        ))
        .await;
    assert!(!generation.ok);
    assert!(
        generation
            .message
            .contains("modèle inconnu : claude-inconnu-2"),
        "{}",
        generation.message
    );
}

#[tokio::test]
async fn the_latency_is_the_time_the_prompt_took() {
    let cloud = Stub::start_slow(
        Flavour::Anthropic { key: CLOUD_KEY },
        Duration::from_millis(60),
    )
    .await;
    let report = cloud_client(&cloud, None)
        .test_connection(&api(cloud.cloud_url(), Some(CLOUD_KEY)))
        .await;
    assert!(report.ok);
    assert!(report.latency_ms >= 60, "{}", report.latency_ms);
}

#[tokio::test]
async fn a_cloud_that_does_not_answer_is_unreachable() {
    let port = closed_port().await;
    let http =
        reqwest::Client::builder().resolve(CLOUD_HOST, SocketAddr::from(([127, 0, 0, 1], port)));
    let client = ApiLlm::with_builder(http, None).allowing_cleartext_cloud();
    let config = api(format!("http://{CLOUD_HOST}:{port}/v1"), Some(CLOUD_KEY));

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::Cloud));
    assert!(report.message.contains("injoignable"), "{}", report.message);
    let error = client
        .generate(&config, &prompt("bonjour"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, LlmError::Failed(why) if why.contains("injoignable")),
        "{error}"
    );
}

#[tokio::test]
async fn an_error_status_is_a_failure_with_its_reason() {
    let broken = Stub::start(Flavour::Failing(503)).await;
    let client = cloud_client(&broken, None);
    let report = client
        .test_connection(&api(broken.cloud_url(), Some(CLOUD_KEY)))
        .await;
    assert!(!report.ok);
    assert!(report.message.contains("HTTP 503"), "{}", report.message);
    assert!(report.message.contains("en panne"), "{}", report.message);
}

#[tokio::test]
async fn a_request_that_failed_is_still_journalled() {
    let broken = Stub::start(Flavour::Failing(500)).await;
    let client = cloud_client(&broken, None);
    let config = api(broken.cloud_url(), Some(CLOUD_KEY));
    client.test_connection(&config).await;
    let _ = client
        .generate(
            &config,
            &GenerateRequest::new(TextKind::Brief, "une embuscade"),
        )
        .await;
    let journal = client.journal().list();
    assert_eq!(
        journal.iter().map(|record| record.kind).collect::<Vec<_>>(),
        [RequestKind::ConnectionTest, RequestKind::Brief]
    );
    assert_eq!(journal[1].text, "une embuscade");
}

// --- the opencode endpoint -------------------------------------------------

#[tokio::test]
async fn the_home_server_speaks_chat_completions() {
    let home = Stub::start(Flavour::Opencode {
        bearer: Some(HOME_KEY),
    })
    .await;
    let config = api(home.url(), Some(HOME_KEY));
    let client = llm();

    let report = client.test_connection(&config).await;
    assert!(report.ok, "{}", report.message);
    assert_eq!(report.provider, Some(Provider::OpencodeServer));
    let answer = client
        .generate(
            &config,
            &GenerateRequest::new(TextKind::Journal, "00:01 la taverne"),
        )
        .await
        .unwrap();
    assert_eq!(
        (answer.model.as_str(), answer.text.as_str()),
        (SUMMARY, "ok de claude-haiku-4-5-20251001")
    );

    let bodies = home.with_seen("POST", "/v1/chat/completions", |seen| seen.body.clone());
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[1]["model"], SUMMARY);
    assert_eq!(
        bodies[1]["messages"],
        json!([{"role": "user", "content": "00:01 la taverne"}])
    );
    // The key is a bearer token, and no Anthropic header is sent to the server.
    let headers = home.with_seen("POST", "/v1/chat/completions", |seen| {
        (
            header_of(&seen.headers, "authorization"),
            header_of(&seen.headers, "x-api-key"),
        )
    });
    assert!(
        headers
            .iter()
            .all(|(auth, x)| auth == &format!("Bearer {HOME_KEY}") && x.is_empty())
    );
}

#[tokio::test]
async fn a_server_that_needs_no_key_is_sent_none() {
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    let report = llm().test_connection(&api(home.url(), None)).await;
    assert!(report.ok, "{}", report.message);
    let auth = home.with_seen("POST", "/v1/chat/completions", |seen| {
        seen.headers.contains_key("authorization")
    });
    assert_eq!(auth, [false]);
}

#[tokio::test]
async fn the_endpoint_may_be_typed_with_its_v1_or_a_path_of_the_api() {
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    for typed in [
        format!("{}/v1", home.url()),
        format!("{}/v1/chat/completions", home.url()),
        format!("{}/", home.url()),
    ] {
        let report = llm().test_connection(&api(&typed, None)).await;
        assert!(report.ok, "{typed}: {}", report.message);
    }
}

#[tokio::test]
async fn a_home_server_without_the_summary_model_is_an_unknown_model() {
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    let report = llm()
        .test_connection(&api_with(&home.url(), None, GENERATION, "qwen-absent"))
        .await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::OpencodeServer));
    assert!(
        report.message.contains("modèle inconnu : qwen-absent"),
        "{}",
        report.message
    );

    let report = llm()
        .test_connection(&api_with(&home.url(), None, "qwen-absent", SUMMARY))
        .await;
    assert!(!report.ok);
    assert!(
        report.message.contains("modèle inconnu : qwen-absent"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn a_saved_api_type_skips_the_detection_when_generating() {
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    let ProviderConfig::Api {
        url, key, models, ..
    } = api(home.url(), None)
    else {
        unreachable!()
    };
    let saved = ProviderConfig::Api {
        url,
        api_type: Some(ports_llm::ApiType::Opencode),
        key,
        models,
    };
    llm().generate(&saved, &prompt("bonjour")).await.unwrap();
    assert_eq!(home.count("GET", "/v1/models"), 0);
    assert_eq!(home.count("POST", "/v1/chat/completions"), 1);
}

// --- a URL that is neither ---------------------------------------------------

#[tokio::test]
async fn an_unrecognised_url_is_refused_before_any_prompt() {
    let opaque = Stub::start(Flavour::Opaque).await;
    let client = llm();
    let config = api(opaque.url(), Some(CLOUD_KEY));

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, None);
    assert_eq!(report.message, "type d'API non reconnu");
    assert_eq!(
        client.generate(&config, &prompt("bonjour")).await,
        Err(LlmError::UnrecognisedApiType)
    );

    // All it was asked: the list of models, which the type is told from.
    assert!(
        opaque
            .requests()
            .iter()
            .all(|(m, p)| (m.as_str(), p.as_str()) == ("GET", "/v1/models"))
    );
    assert!(client.journal().list().is_empty());
}

#[tokio::test]
async fn what_is_not_a_url_sends_nothing_at_all() {
    // The only listener there is would hear it.
    let opaque = Stub::start(Flavour::Opaque).await;
    let client = llm();
    for typed in [
        "",
        "pas une url",
        "ftp://127.0.0.1",
        "https:///v1",
        "127.0.0.1",
    ] {
        let report = client.test_connection(&api(typed, Some(CLOUD_KEY))).await;
        assert_eq!(
            (report.ok, report.message.as_str()),
            (false, "type d'API non reconnu"),
            "{typed:?}"
        );
    }
    assert!(opaque.requests().is_empty());
    assert!(client.journal().list().is_empty());
}

#[tokio::test]
async fn a_look_alike_of_the_cloud_host_is_not_the_cloud() {
    // The key must never be sent to a host that only looks like Anthropic's.
    let opaque = Stub::start(Flavour::Opaque).await;
    let http = reqwest::Client::builder()
        .resolve("api.anthropic.com.example.org", opaque.addr)
        .resolve("evil.example", opaque.addr);
    let client =
        ApiLlm::with_builder(http, Some(Secret::new(CLOUD_KEY))).allowing_cleartext_cloud();
    for typed in [
        format!(
            "http://api.anthropic.com.example.org:{}/v1",
            opaque.addr.port()
        ),
        format!(
            "http://evil.example\\@api.anthropic.com:{}/v1",
            opaque.addr.port()
        ),
    ] {
        let report = client
            .test_connection(&api(typed.clone(), Some(CLOUD_KEY)))
            .await;
        assert!(!report.ok, "{typed}");
        assert_eq!(report.provider, None, "{typed}");
    }
    assert_eq!(opaque.count("POST", "/v1/messages"), 0);
}

#[tokio::test]
async fn an_unrecognised_endpoint_that_does_not_answer_says_why() {
    let port = closed_port().await;
    let report = llm()
        .test_connection(&api(format!("http://127.0.0.1:{port}"), None))
        .await;
    assert!(!report.ok);
    assert!(report.message.contains("injoignable"), "{}", report.message);
    assert!(
        report.message.ends_with("type d'API non reconnu"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn a_server_that_answers_an_error_to_the_list_is_unrecognised() {
    let broken = Stub::start(Flavour::Failing(500)).await;
    let report = llm().test_connection(&api(broken.url(), None)).await;
    assert!(!report.ok);
    assert_eq!(report.provider, None);
    assert!(report.message.contains("HTTP 500"), "{}", report.message);
    assert!(
        report.message.ends_with("type d'API non reconnu"),
        "{}",
        report.message
    );
}

// --- the other mode --------------------------------------------------------

#[tokio::test]
async fn a_binary_is_not_for_this_client() {
    let client = llm();
    let config = ProviderConfig::Binary {
        binary: Binary::ClaudeCode,
        models: models(),
    };
    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert!(matches!(
        client.generate(&config, &prompt("bonjour")).await,
        Err(LlmError::Failed(_))
    ));
    assert!(client.journal().list().is_empty());
}

// --- where the key and the prompt may go -----------------------------------

#[tokio::test]
async fn the_cloud_is_not_spoken_to_in_clear() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    // The same client as the others, without the test's leave for plain HTTP.
    let http = reqwest::Client::builder().resolve(CLOUD_HOST, cloud.addr);
    let client = ApiLlm::with_builder(http, None);
    let config = api(cloud.cloud_url(), Some(CLOUD_KEY));

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert_eq!(report.provider, Some(Provider::Cloud));
    assert!(report.message.contains("https"), "{}", report.message);
    assert!(matches!(
        client.generate(&config, &prompt("bonjour")).await,
        Err(LlmError::Failed(why)) if why.contains("https")
    ));
    assert!(cloud.requests().is_empty());
    assert!(client.journal().list().is_empty());
}

#[tokio::test]
async fn the_environments_key_is_the_clouds_and_never_the_home_servers() {
    let home = Stub::start(Flavour::Opencode { bearer: None }).await;
    let client = ApiLlm::new(Some(Secret::new(CLOUD_KEY)));
    assert!(client.test_connection(&api(home.url(), None)).await.ok);
    client
        .generate(&api(home.url(), None), &prompt("bonjour"))
        .await
        .unwrap();
    let sent = home.with_seen("POST", "/v1/chat/completions", |seen| {
        seen.headers.contains_key("authorization")
    });
    assert_eq!(sent, [false, false]);
    let listed = home.with_seen("GET", "/v1/models", |seen| {
        seen.headers.contains_key("authorization")
    });
    assert!(listed.iter().all(|sent| !sent));
}

#[tokio::test]
async fn a_blank_key_in_the_settings_leaves_the_environments_key_to_the_cloud() {
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let client = cloud_client(&cloud, Some(CLOUD_KEY));
    for blank in ["", "  "] {
        let report = client
            .test_connection(&api(cloud.cloud_url(), Some(blank)))
            .await;
        assert!(report.ok, "{blank:?}: {}", report.message);
    }
}

#[tokio::test]
async fn a_redirect_is_not_followed_with_the_key_or_the_prompt() {
    let elsewhere = Stub::start(Flavour::Opencode { bearer: None }).await;
    let target: &'static str =
        Box::leak(format!("{}/v1/chat/completions", elsewhere.url()).into_boxed_str());
    let redirecting = Stub::start(Flavour::Redirecting(target)).await;
    let client = cloud_client(&redirecting, None);
    let config = api(redirecting.cloud_url(), Some(CLOUD_KEY));

    let report = client.test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.message.contains("HTTP 307"), "{}", report.message);
    assert!(client.generate(&config, &prompt("bonjour")).await.is_err());
    assert!(
        elsewhere.requests().is_empty(),
        "{:?}",
        elsewhere.requests()
    );
}

#[tokio::test]
async fn a_saved_cloud_type_does_not_send_the_key_to_another_host() {
    // The provider was saved as the cloud; its URL has since been changed.
    let opaque = Stub::start(Flavour::Opaque).await;
    let saved = ProviderConfig::Api {
        url: opaque.url(),
        api_type: Some(ports_llm::ApiType::Cloud),
        key: Some(Secret::new(CLOUD_KEY)),
        models: models(),
    };
    assert_eq!(
        llm().generate(&saved, &prompt("bonjour")).await,
        Err(LlmError::UnrecognisedApiType)
    );
    assert_eq!(opaque.count("POST", "/v1/messages"), 0);
    let keys = opaque.with_seen("GET", "/v1/models", |seen| {
        header_of(&seen.headers, "x-api-key")
    });
    assert!(keys.iter().all(String::is_empty), "{keys:?}");
}

// --- what an endpoint can answer ---------------------------------------------

#[tokio::test]
async fn a_404_page_is_a_wrong_address_not_an_unknown_model() {
    let proxy = Stub::start(Flavour::Page(404)).await;
    let report = cloud_client(&proxy, None)
        .test_connection(&api(proxy.cloud_url(), Some(CLOUD_KEY)))
        .await;
    assert!(!report.ok);
    assert!(report.message.contains("HTTP 404"), "{}", report.message);
    assert!(
        !report.message.contains("modèle inconnu"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn a_model_that_answers_nothing_still_connects_but_generates_nothing() {
    let home = Stub::start(Flavour::Reasoning).await;
    let client = llm();
    let config = api(home.url(), None);
    let report = client.test_connection(&config).await;
    assert!(report.ok, "{}", report.message);
    let error = client
        .generate(&config, &prompt("bonjour"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, LlmError::Failed(why) if why.contains("illisible")),
        "{error}"
    );
}

#[tokio::test]
async fn a_saved_home_server_type_does_not_send_chat_completions_to_the_cloud() {
    // The provider was saved as the home server; its URL has since become the cloud's.
    let cloud = Stub::start(Flavour::Anthropic { key: CLOUD_KEY }).await;
    let saved = ProviderConfig::Api {
        url: cloud.cloud_url(),
        api_type: Some(ports_llm::ApiType::Opencode),
        key: Some(Secret::new(CLOUD_KEY)),
        models: models(),
    };
    let client = cloud_client(&cloud, None);
    client.generate(&saved, &prompt("bonjour")).await.unwrap();
    assert_eq!(cloud.count("POST", "/v1/messages"), 1);
    assert_eq!(cloud.count("POST", "/v1/chat/completions"), 0);
    assert_eq!(client.journal().list()[0].provider, Provider::Cloud);
}
