//! `ProviderApiType@1`, as this port implements it, held to the Concept's
//! fixtures.

use platform_conformance::check;
use ports_llm::{ApiType, UNRECOGNISED_API_TYPE, detect_api_type};
use serde_json::{Value, json};

fn classify(input: Value) -> Value {
    let url = input["url"].as_str().unwrap_or_default();
    let answer = input.get("answer").filter(|answer| !answer.is_null());
    match detect_api_type(url, answer) {
        Some(kind) => {
            let kind = match kind {
                ApiType::Cloud => "cloud",
                ApiType::Opencode => "opencode",
            };
            json!({"type": kind, "recognized": true, "message": null})
        }
        None => json!({"type": null, "recognized": false, "message": UNRECOGNISED_API_TYPE}),
    }
}

#[test]
fn conformance() {
    check("ProviderApiType@1", classify);
}
