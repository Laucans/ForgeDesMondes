//! Rule R92: Détection automatique du type d'API.
mod common;

use common::*;

// given: deux URL valides, l'une d'une API cloud et l'autre de l'endpoint opencode du serveur maison, ainsi qu'une URL qui ne répond pas à un type d'API connu
// when: le DM saisit tour à tour chacune de ces URL en mode « API » et teste la connexion, sans préciser le type d'API
// then: le type correct est détecté et affiché pour chacune des deux premières URL et la connexion réussit, tandis que la troisième URL est refusée avec un message indiquant que le type d'API n'est pas reconnu
#[test]
fn r92_api_type_detected_from_the_url() {
    rule(
        "R92",
        &[
            "data-capability:ai.testConnection",
            "capability:ai.getSettings",
        ],
        |s| {
            let settings = fixture("AiSettings");
            let models = &settings["providers"]["opencode"];
            let cases = [
                ("https://api.anthropic.com/v1", Some("cloud")),
                ("http://192.168.1.40:4096", Some("opencode")),
                ("https://inconnu.example.org/pas-une-api", None),
            ];
            for (url, kind) in cases {
                // The DM gives the URL, never the type.
                let result = s.command(
                    "ai.testConnection",
                    json!({
                        "mode": "api",
                        "url": url,
                        "generationModel": models["generationModel"],
                        "summaryModel": models["summaryModel"]
                    }),
                    None,
                )?;
                let last = s.read("ai.getSettings", json!({}))?["lastTest"].clone();
                match kind {
                    Some(expected) => {
                        check(
                            result["status"] == "applied",
                            &format!("{url}: the test was not applied"),
                        )?;
                        check(
                            last["ok"] == true,
                            &format!("{url}: the connection did not succeed"),
                        )?;
                        check(
                            last["provider"] == expected,
                            &format!("{url}: the type detected is not {expected}"),
                        )?;
                    }
                    None => {
                        check(
                            last["ok"] == false,
                            &format!("{url}: the connection was not refused"),
                        )?;
                        check(
                            text(&last, "message").contains("reconnu"),
                            &format!(
                                "{url}: the refusal does not say the API type is not recognised"
                            ),
                        )?;
                    }
                }
            }

            Ok(())
        },
    );
}
