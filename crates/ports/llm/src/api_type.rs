//! `ProviderApiType@1`, implemented locally: what a provider URL is.
//!
//! The Concept (`concepts/ProviderApiType/v1`) is the meaning, this module is
//! this port's own reading of it, held to the Concept's fixtures by
//! `tests/api_type.rs`. The host `api.anthropic.com` is the cloud API,
//! whatever the endpoint answered; any other host is the home opencode server
//! when the endpoint answered its model list in the OpenAI-compatible shape;
//! everything else is not recognised.

use serde_json::Value;

use crate::ApiType;

/// What a caller is told when the URL is not recognised: the Concept's
/// `message`, and the end of the failing connection test's message.
pub const UNRECOGNISED_API_TYPE: &str = "type d'API non reconnu";

/// The only host told to be the cloud by the URL alone.
const CLOUD_HOST: &str = "api.anthropic.com";

/// The type of the API behind `url`, given the JSON the endpoint answered to
/// its list of models (`None` when it did not answer, or not in JSON).
#[must_use]
pub fn detect_api_type(url: &str, model_list_answer: Option<&Value>) -> Option<ApiType> {
    let host = host_of(url)?;
    if host == CLOUD_HOST {
        Some(ApiType::Cloud)
    } else if model_list_answer.is_some_and(is_openai_model_list) {
        Some(ApiType::Opencode)
    } else {
        None
    }
}

/// The host of an absolute http(s) URL, lowercased, without userinfo or port.
fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    // A backslash ends the authority too, as it does for the clients that
    // will connect: `evil.example\@api.anthropic.com` is the host `evil.example`.
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if host_port.starts_with('[') {
        &host_port[..=host_port.find(']')?]
    } else {
        host_port
            .rsplit_once(':')
            .map_or(host_port, |(host, _)| host)
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `{"object": "list", "data": [...]}`.
fn is_openai_model_list(answer: &Value) -> bool {
    answer["object"] == "list" && answer["data"].is_array()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn host_of_drops_scheme_case_userinfo_port_and_path() {
        assert_eq!(
            host_of("HTTPS://User:pw@API.Anthropic.com:443/v1/messages?x=1#y").as_deref(),
            Some("api.anthropic.com")
        );
        assert_eq!(host_of("http://[::1]:4096").as_deref(), Some("[::1]"));
        assert_eq!(
            host_of("http://192.168.1.40:4096").as_deref(),
            Some("192.168.1.40")
        );
    }

    #[test]
    fn a_backslash_ends_the_authority_like_a_slash() {
        assert_eq!(
            host_of("https://evil.example\\@api.anthropic.com/").as_deref(),
            Some("evil.example")
        );
        assert_eq!(
            detect_api_type("https://evil.example\\@api.anthropic.com/", None),
            None
        );
    }

    #[test]
    fn host_of_refuses_what_is_not_an_http_url_with_a_host() {
        for url in [
            "",
            "pas une url",
            "ftp://api.anthropic.com",
            "https:///v1",
            "http://[::1",
        ] {
            assert_eq!(host_of(url), None, "{url:?}");
        }
    }

    #[test]
    fn the_openai_shape_needs_the_list_object_and_a_data_array() {
        assert!(is_openai_model_list(&json!({"object": "list", "data": []})));
        assert!(!is_openai_model_list(&json!({"object": "list"})));
        assert!(!is_openai_model_list(
            &json!({"object": "model", "data": []})
        ));
        assert!(!is_openai_model_list(
            &json!({"object": "list", "data": {}})
        ));
        assert!(!is_openai_model_list(&json!("list")));
    }

    #[test]
    fn the_cloud_host_wins_over_the_answer_and_needs_none() {
        let other = json!({"error": "x"});
        assert_eq!(
            detect_api_type("https://api.anthropic.com/v1", None),
            Some(ApiType::Cloud)
        );
        assert_eq!(
            detect_api_type("https://api.anthropic.com/v1", Some(&other)),
            Some(ApiType::Cloud)
        );
    }

    #[test]
    fn another_host_is_opencode_only_when_it_answered_the_list() {
        let list = json!({"object": "list", "data": []});
        assert_eq!(
            detect_api_type("http://192.168.1.40:4096", Some(&list)),
            Some(ApiType::Opencode)
        );
        assert_eq!(detect_api_type("http://192.168.1.40:4096", None), None);
        assert_eq!(detect_api_type("pas une url", Some(&list)), None);
    }
}
