//! What a unit's tests would otherwise retype — installed by `harness
//! init-repo`, and the harness's: never edit it here, contest it.
//!
//! The first product's forty-eight `DataCapabilities` each carried their own
//! copy of the same helpers: the aggregate parsed, the plugins collected, the
//! engine planted over memory on a fixed clock, a `submit` writing the wire
//! body, a `settled` running the applier, a refusal unwrapped, a scratch
//! database created, migrated and seeded by hand, the planters' manifests as
//! JSON literals. Every copy drifted a little. They are here once:
//!
//! ```ignore
//! use platform_test_kit::{Bench, planter};
//! use serde_json::json;
//!
//! const AGGREGATE: &str = include_str!("../../../aggregates/Token/aggregate.json");
//!
//! #[tokio::test]
//! async fn a_moved_token_stands_on_its_new_hex() {
//!     let bench = Bench::new(
//!         include_str!("../data-capability.json"),
//!         AGGREGATE,
//!         [aggregate_token::plugin(), move_token::plugin()],
//!     )
//!     .planting(&planter("test.dropToken", "Token", "$tokenId", "insert"))
//!     .memory()
//!     .await;
//!     bench
//!         .plant("test.dropToken", [json!({ "tokenId": TOKEN, "q": 0, "r": 0 })])
//!         .await;
//!     bench
//!         .applied("map.moveToken", json!({ "q": 2, "r": 3 }), Some(TOKEN))
//!         .await;
//!     assert_eq!(bench.row(&format!("Token/{TOKEN}"))["q"], 2);
//! }
//! ```
//!
//! Over PostgreSQL, the same bench on a [`Scratch`]: a database of its own
//! (`platform-scratch-db`), migrated by the product's Data layer
//! (`data::migrate` over `crates/data/migrations/`), seeded with the rows the
//! tables' foreign keys ask for, dropped when the test ends, panicking or
//! not. A Capability's guard checks are [`read`]'s: what the gate
//! `capability-conventions` judges, said before the pull request.
//!
//! A bench submits as a surface of the product (`surfaces.json`, contract
//! S, read by [`surfaces`]): its default surface, or the one
//! [`Bench::surface`] names — never a role written by hand. While the
//! product declares no surfaces, as the fallback approver `dm`.
//!
//! A dev-dependency only: `platform-test-kit = { path = "<to the
//! root>/crates/platform/test-kit" }`. Every helper panics with what went
//! wrong: it runs in a test, where a panic is the failure.

#![deny(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery, missing_docs, rust_2018_idioms)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugin, Plugins, RefusalKind, Shared};
use harness_dataguard::postgres::{PgStore, ReadOnly};
use harness_dataguard::reads::Rows;
use harness_dataguard::store::{Ids, Store};
use harness_dataguard::tokio_postgres::Client;
use harness_dataguard::{Error, Result};
use platform_scratch_db::ScratchDb;
use serde_json::{Value, json};

pub mod surfaces;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Where every bench's clock starts, in seconds since the epoch.
pub const NOW: u64 = 1_792_000_000;

/// A fixture id in the house style, `<prefix>000000-0000-4000-8000-<n>`.
///
/// One prefix per kind of row, `n` the how-many-th — `uuid(0xc1, 1)` is
/// `c1000000-0000-4000-8000-000000000001`: a valid uuid that reads at a
/// glance in an assertion.
#[must_use]
pub fn uuid(prefix: u8, n: u64) -> String {
    format!("{prefix:02x}000000-0000-4000-8000-{n:012x}")
}

/// The product's root: this crate sits at `crates/platform/test-kit`.
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A file of the product, read from its root: the aggregate's document a
/// unit's tests plant it beside — a unit includes nothing outside its
/// folder (`scripts/isolation.sh`).
///
/// # Panics
///
/// When the file is missing.
#[must_use]
pub fn file(path: &str) -> String {
    let full = root().join(path);
    std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("the product's {}: {e}", full.display()))
}

/// The engine's caller for surface `name` of the product's `surfaces.json`
/// — its default when `None` — from `test`; the fallback approver while
/// the product declares none.
///
/// # Panics
///
/// When `surfaces.json` does not read, or names no such surface.
#[must_use]
pub fn surface_caller(name: Option<&str>) -> Caller {
    let declared = surfaces::read(&root())
        .transpose()
        .unwrap_or_else(|e| panic!("{e}"));
    let caller =
        surfaces::caller(declared.as_ref(), name, "test").unwrap_or_else(|e| panic!("{e}"));
    let roles: Vec<&str> = caller.roles.iter().map(String::as_str).collect();
    let permissions: Vec<&str> = caller.permissions.iter().map(String::as_str).collect();
    Engine::caller(&caller.by, &caller.from, &permissions, &roles)
}

