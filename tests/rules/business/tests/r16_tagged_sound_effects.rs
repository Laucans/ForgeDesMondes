//! Rule R16: Soundbox d'effets tagués.
mod common;

use common::*;

// given: trois sons tagués
// when: le DM filtre par tag et en joue deux à la fois
// then: chacun respecte son volume propre et le volume global, « Tout arrêter » coupe tout, et aucun son ne sort de la player view.
#[test]
fn r16_tagged_sound_effects() {
    rule(
        "R16",
        &[
            "data-capability:soundbox.addSound",
            "data-capability:soundbox.updateSound",
            "data-capability:soundbox.playSound",
            "data-capability:soundbox.setMasterVolume",
            "data-capability:soundbox.stopAll",
            "capability:soundbox.listSounds",
            "capability:live.getDiffusion",
            "capability:campaign.listCampaigns",
            "data-capability:campaign.createCampaign",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let tag = unique("tag");
            let mut ids = Vec::new();
            for (name, tags, volume) in [
                ("A", vec![tag.as_str(), "x"], 30),
                ("B", vec![tag.as_str()], 60),
                ("C", vec!["y"], 90),
            ] {
                let name = unique(name);
                apply(
                    s,
                    "soundbox.addSound",
                    json!({ "campaign": campaign, "name": name, "tags": tags, "file": "son.ogg" }),
                    None,
                )?;
                let sounds = items(
                    &s.read("soundbox.listSounds", json!({ "campaign": campaign }))?,
                    "sounds",
                );
                let id = by_name_or_title(&sounds, &name).ok_or("the sound is not listed")?;
                apply(
                    s,
                    "soundbox.updateSound",
                    json!({ "volume": volume }),
                    Some(&id),
                )?;
                ids.push(id);
            }
            let sounds = |s: &Server| -> Result<Vec<Value>, String> {
                Ok(items(
                    &s.read("soundbox.listSounds", json!({ "campaign": campaign }))?,
                    "sounds",
                ))
            };

            // Filtered by the tag, two of the three sounds are played together.
            let tagged: Vec<String> = sounds(s)?
                .iter()
                .filter(|v| {
                    v["tags"]
                        .as_array()
                        .is_some_and(|t| t.iter().any(|x| *x == tag))
                })
                .map(|v| text(v, "id").to_string())
                .collect();
            check(
                tagged.len() == 2 && tagged.iter().all(|t| ids.contains(t)),
                "the tag filter must keep two sounds",
            )?;
            apply(
                s,
                "soundbox.setMasterVolume",
                json!({ "campaign": campaign, "volume": 50 }),
                None,
            )?;
            for id in &tagged {
                apply(s, "soundbox.playSound", json!({}), Some(id))?;
            }

            let playing = sounds(s)?;
            for (id, own) in [(&ids[0], 30.0), (&ids[1], 60.0)] {
                let sound = by_id(&playing, id).ok_or("a sound vanished")?;
                check(sound["playing"] == true, "a played sound is not playing")?;
                let gain = sound["gain"].as_f64().ok_or("no effective volume")?;
                check(
                    (gain - own * 50.0 / 100.0).abs() < 0.5,
                    "the volume is not the sound's times the master's",
                )?;
            }
            check(
                by_id(&playing, &ids[2]).is_some_and(|c| c["playing"] == false),
                "the third sound plays",
            )?;

            // The two sounds play, and none of them goes out through the player view.
            let diffusion = s.read(
                "live.getDiffusion",
                json!({ "campaign": campaign, "view": "player" }),
            )?;
            let carried = diffusion.to_string();
            check(
                !carried.contains("\"playing\":true")
                    && !tagged.iter().any(|id| carried.contains(id.as_str())),
                "a sound is carried by the player view",
            )?;

            apply(s, "soundbox.stopAll", json!({ "campaign": campaign }), None)?;
            check(
                sounds(s)?.iter().all(|v| v["playing"] == false),
                "« Tout arrêter » left a sound playing",
            )?;

            Ok(())
        },
    );
}
