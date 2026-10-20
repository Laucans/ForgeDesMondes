//! Rule R111: Sons simultanés et volumes.
mod common;

use common::*;

// given: « bruit de foule » joue depuis la soundbox
// when: le DM déclenche « porte qui grince » depuis la pastille, baisse le volume global, puis clique sur « Tout arrêter »
// then: les deux sons se superposent, tous deux baissent avec le volume global en gardant leur rapport, puis plus aucun son ne joue
#[test]
fn r111_simultaneous_sounds_and_volumes() {
    rule(
        "R111",
        &[
            "data-capability:soundbox.addSound",
            "data-capability:soundbox.playSound",
            "data-capability:soundbox.setMasterVolume",
            "data-capability:soundbox.stopAll",
            "capability:soundbox.listSounds",
            "capability:campaign.listCampaigns",
            "data-capability:campaign.createCampaign",
        ],
        |s| {
            let campaign = new_campaign(s, "oneshot")?;
            let mut ids = Vec::new();
            for (name, volume) in [("Bruit de foule", 55), ("Porte qui grince", 80)] {
                let name = unique(name);
                apply(
                    s,
                    "soundbox.addSound",
                    json!({ "campaign": campaign, "name": name, "tags": ["taverne"], "volume": volume, "file": "son.ogg" }),
                    None,
                )?;
                let sounds = items(
                    &s.read("soundbox.listSounds", json!({ "campaign": campaign }))?,
                    "sounds",
                );
                ids.push(by_name_or_title(&sounds, &name).ok_or("the sound is not listed")?);
            }
            let (crowd, door) = (&ids[0], &ids[1]);
            apply(
                s,
                "soundbox.setMasterVolume",
                json!({ "campaign": campaign, "volume": 80 }),
                None,
            )?;
            let read = |s: &Server| -> Result<Vec<Value>, String> {
                Ok(items(
                    &s.read("soundbox.listSounds", json!({ "campaign": campaign }))?,
                    "sounds",
                ))
            };
            let gain = |list: &[Value], id: &str| -> Result<f64, String> {
                by_id(list, id)
                    .and_then(|v| v["gain"].as_f64())
                    .ok_or_else(|| "no effective volume".to_string())
            };

            // The crowd plays from the soundbox, the door from the pill.
            apply(s, "soundbox.playSound", json!({}), Some(crowd))?;
            apply(
                s,
                "soundbox.playSound",
                json!({ "source": "suggestion" }),
                Some(door),
            )?;
            let both = read(s)?;
            check(
                by_id(&both, crowd).is_some_and(|v| v["playing"] == true)
                    && by_id(&both, door).is_some_and(|v| v["playing"] == true),
                "the two sounds do not overlap",
            )?;
            let (crowd_high, door_high) = (gain(&both, crowd)?, gain(&both, door)?);

            // The master goes down: both go down, their ratio kept.
            apply(
                s,
                "soundbox.setMasterVolume",
                json!({ "campaign": campaign, "volume": 40 }),
                None,
            )?;
            let lower = read(s)?;
            let (crowd_low, door_low) = (gain(&lower, crowd)?, gain(&lower, door)?);
            check(
                crowd_low < crowd_high && door_low < door_high,
                "the master volume did not lower both sounds",
            )?;
            check(
                ((door_low / crowd_low) - (door_high / crowd_high)).abs() < 0.02,
                "the ratio between the sounds changed",
            )?;

            apply(s, "soundbox.stopAll", json!({ "campaign": campaign }), None)?;
            check(
                read(s)?.iter().all(|v| v["playing"] == false),
                "a sound still plays after « Tout arrêter »",
            )?;

            Ok(())
        },
    );
}