/// A `DataCapability`'s manifest (contract F), parsed.
///
/// # Panics
///
/// When `json` is not one.
#[must_use]
pub fn manifest(json: &str) -> DataCapability {
    DataCapability::parse(json)
        .unwrap_or_else(|e| panic!("not a DataCapability manifest (contract F): {e}"))
}

/// An aggregate's document (contract I), parsed.
///
/// # Panics
///
/// When `json` is not one.
#[must_use]
pub fn aggregate(json: &str) -> Aggregate {
    Aggregate::parse(json).unwrap_or_else(|e| panic!("not an aggregate (contract I): {e}"))
}

/// The manifest of a test-only `DataCapability` that plants rows for a test:
/// `id` (`test.<verb>`) does `effect` on `aggregate`, its row named by
/// `target` (`$<payload field>`), callable by anyone, overwriting.
#[must_use]
pub fn planter(id: &str, aggregate: &str, target: &str, effect: &str) -> String {
    json!({
        "dataCapability": id,
        "owner": "dataguard",
        "version": 1,
        "effect": effect,
        "target": { "aggregate": aggregate, "id": target },
        "mode": "overwrite",
        "callableBy": ["*"],
    })
    .to_string()
}

/// The message of `answer`, a refusal of `kind`.
///
/// # Panics
///
/// When `answer` is anything else: queued, or refused otherwise.
#[must_use]
pub fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal, not {other:?}"),
    }
}

