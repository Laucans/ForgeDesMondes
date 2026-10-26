//! The rules' kit, for the business rules' tests (`tests/*.rs`): a test
//! proves one rule of `rules.json` (contract P), written before the code it
//! proves, and so it reaches the product only the way a Micro-UI does — by
//! identifier, over the shell's wire protocol:
//!
//! - `POST /capabilities/<id>` with `{ "variables": … }` reads a Capability;
//! - `POST /commands` with `{ "dataCapability": "<name>@<v>", "payload": …,
//!   "target": { "id": … } }` sends a command, its result (contract H) read
//!   back at `GET /commands/<id>` until it settles.
//!
//! A rule's test is **pending** while a unit it needs is not built yet (no
//! manifest names it) or no server answers (`RULES_SERVER_URL` unset): it
//! passes, and says so. Once everything it needs exists it runs, and must
//! hold. Each outcome is written, one JSON line, to `RULES_STATUS` (default
//! `target/rules-status.jsonl`), which `scripts/rules-gate.mjs` reads.
//!
//! A test acts as a surface of the product (`surfaces.json`, contract S),
//! as its shell would (caller model A): every command carries the caller of
//! the first screen its needs name (`screen:<s>`, that screen's `surface`;
//! one that declares none acts with no role, fail closed), else of the
//! default surface; [`Server::command_as`] names another surface. A test
//! never writes a caller or a role itself. While the product declares no
//! surfaces, it acts as the fallback approver (`surfaces::FALLBACK_BY`). A
//! command that waits for a review settles red at once, saying which caller
//! lacked which role — a rule that expects the review says so with
//! [`Server::command_reviewed`].
//!
//! A running test is also checked for how it is mounted (`mounting`): a
//! unit it reaches that its needs do not name, a read's variable its
//! `capability.json` `input` does not take, a payload key its
//! `data-capability.json` does not declare (the engine's warning). Any of
//! them makes the rule **mis-mounted** — neither red nor contested, and the
//! test passes, saying so: the rules' task fixes it, never the product.
//!
//! Needs are ids, as the manifests and `plan/graph.json` give them:
//! `capability:<system>.<name>`, `data-capability:<system>.<name>` (no
//! version), `micro-ui:<Name>`, `screen:<name>`, `aggregate:<Name>`.
//!
//! ```ignore
//! use rules_business::{rule, fixture};
//!
//! #[test]
//! fn r3_a_campaign_archived_leaves_the_list() {
//!     // given: a campaign in the list
//!     // when: the DM archives it
//!     // then: it is no longer listed
//!     rule("R3", &["data-capability:campaign.archiveCampaign", "capability:campaign.listCampaigns"], |s| {
//!         let campaign = fixture("campaigns")[0].clone();
//!         s.command("campaign.archiveCampaign@1", serde_json::json!({}), campaign["id"].as_str())?;
//!         let listed = s.read("campaign.listCampaigns", serde_json::json!({}))?;
//!         rules_business::check(!listed.to_string().contains(campaign["id"].as_str().unwrap_or("")), "still listed")
//!     });
//! }
//! ```

use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

mod mounting;
pub mod surfaces;

/// How long a command may take to settle before the test says so.
const SETTLE: Duration = Duration::from_secs(20);

/// The repository's root: this crate sits at `tests/rules/business`.
#[must_use]
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The product's server, as the shell's wire protocol reaches it — and
/// what the running test reached off its mounting.
pub struct Server {
    base: String,
    /// The test's needs, as `rule` was given them.
    needs: Vec<String>,
    /// Each way the test is mis-mounted, in the order found.
    mis_mounted: RefCell<Vec<String>>,
}

/// What a command is expected to become.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// It applies, or is rejected: a review is the shell's defect.
    Settles,
    /// It waits for a review: the rule says so.
    Review,
}

impl Server {
    /// Keeps what is wrong with the test's mounting; refuses the call when
    /// anything is.
    fn mounted(&self, problems: Vec<String>) -> Result<(), String> {
        let Some(first) = problems.first().cloned() else {
            return Ok(());
        };
        self.mis_mounted.borrow_mut().extend(problems);
        Err(format!("mis-mounted: {first}"))
    }

