//! Rule R112: Fenêtre d'illustration proportionnelle.
mod common;

use common::*;

// given: une illustration de 800 × 1200 envoyée en mode fenêtre
// when: le DM l'agrandit par glisser-déposer dans son onglet player view
// then: la fenêtre garde un rapport largeur sur hauteur de 2/3, et sa position et sa taille sont les mêmes sur la player view projetée
#[test]
fn r112_proportional_illustration_window() {
    rule(
        "R112",
        &[
            "data-capability:media.uploadMedia",
            "data-capability:live.showWindowOnPlayerView",
            "data-capability:live.moveWindow",
            "data-capability:live.resizeWindow",
            "capability:live.getDiffusion",
            "capability:campaign.listCampaigns",
            "capability:media.listMedia",
            "data-capability:campaign.createCampaign",
            "data-capability:session.createSession",
            "capability:session.listSessions",
            "data-capability:live.setMicrophone",
            "data-capability:live.showCover",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            // A window floats over a base: it needs a live session whose diffusion is committed.
            new_session(s, &campaign, &[])?;
            apply(s, "live.setMicrophone", json!({ "on": true }), None)?;
            apply(s, "live.showCover", json!({ "campaign": campaign }), None)?;
            let title = unique("Illustration 800 x 1200");
            apply(
                s,
                "media.uploadMedia",
                json!({ "campaign": campaign, "kind": "illustration", "title": title, "width": 800, "height": 1200 }),
                None,
            )?;
            let listed = items(
                &s.read("media.listMedia", json!({ "campaign": campaign }))?,
                "illustrations",
            );
            let media =
                by_name_or_title(&listed, &title).ok_or("the illustration is not listed")?;
            apply(
                s,
                "live.showWindowOnPlayerView",
                json!({ "campaign": campaign, "media": media }),
                None,
            )?;

            let window = |s: &Server, view: &str| -> Result<Value, String> {
                let data = s.read(
                    "live.getDiffusion",
                    json!({ "campaign": campaign, "view": view }),
                )?;
                Ok(data
                    .pointer("/diffusion/window")
                    .or(data.get("window"))
                    .cloned()
                    .unwrap_or(Value::Null))
            };
            let number = |w: &Value, k: &str| {
                w[k].as_f64()
                    .ok_or_else(|| format!("the window has no {k}"))
            };
            let before = window(s, "dm")?;
            check(!before.is_null(), "no floating window was opened")?;

            // The DM drags the corner to a square: the window keeps the image's proportions.
            // Position and size are fractions of the screen's width (0 to 1), not pixels.
            apply(
                s,
                "live.moveWindow",
                json!({ "campaign": campaign, "x": 0.2, "y": 0.1 }),
                None,
            )?;
            apply(
                s,
                "live.resizeWindow",
                json!({ "campaign": campaign, "w": 0.6, "h": 0.6 }),
                None,
            )?;
            let dm = window(s, "dm")?;
            let (w, h) = (number(&dm, "w")?, number(&dm, "h")?);
            check(
                (w / h - 2.0 / 3.0).abs() < 0.01,
                "the window lost the image's 2/3 proportions",
            )?;
            check(w > number(&before, "w")?, "the window was not enlarged")?;

            // The projected player view shows the same position and size.
            let projected = window(s, "player")?;
            for key in ["x", "y", "w", "h"] {
                check(
                    projected[key] == dm[key],
                    &format!("the player view's {key} differs from the DM's"),
                )?;
            }

            Ok(())
        },
    );
}