/// Asserts that `payload`, what a `Prepare` writes, holds only fields the
/// manifest's `touches` declares (`<Aggregate>.<field>`).
///
/// The engine judges derived and critical fields by `touches`, never by
/// the payload. A manifest touching nothing (an insert) writes a new row:
/// nothing to compare.
///
/// # Panics
///
/// When the manifest does not parse as JSON, or a field is undeclared.
pub fn writes_only_touches(manifest: &str, payload: &Row) {
    let json: Value =
        serde_json::from_str(manifest).unwrap_or_else(|e| panic!("the manifest is not JSON: {e}"));
    let touches: Vec<&str> = json["touches"]
        .as_array()
        .map(|all| all.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if touches.is_empty() {
        return;
    }
    let declared: Vec<&str> = touches
        .iter()
        .map(|touch| touch.rsplit_once('.').map_or(*touch, |(_, field)| field))
        .collect();
    for field in payload.keys() {
        assert!(
            declared.contains(&field.as_str()),
            "`{field}` is written but not declared in `touches` ({touches:?})"
        );
    }
}

/// The engine as the server plants a unit: its manifest beside its
/// aggregate, the test-only planters, the plugins — over memory
/// ([`Self::memory`]) or a [`Scratch`] ([`Self::postgres`]).
pub struct Bench {
    manifests: Vec<DataCapability>,
    aggregates: Vec<Aggregate>,
    plugins: Vec<Plugin>,
    shared: Option<Shared>,
    ids: Option<Arc<dyn Ids>>,
    caller: Caller,
}

impl Bench {
    /// The unit's manifest (`include_str!("../data-capability.json")`)
    /// beside its aggregate's document, with the plugins of both —
    /// `[aggregate_x::plugin(), my_unit::plugin()]`. Submissions come from
    /// `test`, as the default surface of the product's `surfaces.json`
    /// ([`surface_caller`]) — the caller `dm` holding `role:approver` while
    /// it declares none.
    ///
    /// # Panics
    ///
    /// When the manifest or the document does not parse, or
    /// `surfaces.json` does not read.
    #[must_use]
    pub fn new(manifest: &str, aggregate: &str, plugins: impl IntoIterator<Item = Plugin>) -> Self {
        Self {
            manifests: vec![crate::manifest(manifest)],
            aggregates: vec![crate::aggregate(aggregate)],
            plugins: plugins.into_iter().collect(),
            shared: None,
            ids: None,
            caller: surface_caller(None),
        }
    }

    /// Submits as surface `name` of `surfaces.json` — a player's, say —
    /// instead of the default one: the caller the shell sets on that
    /// surface's screens.
    ///
    /// # Panics
    ///
    /// When the product declares no such surface.
    #[must_use]
    pub fn surface(mut self, name: &str) -> Self {
        self.caller = surface_caller(Some(name));
        self
    }

    /// A test-only `DataCapability` beside the unit ([`planter`]).
    ///
    /// # Panics
    ///
    /// When the manifest does not parse.
    #[must_use]
    pub fn planting(mut self, manifest: &str) -> Self {
        self.manifests.push(crate::manifest(manifest));
        self
    }

    /// Another aggregate, one a planter writes.
    ///
    /// # Panics
    ///
    /// When the document does not parse.
    #[must_use]
    pub fn beside(mut self, aggregate: &str) -> Self {
        self.aggregates.push(crate::aggregate(aggregate));
        self
    }

    /// What the server shares with the units (a port's fake).
    #[must_use]
    pub fn sharing(mut self, shared: Shared) -> Self {
        self.shared = Some(shared);
        self
    }

    /// Where a fresh row id comes from, in place of the engine's counter.
    #[must_use]
    pub fn ids(mut self, ids: Arc<dyn Ids>) -> Self {
        self.ids = Some(ids);
        self
    }

    /// Who submits, in place of the surface's caller: an AI or a system
    /// caller, which no surface stands for.
    #[must_use]
    pub fn caller(mut self, caller: Caller) -> Self {
        self.caller = caller;
        self
    }

    /// The engine over `store`, on `clock`, with everything declared.
    fn engine(
        self,
        store: Arc<dyn Store>,
        clock: Arc<FixedClock>,
        plugins: Plugins,
    ) -> (Engine, Caller) {
        let Self {
            manifests,
            aggregates,
            shared,
            ids,
            caller,
            ..
        } = self;
        let mut engine = Engine::new(store, Arc::new(MemoryNotifier::default()), clock);
        for aggregate in aggregates {
            engine = engine.with_aggregate(aggregate);
        }
        for manifest in manifests {
            engine = engine.with_data_capability(manifest);
        }
        if let Some(shared) = shared {
            engine = engine.with_shared(shared);
        }
        if let Some(ids) = ids {
            engine = engine.with_ids(ids);
        }
        (engine.with_plugins(plugins), caller)
    }

    /// The engine over the in-memory store, loaded: `load` judged the
    /// manifest against its aggregate's document, as the server does.
    ///
    /// # Panics
    ///
    /// When the engine refuses what it was given.
    pub async fn memory(mut self) -> Served {
        let plugins: Plugins = std::mem::take(&mut self.plugins).into_iter().collect();
        let store = Arc::new(MemoryStore::default());
        let clock = Arc::new(FixedClock::at(NOW));
        let (engine, caller) = self.engine(store.clone(), clock.clone(), plugins);
        engine
            .load()
            .await
            .unwrap_or_else(|e| panic!("the engine refuses the unit beside its aggregate: {e}"));
        Served {
            engine,
            clock,
            store: Some(store),
            caller,
        }
    }

    /// The engine over `scratch`'s tables, loaded: the aggregates'
    /// projections warmed, the `Prepare`s reading SQL through a read-only
    /// connection of their own.
    ///
    /// # Panics
    ///
    /// When the store does not open or warm, or the engine refuses what it
    /// was given.
    pub async fn postgres(mut self, scratch: &Scratch) -> Served {
        let plugins: Plugins = std::mem::take(&mut self.plugins).into_iter().collect();
        let store = PgStore::open(scratch.connect().await)
            .await
            .unwrap_or_else(|e| panic!("the PostgreSQL store does not open: {e}"))
            .with_projections(&plugins)
            .await
            .unwrap_or_else(|e| panic!("a projection does not warm: {e}"));
        let rows: Arc<dyn Rows> = Arc::new(ReadOnly::new(scratch.connect().await));
        let clock = Arc::new(FixedClock::at(NOW));
        let (engine, caller) = self.engine(Arc::new(store), clock.clone(), plugins);
        let engine = engine.with_rows(rows);
        engine
            .load()
            .await
            .unwrap_or_else(|e| panic!("the engine refuses the unit beside its aggregate: {e}"));
        Served {
            engine,
            clock,
            store: None,
            caller,
        }
    }
}

/// A planted engine, and what a test does with it.
pub struct Served {
    /// The engine, for what the helpers do not cover.
    pub engine: Engine,
    /// Its clock, which a test moves (`clock.set(..)`).
    pub clock: Arc<FixedClock>,
    /// The in-memory store, over memory; `None` over PostgreSQL, whose
    /// tables are read through [`Scratch::rows`].
    pub store: Option<Arc<MemoryStore>>,
    caller: Caller,
}

impl Served {
    /// Submits `payload` to `data_capability` (`<id>` or `<id>@<version>`)
    /// on the row `target`, as the bench's caller: queued, or refused
    /// before anything is.
    ///
    /// # Errors
    ///
    /// What the engine answers: a [`Error::Refused`] from the unit's
    /// `Prepare`, or the guard's.
    ///
    /// # Panics
    ///
    /// When `payload` is neither a JSON object nor `null`.
    pub async fn submit(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
    ) -> Result<Submitted> {
        let payload = match payload {
            Value::Object(fields) => fields.into_iter().collect(),
            Value::Null => BTreeMap::new(),
            other => panic!("a payload is a JSON object, not {other}"),
        };
        self.engine
            .submit(Submission {
                data_capability: data_capability.to_string(),
                payload,
                target_id: target.map(str::to_string),
                caller: self.caller.clone(),
                ..Submission::default()
            })
            .await
    }

    /// Runs the applier once: what every queued command became.
    ///
    /// # Panics
    ///
    /// When the applier fails.
    pub async fn settled(&self) -> Vec<CommandResult> {
        self.engine
            .run_once()
            .await
            .unwrap_or_else(|e| panic!("the applier failed: {e}"))
    }

    /// One submission, queued then settled: what it became.
    ///
    /// # Panics
    ///
    /// When it is refused, or the applier settles anything but it alone.
    pub async fn applied(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
    ) -> CommandResult {
        if let Err(e) = self.submit(data_capability, payload, target).await {
            panic!("{data_capability} was refused: {e}");
        }
        let mut settled = self.settled().await;
        assert_eq!(settled.len(), 1, "one command settled: {settled:?}");
        settled.remove(0)
    }

    /// Plants a row per payload through the planter `data_capability`, each
    /// naming its own row, then settles them.
    ///
    /// # Panics
    ///
    /// When one is refused, or not applied.
    pub async fn plant(&self, data_capability: &str, payloads: impl IntoIterator<Item = Value>) {
        for payload in payloads {
            if let Err(e) = self.submit(data_capability, payload, None).await {
                panic!("the planter {data_capability} was refused: {e}");
            }
        }
        let planted = self.settled().await;
        assert!(
            planted.iter().all(|result| result.status == State::Applied),
            "every planted row applied: {planted:?}"
        );
    }

    /// The row of `partition` (`<Aggregate>/<id>`) as the store holds it:
    /// empty when there is none.
    ///
    /// # Panics
    ///
    /// Over PostgreSQL: its tables are read through [`Scratch::rows`].
    #[must_use]
    pub fn row(&self, partition: &str) -> Row {
        self.store.as_ref().map_or_else(
            || panic!("a PostgreSQL bench reads its tables through Scratch::rows"),
            |store| store.row_now(partition),
        )
    }
}

/// A test's database: created on `DATABASE_URL`'s server, migrated as the
/// server migrates it, dropped when the test ends — panicking or not.
pub struct Scratch {
    db: ScratchDb,
    client: Client,
}

impl Scratch {
    /// A database for the test `label` describes, every migration of
    /// `crates/data/migrations/` applied by the Data layer's `migrate`.
    ///
    /// # Panics
    ///
    /// When `DATABASE_URL` names no PostgreSQL that lets this role create a
    /// database, or a migration fails.
    pub async fn new(label: &str) -> Self {
        let db = ScratchDb::create(label)
            .await
            .unwrap_or_else(|e| panic!("DATABASE_URL names the harness's PostgreSQL: {e}"));
        let mut client = db
            .connect()
            .await
            .unwrap_or_else(|e| panic!("the scratch database answers: {e}"));
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .unwrap_or_else(|e| panic!("the migrations apply: {e}"));
        Self { db, client }
    }

    /// Runs `sql`, the rows the tables' foreign keys ask for before a test.
    ///
    /// # Panics
    ///
    /// When PostgreSQL refuses it.
    pub async fn seed(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("the seed is refused: {e}"));
    }

    /// The connection the database was migrated over.
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }

    /// `DATABASE_URL`, naming this database: for what opens its own pool.
    #[must_use]
    pub fn url(&self) -> &str {
        self.db.url()
    }

    /// A new connection to this database.
    ///
    /// # Panics
    ///
    /// When the server refuses it.
    pub async fn connect(&self) -> Client {
        self.db
            .connect()
            .await
            .unwrap_or_else(|e| panic!("the scratch database answers: {e}"))
    }

    /// What `sql` answers with `parameters`, each row a JSON object keyed
    /// by column — the mapping the engine's `Prepare`s read SQL with.
    ///
    /// # Panics
    ///
    /// When PostgreSQL refuses the query.
    pub async fn rows(&self, sql: &str, parameters: &[Value]) -> Vec<Value> {
        let answer = ReadOnly::new(self.connect().await)
            .rows(sql, parameters)
            .await
            .unwrap_or_else(|e| panic!("the query is refused: {e}"));
        match answer {
            Value::Array(rows) => rows,
            other => vec![other],
        }
    }
}