    /// Reads a Capability by its identifier; answers its `data`.
    ///
    /// # Errors
    ///
    /// The server refused it or answered no data; or the test is
    /// mis-mounted: its needs do not name the Capability, or a variable is
    /// not in its manifest's `input` — then nothing is sent.
    pub fn read(&self, capability: &str, variables: Value) -> Result<Value, String> {
        let mut problems: Vec<String> =
            mounting::unnamed_need(&self.needs, "capability", capability)
                .into_iter()
                .collect();
        if let Some((path, manifest)) = named(&format!("capability:{capability}")) {
            problems.extend(mounting::off_input(
                capability,
                &variables,
                &manifest,
                &relative(&path),
            ));
        }
        self.mounted(problems)?;
        let url = format!("{}/capabilities/{capability}", self.base);
        let answer: Value = ureq::post(&url)
            .send_json(json!({ "variables": variables }))
            .map_err(|e| format!("reading {capability}: {e}"))?
            .into_json()
            .map_err(|e| format!("reading {capability}: {e}"))?;
        Ok(answer.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Sends a command to a DataCapability (`<name>@<version>`, the version
    /// 1 when absent), as the surface the test acts on, and waits for its
    /// result (contract H) to settle — and for the follow-ups it queued
    /// (`then`) to settle too.
    ///
    /// # Errors
    ///
    /// The server refused it, it did not settle in time, or it waits for a
    /// review (its caller lacks a critical field's role: the shell's
    /// surfaces, never the rule); or the test is mis-mounted: its needs do
    /// not name the `DataCapability` (nothing is sent), or the engine warned
    /// of a payload key its manifest does not declare.
    pub fn command(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
    ) -> Result<Value, String> {
        self.send(data_capability, payload, target, None, Expect::Settles)
    }

    /// [`Self::command`], as the surface `surface` of `surfaces.json` — a
    /// player's, say — instead of the one the test's screen acts on.
    ///
    /// # Errors
    ///
    /// As [`Self::command`]; and `surface` is no surface the product
    /// declares.
    pub fn command_as(
        &self,
        surface: &str,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
    ) -> Result<Value, String> {
        self.send(
            data_capability,
            payload,
            target,
            Some(surface),
            Expect::Settles,
        )
    }

    /// A command the rule expects to wait for a review: its result once it
    /// does (`awaiting_review`, its `reviewId`).
    ///
    /// # Errors
    ///
    /// It applied or was rejected instead, or as [`Self::command`].
    pub fn command_reviewed(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
    ) -> Result<Value, String> {
        self.send(data_capability, payload, target, None, Expect::Review)
    }

    /// The caller a command carries: of `surface` when named, else of the
    /// first screen the needs name, else of the default surface — the
    /// fallback approver while the product declares none.
    fn caller(&self, surface: Option<&str>) -> Result<surfaces::Caller, String> {
        let declared = match surfaces::read(&root()) {
            None => None,
            Some(read) => Some(read?),
        };
        if surface.is_some() {
            return surfaces::caller(declared.as_ref(), surface, "rules");
        }
        let Some(declared) = declared else {
            return surfaces::caller(None, None, "rules");
        };
        let screen = self
            .needs
            .iter()
            .find_map(|need| need.strip_prefix("screen:"));
        Ok(match screen {
            Some(screen) => {
                let on = named(&format!("screen:{screen}")).and_then(|(_, manifest)| {
                    manifest
                        .get("surface")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                surfaces::on_screen(&declared, on.as_deref(), "rules")
            }
            None => surfaces::caller(Some(&declared), None, "rules")?,
        })
    }

    /// Sends a command as the caller it is owed, and settles it as `expect`
    /// says.
    fn send(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
        surface: Option<&str>,
        expect: Expect,
    ) -> Result<Value, String> {
        self.mounted(
            mounting::unnamed_need(&self.needs, "data-capability", data_capability)
                .into_iter()
                .collect(),
        )?;
        let caller = self.caller(surface)?;
        let name = if data_capability.contains('@') {
            data_capability.to_string()
        } else {
            format!("{data_capability}@1")
        };
        let mut body =
            json!({ "dataCapability": name, "payload": payload, "caller": caller.to_json() });
        if let Some(id) = target {
            body["target"] = json!({ "id": id });
        }
        let accepted: Value = ureq::post(&format!("{}/commands", self.base))
            .send_json(body)
            .map_err(|e| format!("commanding {data_capability}: {e}"))?
            .into_json()
            .map_err(|e| format!("commanding {data_capability}: {e}"))?;
        self.mounted(mounting::off_payload(&accepted))?;
        let mut results = vec![self.settle(data_capability, &accepted)?];
        // A command may queue follow-ups (`then`): it is done when they are.
        for follow_up in accepted
            .get("then")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // On the wire: `{"submitted": {…}}` or `{"refused": {…, "error"}}`.
            match follow_up.get("submitted") {
                Some(submitted) => results.push(self.settle(data_capability, submitted)?),
                None => {
                    return Err(format!(
                        "{data_capability}: a follow-up was refused: {follow_up}"
                    ));
                }
            }
        }
        // A follow-up inherits its leader's caller: one waiting is the same defect.
        let waits = surfaces::parked(&accepted) || results.iter().any(awaits_review);
        let result = results.swap_remove(0);
        match (waits, expect) {
            (true, Expect::Review) | (false, Expect::Settles) => Ok(result),
            (true, Expect::Settles) => Err(parked_line(data_capability, &accepted, &caller)),
            (false, Expect::Review) => Err(format!(
                "{data_capability}: {}, while the rule expects it to wait for a review — the caller {} (surface {}) holds every role its critical fields ask",
                result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("settled"),
                caller.by,
                caller.named()
            )),
        }
    }

    /// Waits for the command an `accepted` answer names to settle (or to
    /// wait for a review).
    fn settle(&self, data_capability: &str, accepted: &Value) -> Result<Value, String> {
        let mut result = accepted.get("result").cloned().unwrap_or(Value::Null);
        let id = accepted
            .get("commandId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let started = Instant::now();
        while !settled(&result) {
            if started.elapsed() > SETTLE {
                return Err(format!(
                    "{data_capability}: not settled after {}s",
                    SETTLE.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
            let read: Value = ureq::get(&format!("{}/commands/{id}", self.base))
                .call()
                .map_err(|e| format!("reading the command {id}: {e}"))?
                .into_json()
                .map_err(|e| format!("reading the command {id}: {e}"))?;
            result = read.get("result").cloned().unwrap_or(Value::Null);
        }
        Ok(result)
    }
}

/// Why a command that waits for a review fails a rule's test: the caller
/// lacked a critical field's role — the product's surfaces, never the rule.
fn parked_line(data_capability: &str, accepted: &Value, caller: &surfaces::Caller) -> String {
    let review = surfaces::review_of(accepted).unwrap_or_else(|| "(no id)".to_string());
    let fix = if root().join(surfaces::FILE).exists() {
        "fix surfaces.json, the screen's surface or the aggregate's review, never the rule"
    } else {
        "declare surfaces.json (contract S) with the role, never the rule"
    };
    format!(
        "{data_capability}: parked for review {review}, the caller {} (surface {}) lacks a critical field's role: {fix}",
        caller.by,
        caller.named()
    )
}

/// A command's result has settled: applied or rejected — or it waits for
/// a review, which no amount of waiting settles.
fn settled(result: &Value) -> bool {
    matches!(
        result.get("status").and_then(Value::as_str),
        Some("applied" | "rejected" | "awaiting_review")
    )
}

/// A result that waits for a review.
fn awaits_review(result: &Value) -> bool {
    result.get("status").and_then(Value::as_str) == Some("awaiting_review")
}

/// A fixture of the mock-up (`mock/fixtures/<name>.json`): the data the
/// human validated with it, a test's given.
///
/// # Panics
///
/// The fixture is not there or not JSON: the test is wrong, not the product.
#[must_use]
pub fn fixture(name: &str) -> Value {
    let path = root().join("mock/fixtures").join(format!("{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A then that does not hold, said.
///
/// # Errors
///
/// `ok` is false.
pub fn check(ok: bool, why: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(why.to_string()) }
}

/// What a manifest names, by the kind of unit a test may need.
const KINDS: [(&str, &str, &str); 5] = [
    ("capability", "capability.json", "capability"),
    ("data-capability", "data-capability.json", "dataCapability"),
    ("micro-ui", "micro-ui.json", "microUi"),
    ("aggregate", "aggregate.json", "aggregate"),
    ("screen", "", "screen"),
];

/// Every manifest file under a folder.
fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == "node_modules")
            {
                continue;
            }
            manifests(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// Every manifest of the product, read once per process: the units do not
/// change while the suite runs, and a test polling the server reads them
/// on every request.
fn index() -> &'static [(PathBuf, Value)] {
    static INDEX: OnceLock<Vec<(PathBuf, Value)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut found = Vec::new();
        manifests(&root().join("crates"), &mut found);
        manifests(&root().join("apps"), &mut found);
        found
            .into_iter()
            .filter_map(|path| {
                let manifest = fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())?;
                Some((path, manifest))
            })
            .collect()
    })
}

/// The manifest that names a unit (`kind:name`, as a rule's test declares
/// what it needs): where it is, and what it says.
fn named(need: &str) -> Option<(PathBuf, Value)> {
    let (kind, name) = need.split_once(':')?;
    let (_, file, field) = KINDS.iter().find(|(k, _, _)| *k == kind)?;
    index().iter().find_map(|(path, manifest)| {
        let named = if file.is_empty() {
            path.parent().is_some_and(|p| p.ends_with("apps/screens"))
        } else {
            path.file_name().is_some_and(|n| n == *file)
        };
        (named && manifest.get(*field).and_then(Value::as_str) == Some(name))
            .then(|| (path.clone(), manifest.clone()))
    })
}

/// A path under the repository's root, as a line names it.
fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// A unit the product has built: a manifest names it. `kind:name`, as a
/// rule's test declares what it needs.
#[must_use]
pub fn built(need: &str) -> bool {
    named(need).is_some()
}

/// Writes one outcome where the rules' gate reads it.
fn record(id: &str, state: &str, why: &str) {
    let test = std::thread::current().name().unwrap_or("?").to_string();
    let path = std::env::var("RULES_STATUS")
        .map_or_else(|_| root().join("target/rules-status.jsonl"), PathBuf::from);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let line =
        json!({ "rule": id, "family": "business", "test": test, "state": state, "why": why });
    // One write per line: tests run in parallel, and a line written in
    // pieces would interleave with another test's.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

/// Proves rule `id`: pending while a unit it `needs` is not built or no
/// server answers; else `body` runs on the server, and must hold. A test
/// that reached the server off its mounting is mis-mounted whatever its
/// body answered: it passes, saying so, and the rules' gate fails it apart.
///
/// # Panics
///
/// The rule does not hold: the test fails, as it must.
pub fn rule(id: &str, needs: &[&str], body: impl FnOnce(&Server) -> Result<(), String>) {
    let missing: Vec<&str> = needs.iter().copied().filter(|n| !built(n)).collect();
    if !missing.is_empty() {
        let why = format!("waiting for {}", missing.join(", "));
        eprintln!("{id} pending: {why}");
        record(id, "pending", &why);
        return;
    }
    let Ok(base) = std::env::var("RULES_SERVER_URL") else {
        let why = "no server: crates/server is not built, or no DATABASE_URL";
        eprintln!("{id} pending: {why}");
        record(id, "pending", why);
        return;
    };
    let server = Server {
        base: base.trim_end_matches('/').to_string(),
        needs: needs.iter().map(ToString::to_string).collect(),
        mis_mounted: RefCell::default(),
    };
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| body(&server)));
    let mis_mounted = server.mis_mounted.take();
    if !mis_mounted.is_empty() {
        let why = mis_mounted.join(" · ");
        eprintln!(
            "{id} mis-mounted: {why} — the test is mounted wrong, not the product: the rules' task fixes it"
        );
        record(id, "mis-mounted", &why);
        return;
    }
    match outcome {
        Ok(Ok(())) => record(id, "green", ""),
        Ok(Err(why)) => {
            record(id, "red", &why);
            panic!("{id} does not hold: {why}");
        }
        Err(cause) => {
            let why = cause
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| cause.downcast_ref::<&str>().map(ToString::to_string))
                .unwrap_or_else(|| "it panicked".to_string());
            record(id, "red", &why);
            panic::resume_unwind(cause);
        }
    }
}
