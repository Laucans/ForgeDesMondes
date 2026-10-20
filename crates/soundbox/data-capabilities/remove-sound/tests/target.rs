//! The sound a removal names before it is submitted, or why it is refused
//! before it is queued.

use remove_sound::{Refusal, names, target};

const DOOR: &str = "50000000-0000-4000-8000-000000000001";

#[test]
fn the_sound_is_its_target_trimmed_and_lowercased() {
    assert_eq!(target(Some(DOOR)), Ok(DOOR.to_owned()));
    let given = format!(" {} ", DOOR.to_uppercase());
    assert_eq!(target(Some(&given)), Ok(DOOR.to_owned()));
}

#[test]
fn a_removal_naming_no_sound_is_refused() {
    for given in [None, Some(""), Some("   ")] {
        assert_eq!(target(given), Err(Refusal::NoSound), "{given:?}");
    }
}

#[test]
fn the_refusal_says_why_in_french() {
    assert_eq!(Refusal::NoSound.to_string(), "ce son n'existe pas");
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("soundbox.removeSound"));
    assert!(names("soundbox.removeSound@1"));
    assert!(!names("soundbox.updateSound"));
    assert!(!names("soundbox.removeSounds"));
}