/// A Capability's guard checks: what the gate `capability-conventions`
/// judges of its `SQL`, its `VARIABLES` and its persisted query, said by a
/// test before the pull request.
pub mod read {
    use std::collections::BTreeSet;
    use std::path::Path;

    use serde_json::Value;

    /// The mock-up's answer for `capability`, `mock/fixtures/<id>.json`.
    ///
    /// # Panics
    ///
    /// When the file is missing or is not JSON.
    #[must_use]
    pub fn fixture(capability: &str) -> Value {
        fixture_at(&crate::root(), capability)
    }

    /// [`fixture`], under the product's root `root`.
    pub(crate) fn fixture_at(root: &Path, capability: &str) -> Value {
        let path = root
            .join("mock/fixtures")
            .join(format!("{capability}.json"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the mock-up's fixture {}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is JSON: {e}", path.display()))
    }

    /// The persisted query `crates/<system>/queries/<name>.graphql`, read
    /// from the root — a Capability's crate includes nothing outside its
    /// folder (`scripts/isolation.sh`).
    ///
    /// # Panics
    ///
    /// When the file is missing.
    #[must_use]
    pub fn query(system: &str, name: &str) -> String {
        query_at(&crate::root(), system, name)
    }

    /// [`query`], under the product's root `root`.
    pub(crate) fn query_at(root: &Path, system: &str, name: &str) -> String {
        let path = root.join(format!("crates/{system}/queries/{name}.graphql"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the persisted query {}: {e}", path.display()))
    }

    /// The words of `text`: runs of letters, digits and `_`.
    fn words(text: &str) -> impl Iterator<Item = &str> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| !word.is_empty())
    }

    /// Asserts that `sql` is one read statement: it starts with `SELECT` or
    /// `WITH`, holds no `;`, and no word that writes.
    ///
    /// # Panics
    ///
    /// When it is not.
    pub fn one_read_statement(sql: &str) {
        let upper = sql.trim_start().to_ascii_uppercase();
        assert!(
            upper.starts_with("SELECT") || upper.starts_with("WITH"),
            "the SQL starts with SELECT or WITH: {sql}"
        );
        assert!(!sql.contains(';'), "one statement, no `;`: {sql}");
        let writes = [
            "INSERT", "UPDATE", "DELETE", "MERGE", "ALTER", "DROP", "GRANT",
        ];
        for word in words(&upper) {
            assert!(!writes.contains(&word), "the SQL writes ({word}): {sql}");
        }
    }

    /// The parameters `$1`, `$2`… `sql` names, as numbers.
    fn parameters(sql: &str) -> BTreeSet<usize> {
        sql.split('$')
            .skip(1)
            .filter_map(|after| {
                let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }

    /// Asserts that `sql` binds exactly `$1`…`$n` for the `n` request
    /// variables, each compared as text.
    ///
    /// `<column>::text = $n`, or `= lower($n)`: an id that is no uuid then
    /// answers empty instead of failing. The registry binds `variables` by
    /// position.
    ///
    /// # Panics
    ///
    /// When a parameter is missing, extra, or compared otherwise.
    pub fn binds_in_order(sql: &str, variables: &[&str]) {
        let expected: BTreeSet<usize> = (1..=variables.len()).collect();
        assert_eq!(
            parameters(sql),
            expected,
            "the SQL binds $1..${} for {variables:?}: {sql}",
            variables.len()
        );
        for n in 1..=variables.len() {
            assert!(
                compared_as_text(sql, n),
                "${n} ({}) is compared as text, `<column>::text = ${n}`: {sql}",
                variables[n - 1]
            );
        }
    }

    /// Whether `sql` compares `$n` as text somewhere: `::text`, an `=`,
    /// then `$n` or `lower($n)`, spaces allowed between.
    fn compared_as_text(sql: &str, n: usize) -> bool {
        let wanted = n.to_string();
        sql.match_indices("::text").any(|(at, cast)| {
            let rest = sql[at + cast.len()..].trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                return false;
            };
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("lower(").map_or(rest, str::trim_start);
            rest.strip_prefix('$')
                .and_then(|after| after.strip_prefix(wanted.as_str()))
                .is_some_and(|after| !after.starts_with(|c: char| c.is_ascii_digit()))
        })
    }

    /// `camelCase` as `camel_case`.
    fn snake(name: &str) -> String {
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    /// Asserts that the persisted query `query` (its `.graphql` text) is
    /// the read `sql` makes: its variables are `variables`, in order, and
    /// every field it selects (`camelCase`) is a word of the SQL in
    /// `snake_case`.
    ///
    /// # Panics
    ///
    /// When they disagree.
    pub fn query_file_matches(sql: &str, query: &str, variables: &[&str]) {
        let body: Vec<&str> = query
            .lines()
            .skip_while(|line| !line.trim_start().starts_with("query"))
            .collect();
        let declared: Vec<String> = body
            .first()
            .map(|line| {
                line.split('$')
                    .skip(1)
                    .filter_map(|after| after.split(':').next())
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty() && !name.contains(' '))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            declared, variables,
            "the query file's variables are VARIABLES, in order"
        );
        let in_sql: BTreeSet<&str> = words(sql).collect();
        for line in body.iter().skip(1) {
            let field = line.trim();
            if field.is_empty() || !field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let column = snake(field);
            assert!(
                in_sql.contains(column.as_str()),
                "the query file selects `{field}`, and the SQL has no `{column}`: {sql}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_house_id_reads_at_a_glance() {
        assert_eq!(uuid(0xc1, 1), "c1000000-0000-4000-8000-000000000001");
        assert_eq!(uuid(0x0b, 10), "0b000000-0000-4000-8000-00000000000a");
    }

    #[test]
    fn a_planter_is_a_manifest_the_engine_reads() {
        let text = planter("test.dropToken", "Token", "$tokenId", "insert");
        let parsed = manifest(&text);
        assert_eq!(parsed.data_capability, "test.dropToken");
        let json: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(
            json["target"],
            json!({ "aggregate": "Token", "id": "$tokenId" })
        );
        assert_eq!(json["effect"], "insert");
        assert_eq!(json["mode"], "overwrite");
        assert_eq!(json["callableBy"], json!(["*"]));
    }

    const TOKEN: &str = r#"{"aggregate":"Token","fields":{"q":{"kind":"authoritative","criticality":"normal"}},"invariants":[],"relations":[]}"#;
    const MOVE: &str = r#"{"dataCapability":"map.moveToken","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Token","id":"$tokenId"},"touches":["Token.q"],"mode":"overwrite","callableBy":["*"]}"#;

    #[test]
    fn what_a_prepare_writes_is_held_to_the_touches() {
        let row = |field: &str| Row::from([(field.to_string(), json!(1))]);
        writes_only_touches(MOVE, &row("q"));
        let undeclared = std::panic::catch_unwind(|| writes_only_touches(MOVE, &row("r")));
        assert!(undeclared.is_err());
        writes_only_touches(&planter("t.p", "Token", "$id", "insert"), &row("anything"));
    }

    #[tokio::test]
    async fn a_bench_plants_moves_and_refuses_over_memory() {
        let bench = Bench::new(MOVE, TOKEN, [])
            .planting(&planter("test.dropToken", "Token", "$tokenId", "insert"))
            .sharing(Shared::default())
            .memory()
            .await;
        let id = uuid(0x70, 1);
        bench
            .plant("test.dropToken", [json!({ "tokenId": id, "q": 0 })])
            .await;
        let moved = bench
            .applied("map.moveToken@1", json!({ "q": 4 }), Some(&id))
            .await;
        assert_eq!(moved.status, State::Applied, "{moved:?}");
        assert_eq!(bench.row(&format!("Token/{id}"))["q"], 4);
        assert!(bench.row("Token/none").is_empty());
        assert_eq!(bench.settled().await.len(), 0, "nothing queued");
        let unknown = bench.submit("map.nothing", json!({}), None).await;
        assert!(unknown.is_err());
        let wrong = std::panic::catch_unwind(|| {
            refused(Err(Error::Store("x".into())), RefusalKind::NotFound)
        });
        assert!(wrong.is_err());
    }

    #[test]
    fn the_guard_checks_pass_a_canonical_read_and_name_a_drift() {
        let sql = "SELECT s.id::text AS id, s.planned_on FROM session s WHERE s.id::text = $1";
        read::one_read_statement(sql);
        read::binds_in_order(sql, &["id"]);
        let query = "# fields: session.id, session.plannedOn\nquery GetSession($id: ID!) {\n  session(id: $id) {\n    id\n    plannedOn\n  }\n}\n";
        read::query_file_matches(sql, query, &["id"]);
        for drift in [
            "UPDATE session SET id = $1",
            "SELECT 1; SELECT 2",
            "  delete from session",
        ] {
            assert!(
                std::panic::catch_unwind(|| read::one_read_statement(drift)).is_err(),
                "{drift}"
            );
        }
        assert!(
            std::panic::catch_unwind(|| read::binds_in_order(sql, &["id", "campaign"])).is_err()
        );
        assert!(
            std::panic::catch_unwind(|| read::binds_in_order("SELECT 1 WHERE id = $1", &["id"]))
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| read::binds_in_order(
                "SELECT 1 WHERE id::text = $12",
                &["id"]
            ))
            .is_err()
        );
        read::binds_in_order(
            "SELECT 1 WHERE a::text = lower($1) AND b::text = $2",
            &["a", "b"],
        );
        let extra = query.replace("plannedOn\n", "plannedOn\n    position\n");
        assert!(
            std::panic::catch_unwind(|| read::query_file_matches(sql, &extra, &["id"])).is_err()
        );
        assert!(
            std::panic::catch_unwind(|| read::query_file_matches(sql, query, &["session"]))
                .is_err()
        );
    }

    const TEST_ONLY_MOVE: &str = r#"{"dataCapability":"map.moveToken","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Token","id":"$tokenId"},"touches":["Token.q"],"mode":"overwrite","callableBy":["test"]}"#;

    #[test]
    fn a_product_file_is_read_from_the_root() {
        assert!(file("Cargo.toml").contains("[workspace]"));
        assert!(std::panic::catch_unwind(|| file("no/such/file.json")).is_err());
    }

    #[test]
    fn a_refusal_gives_its_message_only_for_its_kind() {
        let refusal = |kind| {
            Err(Error::Refused(harness_dataguard::plugins::Refusal {
                kind,
                message: "nope".to_string(),
            }))
        };
        assert_eq!(
            refused(refusal(RefusalKind::NotFound), RefusalKind::NotFound),
            "nope"
        );
        let other = std::panic::catch_unwind(|| {
            refused(refusal(RefusalKind::NotFound), RefusalKind::Unprocessable)
        });
        assert!(other.is_err());
    }

    #[tokio::test]
    async fn a_submission_carries_the_bench_caller() {
        let served = Bench::new(TEST_ONLY_MOVE, TOKEN, []).memory().await;
        let answer = served
            .submit("map.moveToken", json!({ "q": 1 }), Some("t"))
            .await;
        assert!(answer.is_ok(), "{answer:?}");
        let stranger = Bench::new(TEST_ONLY_MOVE, TOKEN, [])
            .caller(Engine::caller("dm", "elsewhere", &[], &[]))
            .memory()
            .await;
        let answer = stranger
            .submit("map.moveToken", json!({ "q": 1 }), Some("t"))
            .await;
        assert!(answer.is_err(), "{answer:?}");
    }

    #[tokio::test]
    async fn a_planted_row_stands_before_anything_moves_it() {
        let bench = Bench::new(MOVE, TOKEN, [])
            .planting(&planter("test.dropToken", "Token", "$tokenId", "insert"))
            .memory()
            .await;
        let id = uuid(0x71, 1);
        bench
            .plant("test.dropToken", [json!({ "tokenId": id, "q": 7 })])
            .await;
        assert_eq!(bench.row(&format!("Token/{id}"))["q"], 7);
    }

    #[tokio::test]
    async fn a_scratch_seeds_answers_and_names_its_database() {
        // A product's CI runs it on its PostgreSQL; a checkout with none
        // (the harness's own skeleton check) says so and skips it.
        if std::env::var("DATABASE_URL").is_err() {
            eprintln!("no DATABASE_URL: the scratch database is not checked here");
            return;
        }
        let scratch = Scratch::new("kit_scratch").await;
        scratch
            .seed("CREATE TABLE kit_probe (id int, name text); INSERT INTO kit_probe VALUES (1, 'a'), (2, 'b');")
            .await;
        let rows = scratch
            .rows("SELECT id, name FROM kit_probe ORDER BY id", &[])
            .await;
        assert_eq!(
            rows,
            vec![
                json!({ "id": 1, "name": "a" }),
                json!({ "id": 2, "name": "b" })
            ]
        );
        assert!(scratch.url().contains("kit_scratch"), "{}", scratch.url());
        let count = scratch
            .client()
            .query_one("SELECT count(*) FROM kit_probe", &[])
            .await
            .expect("count");
        assert_eq!(count.get::<_, i64>(0), 2);
    }

    /// A product root of its own, removed when the test ends.
    struct Root(std::path::PathBuf);

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_mock_up_fixture_and_the_query_file_are_read() {
        let root = Root(std::env::temp_dir().join(format!(
            "platform-test-kit-read-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        let write = |path: &str, text: &str| {
            let full = root.0.join(path);
            std::fs::create_dir_all(full.parent().expect("a parent")).expect("mkdir");
            std::fs::write(full, text).expect("write");
        };
        write("mock/fixtures/kit.getSettings.json", r#"{"provider": "x"}"#);
        write(
            "crates/kit/queries/getSettings.graphql",
            "query GetSettings {\n  provider\n}\n",
        );
        let fixture = read::fixture_at(&root.0, "kit.getSettings");
        assert_eq!(fixture, json!({ "provider": "x" }));
        let query = read::query_at(&root.0, "kit", "getSettings");
        assert!(query.contains("query GetSettings"), "{query}");
        assert!(std::panic::catch_unwind(|| read::fixture_at(&root.0, "kit.nothing")).is_err());
        assert!(std::panic::catch_unwind(|| read::query_at(&root.0, "kit", "nothing")).is_err());
        write("mock/fixtures/kit.broken.json", "{");
        assert!(std::panic::catch_unwind(|| read::fixture_at(&root.0, "kit.broken")).is_err());
    }

    /// The message a caught panic carried.
    fn panic_message(caught: &(dyn std::any::Any + Send)) -> String {
        caught
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| caught.downcast_ref::<&str>().map(ToString::to_string))
            .unwrap_or_default()
    }

    #[test]
    fn the_root_wrappers_read_under_the_product_root() {
        // No product file needed: an id no product has panics, and the
        // panic names the path under the product's root the wrapper read.
        let fixture = std::panic::catch_unwind(|| read::fixture("harness.absent"))
            .expect_err("no fixture harness.absent");
        let wanted = root().join("mock/fixtures/harness.absent.json");
        assert!(
            panic_message(fixture.as_ref()).contains(&wanted.display().to_string()),
            "{}",
            panic_message(fixture.as_ref())
        );
        let query = std::panic::catch_unwind(|| read::query("harness_absent", "absent"))
            .expect_err("no query harness_absent/absent");
        let wanted = root().join("crates/harness_absent/queries/absent.graphql");
        assert!(
            panic_message(query.as_ref()).contains(&wanted.display().to_string()),
            "{}",
            panic_message(query.as_ref())
        );
    }

    #[test]
    fn a_selected_field_may_start_with_a_capital() {
        let sql = "SELECT foo FROM t WHERE a::text = $1";
        let query = "query Q($id: ID!) {\n  Foo\n}\n";
        read::query_file_matches(sql, query, &["id"]);
        assert!(
            std::panic::catch_unwind(|| read::query_file_matches("SELECT _foo", query, &["id"]))
                .is_err()
        );
    }

    #[test]
    fn a_nameless_variable_is_not_a_declared_one() {
        let sql = "SELECT id";
        let query = "query Q($: ID!, $id: ID!) {\n  id\n}\n";
        read::query_file_matches(sql, query, &["id"]);
        let spaced = "query Q($a b: ID!, $id: ID!) {\n  id\n}\n";
        read::query_file_matches(sql, spaced, &["id"]);
    }

    #[test]
    fn a_bench_acts_as_the_product_s_surface_and_never_a_role_by_hand() {
        let declared = surfaces::read(&root()).transpose().expect("readable");
        let caller = surface_caller(None);
        match declared {
            None => {
                assert_eq!(caller.by, surfaces::FALLBACK_BY);
                assert_eq!(caller.roles, ["role:approver"]);
            }
            Some(declared) => {
                let default = &declared.surfaces[&declared.default];
                assert_eq!(caller.by, default.by);
                assert_eq!(caller.roles, default.roles);
            }
        }
        assert_eq!(caller.from, "test");
        assert!(
            std::panic::catch_unwind(|| surface_caller(Some("no-such-surface"))).is_err(),
            "an undeclared surface is the test's error"
        );
    }
}
