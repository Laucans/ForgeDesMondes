//! The registry's glue, as the product's units make it: every unit found,
//! its plugin called, what it needs told. The product's own tests of an
//! installed crate: `src/` and `build.rs` are the harness's.

use std::any::type_name;
use std::collections::BTreeMap;
use std::future::Future;
use std::task::{Context, Poll, Waker};

use dataguard_registry::{AGGREGATES, CAPABILITIES, DATA_CAPABILITIES, Unit, plugins};
use harness_dataguard::plugins::Plugin;
use harness_dataguard::reads::Reads;

/// `future`'s output: a read over no database waits on nothing.
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

#[test]
fn every_unit_folder_is_found_with_its_manifest_and_its_plugin() {
    for units in [AGGREGATES, DATA_CAPABILITIES, CAPABILITIES] {
        assert!(!units.is_empty());
        for unit in units {
            assert!(
                unit.manifest.is_some(),
                "{} holds its manifest",
                unit.folder
            );
            assert!(unit.plugin.is_some(), "{} is a crate", unit.folder);
        }
    }
    assert!(
        AGGREGATES
            .iter()
            .any(|unit| unit.folder == "crates/campaign/aggregates/Entity")
    );
}

#[test]
fn the_plugins_answer_every_capability_the_registry_holds() {
    let reads = Reads::from(&plugins());
    let listed = ready(reads.run("campaign.listCampaigns", &BTreeMap::new()));
    assert_eq!(
        listed.map(|answer| answer.to_string()),
        Ok("[]".to_string()),
        "over no row, none listed"
    );
    assert!(ready(reads.run("nothing.registered", &BTreeMap::new())).is_err());
}

/// A plugin that reads a `String` from what the server shares.
fn needing() -> Plugin {
    Plugin::new().needs::<String>()
}

#[test]
fn a_unit_tells_what_its_plugin_needs() {
    let unit = Unit {
        folder: "crates/test/data-capabilities/needing",
        manifest: None,
        plugin: Some(needing),
    };
    assert_eq!(unit.needs(), [type_name::<String>()]);
    let bare = Unit {
        plugin: None,
        ..unit
    };
    assert!(bare.needs().is_empty());
}
